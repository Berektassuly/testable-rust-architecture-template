//! Application service layer with graceful degradation.

use chrono::{Duration, Utc};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};
use validator::Validate;

use crate::domain::{
    BlockchainClient, BlockchainError, BlockchainStatus, CreateItemRequest, HealthResponse,
    HealthStatus, Item, ItemError, ItemRepository, OutboxRepository, OutboxStatus,
    PaginatedResponse, SolanaOutboxEntry, ValidationError, build_solana_outbox_payload_from_item,
};

/// Error type for create-item flow (validation or repository).
#[derive(Debug)]
pub enum CreateItemError {
    Validation(ValidationError),
    Item(ItemError),
}

impl From<ValidationError> for CreateItemError {
    fn from(e: ValidationError) -> Self {
        CreateItemError::Validation(e)
    }
}

impl From<ItemError> for CreateItemError {
    fn from(e: ItemError) -> Self {
        CreateItemError::Item(e)
    }
}

/// Error type for outbox processing (repository or blockchain).
#[derive(Debug)]
pub enum ProcessError {
    Item(ItemError),
    Blockchain(crate::domain::BlockchainError),
}

impl From<ItemError> for ProcessError {
    fn from(e: ItemError) -> Self {
        ProcessError::Item(e)
    }
}

impl From<crate::domain::BlockchainError> for ProcessError {
    fn from(e: crate::domain::BlockchainError) -> Self {
        ProcessError::Blockchain(e)
    }
}

/// Maximum number of retry attempts for blockchain submission
const MAX_RETRY_ATTEMPTS: i32 = 10;

/// Maximum backoff duration in seconds (5 minutes)
const MAX_BACKOFF_SECS: i64 = 300;

/// Application service containing business logic
pub struct AppService {
    item_repo: Arc<dyn ItemRepository>,
    outbox_repo: Arc<dyn OutboxRepository>,
    blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppService {
    #[must_use]
    pub fn new(
        item_repo: Arc<dyn ItemRepository>,
        outbox_repo: Arc<dyn OutboxRepository>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            item_repo,
            outbox_repo,
            blockchain_client,
        }
    }

    /// Create a new item and enqueue blockchain submission in the outbox.
    #[instrument(skip(self, request), fields(item_name = %request.name))]
    pub async fn create_and_submit_item(
        &self,
        request: &CreateItemRequest,
    ) -> Result<Item, CreateItemError> {
        request.validate().map_err(|e| {
            warn!(error = %e, "Validation failed");
            CreateItemError::Validation(ValidationError::from(e))
        })?;

        info!("Creating new item: {}", request.name);
        let item = self.item_repo.create_item(request).await?;
        info!(item_id = %item.id, "Item created and outbox queued");

        Ok(item)
    }

    /// Get an item by ID
    #[instrument(skip(self))]
    pub async fn get_item(&self, id: &str) -> Result<Option<Item>, ItemError> {
        self.item_repo.get_item(id).await
    }

    /// List items with pagination
    #[instrument(skip(self))]
    pub async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, ItemError> {
        self.item_repo.list_items(limit, cursor).await
    }

    /// Retry blockchain submission for a specific item
    #[instrument(skip(self))]
    pub async fn retry_blockchain_submission(&self, id: &str) -> Result<Item, ItemError> {
        let item = self
            .item_repo
            .get_item(id)
            .await?
            .ok_or_else(|| ItemError::NotFound(id.to_string()))?;

        if item.blockchain_status != BlockchainStatus::PendingSubmission
            && item.blockchain_status != BlockchainStatus::Failed
        {
            return Err(ItemError::InvalidState(
                "Item is not pending submission or failed".to_string(),
            ));
        }

        if item.blockchain_status == BlockchainStatus::PendingSubmission {
            info!(item_id = %item.id, "Item already queued for submission");
            return Ok(item);
        }

        let payload = build_solana_outbox_payload_from_item(&item);
        let updated = self
            .item_repo
            .enqueue_solana_outbox_for_item(&item.id, &payload)
            .await?;

        Ok(updated)
    }

    /// Process pending blockchain submissions (called by background worker)
    #[instrument(skip(self))]
    pub async fn process_pending_submissions(&self, batch_size: i64) -> Result<usize, ItemError> {
        let pending_entries = self
            .outbox_repo
            .claim_pending_solana_outbox(batch_size)
            .await?;
        let count = pending_entries.len();

        metrics::gauge!("outbox_pending_items_count").set(count as f64);

        if count == 0 {
            return Ok(0);
        }

        info!(count = count, "Processing pending blockchain submissions");

        for entry in pending_entries {
            if let Err(e) = self.process_outbox_entry(&entry).await {
                error!(
                    outbox_id = %entry.id,
                    item_id = %entry.aggregate_id,
                    error = ?e,
                    "Failed to process pending submission"
                );
            }
        }

        Ok(count)
    }

    /// Process a single pending submission (sticky blockhash for idempotent retries).
    async fn process_outbox_entry(&self, entry: &SolanaOutboxEntry) -> Result<(), ProcessError> {
        let hash = &entry.payload.hash;
        let existing_blockhash = entry.attempt_blockhash.as_deref();

        match self
            .blockchain_client
            .submit_transaction(hash, existing_blockhash)
            .await
        {
            Ok((signature, _blockhash_used)) => {
                info!(
                    outbox_id = %entry.id,
                    item_id = %entry.aggregate_id,
                    signature = %signature,
                    "Background submission successful"
                );
                self.outbox_repo
                    .complete_solana_outbox(&entry.id, &entry.aggregate_id, &signature)
                    .await?;
            }
            Err(e) => {
                metrics::counter!("blockchain_submission_retry_total").increment(1);
                warn!(
                    outbox_id = %entry.id,
                    item_id = %entry.aggregate_id,
                    error = ?e,
                    "Background submission failed"
                );
                let retry_count = entry.retry_count + 1;
                let (outbox_status, item_status, next_retry) = if retry_count >= MAX_RETRY_ATTEMPTS
                {
                    (OutboxStatus::Failed, BlockchainStatus::Failed, None)
                } else {
                    let backoff = calculate_backoff(retry_count);
                    (
                        OutboxStatus::Pending,
                        BlockchainStatus::PendingSubmission,
                        Some(Utc::now() + Duration::seconds(backoff)),
                    )
                };

                // CV-01 remediation: Sticky blockhash to prevent double-spend.
                // We MUST NOT clear attempt_blockhash on Timeout, NetworkError, or
                // SubmissionFailed, because the transaction may have landed on-chain
                // despite the error. Clearing would cause the next retry to use a new
                // blockhash and produce a new signature, risking double-spend.
                // Only clear when we know the blockhash is invalid (BlockhashExpired).
                // For errors that don't carry blockhash_used, pass None so we do not
                // update the column and thus keep the existing value (safe default).
                let attempt_blockhash = match &e {
                    BlockchainError::BlockhashExpired => Some(None),
                    BlockchainError::SubmissionFailedWithBlockhash { blockhash_used, .. } => {
                        Some(Some(blockhash_used.as_str()))
                    }
                    BlockchainError::Timeout { blockhash, .. }
                    | BlockchainError::NetworkError { blockhash, .. } => {
                        Some(Some(blockhash.as_str()))
                    }
                    BlockchainError::SubmissionFailed(_) | BlockchainError::InsufficientFunds => {
                        None
                    }
                };

                self.outbox_repo
                    .fail_solana_outbox(
                        &entry.id,
                        &entry.aggregate_id,
                        retry_count,
                        outbox_status,
                        item_status,
                        &e.to_string(),
                        next_retry,
                        attempt_blockhash,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Perform health check on all dependencies
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> HealthResponse {
        let db_health = match self.item_repo.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        };
        let blockchain_health = match self.blockchain_client.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        };
        HealthResponse::new(db_health, blockchain_health)
    }
}

/// Calculate exponential backoff with maximum cap
fn calculate_backoff(retry_count: i32) -> i64 {
    let backoff = 2_i64.pow(retry_count.min(8) as u32);
    backoff.min(MAX_BACKOFF_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_backoff() {
        assert_eq!(calculate_backoff(0), 1);
        assert_eq!(calculate_backoff(1), 2);
        assert_eq!(calculate_backoff(2), 4);
        assert_eq!(calculate_backoff(3), 8);
        assert_eq!(calculate_backoff(4), 16);
        assert_eq!(calculate_backoff(5), 32);
        assert_eq!(calculate_backoff(6), 64);
        assert_eq!(calculate_backoff(7), 128);
        assert_eq!(calculate_backoff(8), 256);
        assert_eq!(calculate_backoff(9), 256); // Capped at 2^8
        assert_eq!(calculate_backoff(10), 256);
    }
}
#[cfg(test)]
mod service_tests {
    use super::*;
    use crate::domain::BlockchainStatus;
    use crate::test_utils::{MockBlockchainClient, MockProvider, mock_repos};
    use chrono::Utc;
    use std::sync::Arc;

    // --- Tests ---

    #[tokio::test]
    async fn test_create_item_validation_error() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        // Name too short/empty assumes validation logic in CreateItemRequest
        // We simulate a request that fails validator::Validate
        let request = CreateItemRequest {
            name: "".to_string(), // Invalid
            description: None,
            content: "content".to_string(),
            metadata: None,
        };

        let result = service.create_and_submit_item(&request).await;
        assert!(matches!(result, Err(CreateItemError::Validation(_))));
    }

    #[tokio::test]
    async fn test_create_item_does_not_submit_blockchain() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::failing("Chain down"));
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest {
            name: "Test Item".to_string(),
            description: None,
            content: "Content".to_string(),
            metadata: None,
        };

        let result = service.create_and_submit_item(&request).await;

        assert!(result.is_ok());
        let item = result.unwrap();

        // Item should be queued for submission, no immediate blockchain attempt
        assert_eq!(item.blockchain_status, BlockchainStatus::PendingSubmission);
        assert!(item.blockchain_signature.is_none());
        assert!(item.blockchain_last_error.is_none());
        assert!(item.blockchain_next_retry_at.is_none());
    }

    #[tokio::test]
    async fn test_retry_submission_invalid_state() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest::new("Test".to_string(), "Content".to_string());
        let created = mock.create_item(&request).await.unwrap();
        mock.update_blockchain_status(
            &created.id,
            BlockchainStatus::Submitted,
            Some("sig"),
            None,
            None,
        )
        .await
        .unwrap();

        let result = service.retry_blockchain_submission(&created.id).await;

        match result {
            Err(ItemError::InvalidState(msg)) => {
                assert!(msg.contains("not pending submission"));
            }
            _ => panic!("Expected invalid state error for invalid item status"),
        }
    }

    #[tokio::test]
    async fn test_retry_submission_failed_requeues() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest::new("Retry".to_string(), "Content".to_string());
        let created = mock.create_item(&request).await.unwrap();
        mock.update_blockchain_status(
            &created.id,
            BlockchainStatus::Failed,
            None,
            Some("previous failure"),
            None,
        )
        .await
        .unwrap();

        let updated = service
            .retry_blockchain_submission(&created.id)
            .await
            .unwrap();
        assert_eq!(
            updated.blockchain_status,
            BlockchainStatus::PendingSubmission
        );
        assert!(updated.blockchain_last_error.is_none());
        assert!(updated.blockchain_next_retry_at.is_none());
        assert_eq!(updated.blockchain_retry_count, 0);
    }

    #[tokio::test]
    async fn test_process_pending_submissions_batch() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request1 = CreateItemRequest::new("Item1".to_string(), "Content".to_string());
        let request2 = CreateItemRequest::new("Item2".to_string(), "Content".to_string());
        let item1 = service.create_and_submit_item(&request1).await.unwrap();
        let item2 = service.create_and_submit_item(&request2).await.unwrap();

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 2);

        let updated1 = mock.get_item(&item1.id).await.unwrap().unwrap();
        let updated2 = mock.get_item(&item2.id).await.unwrap().unwrap();

        assert_eq!(updated1.blockchain_status, BlockchainStatus::Submitted);
        assert_eq!(updated2.blockchain_status, BlockchainStatus::Submitted);
        assert!(updated1.blockchain_signature.is_some());
        assert!(updated2.blockchain_signature.is_some());
    }

    #[tokio::test]
    async fn test_health_check_mixed() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, _outbox_repo) = mock_repos(&mock);
        let other = Arc::new(MockProvider::new());
        let (_, outbox_repo2) = mock_repos(&other);
        let bc = Arc::new(MockBlockchainClient::failing("unhealthy"));
        let service = AppService::new(item_repo, outbox_repo2, bc);
        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_retry_blockchain_submission_item_not_found() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let result = service.retry_blockchain_submission("nonexistent").await;

        assert!(matches!(result, Err(ItemError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_retry_blockchain_submission_failed_status() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest::new("Failed".to_string(), "Content".to_string());
        let created = mock.create_item(&request).await.unwrap();
        mock.update_blockchain_status(
            &created.id,
            BlockchainStatus::Failed,
            None,
            Some("failed"),
            None,
        )
        .await
        .unwrap();

        let result = service.retry_blockchain_submission(&created.id).await;
        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(
            updated.blockchain_status,
            BlockchainStatus::PendingSubmission
        );
    }

    #[tokio::test]
    async fn test_process_pending_submissions_empty() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_get_item_success() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        let result = service.get_item(&created.id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_list_items_success() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);

        let result = service.list_items(10, None).await.unwrap();
        assert!(result.items.is_empty());
        assert!(!result.has_more);
    }

    #[tokio::test]
    async fn test_create_item_blockchain_success() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);
        let request = CreateItemRequest {
            name: "Success Item".to_string(),
            description: Some("Description".to_string()),
            content: "Content".to_string(),
            metadata: None,
        };

        let result = service.create_and_submit_item(&request).await;
        assert!(result.is_ok());

        let item = result.unwrap();
        assert_eq!(item.blockchain_status, BlockchainStatus::PendingSubmission);
        assert!(item.blockchain_signature.is_none());
    }

    #[tokio::test]
    async fn test_health_check_both_healthy() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(item_repo, outbox_repo, bc);
        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Healthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_process_pending_submissions_failure_updates_retry() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::failing("rpc error"));
        let service = AppService::new(item_repo, outbox_repo, bc);

        let request = CreateItemRequest::new("Retry Item".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 1);

        let updated = mock.get_item(&created.id).await.unwrap().unwrap();
        assert_eq!(
            updated.blockchain_status,
            BlockchainStatus::PendingSubmission
        );
        assert!(updated.blockchain_last_error.is_some());
        assert!(updated.blockchain_next_retry_at.is_some());
        assert_eq!(updated.blockchain_retry_count, 1);
        assert!(updated.blockchain_next_retry_at.unwrap() > Utc::now());
    }

    #[tokio::test]
    async fn test_double_spend_protection_on_timeout() {
        // Setup mock with timeout failure that carries a sticky blockhash
        let sticky_hash = "sticky_test_hash_abc";
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);

        let bc = Arc::new(MockBlockchainClient::timeout_with_blockhash(sticky_hash));
        let service = AppService::new(item_repo, outbox_repo, bc);

        // Create item and trigger processing
        let request = CreateItemRequest::new("Sticky Item".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        // Process submissions - should fail with Timeout but persist the blockhash
        service.process_pending_submissions(10).await.unwrap();

        // Verification
        let entries = mock.get_all_outbox_entries();
        let entry = entries
            .iter()
            .find(|e| e.aggregate_id == created.id)
            .unwrap();

        // Assert: retry_count incremented, status still Pending (for retry)
        assert_eq!(entry.retry_count, 1);
        assert_eq!(entry.status, OutboxStatus::Pending);

        // CRITICAL ASSERTION: attempt_blockhash MUST match the sticky hash from the error
        assert_eq!(
            entry.attempt_blockhash,
            Some(sticky_hash.to_string()),
            "Blockhash must be persisted after timeout to prevent double spend"
        );
    }
}
