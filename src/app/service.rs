//! Application service layer with graceful degradation.

use chrono::{Duration, Utc};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};
use validator::Validate;

use crate::domain::{
    AppError, BlockchainClient, BlockchainStatus, CreateItemRequest, DatabaseClient,
    HealthResponse, HealthStatus, Item, OutboxStatus, PaginatedResponse, SolanaOutboxEntry,
    ValidationError, build_solana_outbox_payload_from_item,
};

/// Maximum number of retry attempts for blockchain submission
const MAX_RETRY_ATTEMPTS: i32 = 10;

/// Maximum backoff duration in seconds (5 minutes)
const MAX_BACKOFF_SECS: i64 = 300;

/// Application service containing business logic
pub struct AppService {
    db_client: Arc<dyn DatabaseClient>,
    blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppService {
    #[must_use]
    pub fn new(
        db_client: Arc<dyn DatabaseClient>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            db_client,
            blockchain_client,
        }
    }

    /// Create a new item and enqueue blockchain submission in the outbox.
    #[instrument(skip(self, request), fields(item_name = %request.name))]
    pub async fn create_and_submit_item(
        &self,
        request: &CreateItemRequest,
    ) -> Result<Item, AppError> {
        request.validate().map_err(|e| {
            warn!(error = %e, "Validation failed");
            AppError::Validation(ValidationError::Multiple(e.to_string()))
        })?;

        info!("Creating new item: {}", request.name);
        let item = self.db_client.create_item(request).await?;
        info!(item_id = %item.id, "Item created and outbox queued");

        Ok(item)
    }

    /// Get an item by ID
    #[instrument(skip(self))]
    pub async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        self.db_client.get_item(id).await
    }

    /// List items with pagination
    #[instrument(skip(self))]
    pub async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, AppError> {
        self.db_client.list_items(limit, cursor).await
    }

    /// Retry blockchain submission for a specific item
    #[instrument(skip(self))]
    pub async fn retry_blockchain_submission(&self, id: &str) -> Result<Item, AppError> {
        let item = self.db_client.get_item(id).await?.ok_or_else(|| {
            AppError::Database(crate::domain::DatabaseError::NotFound(id.to_string()))
        })?;

        if item.blockchain_status != BlockchainStatus::PendingSubmission
            && item.blockchain_status != BlockchainStatus::Failed
        {
            return Err(AppError::Validation(ValidationError::InvalidField {
                field: "blockchain_status".to_string(),
                message: "Item is not pending submission or failed".to_string(),
            }));
        }

        if item.blockchain_status == BlockchainStatus::PendingSubmission {
            info!(item_id = %item.id, "Item already queued for submission");
            return Ok(item);
        }

        let payload = build_solana_outbox_payload_from_item(&item);
        let updated = self
            .db_client
            .enqueue_solana_outbox_for_item(&item.id, &payload)
            .await?;

        Ok(updated)
    }

    /// Process pending blockchain submissions (called by background worker)
    #[instrument(skip(self))]
    pub async fn process_pending_submissions(&self, batch_size: i64) -> Result<usize, AppError> {
        let pending_entries = self
            .db_client
            .claim_pending_solana_outbox(batch_size)
            .await?;
        let count = pending_entries.len();

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

    /// Process a single pending submission
    async fn process_outbox_entry(&self, entry: &SolanaOutboxEntry) -> Result<(), AppError> {
        let hash = &entry.payload.hash;

        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(
                    outbox_id = %entry.id,
                    item_id = %entry.aggregate_id,
                    signature = %signature,
                    "Background submission successful"
                );
                self.db_client
                    .complete_solana_outbox(&entry.id, &entry.aggregate_id, &signature)
                    .await?;
            }
            Err(e) => {
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

                self.db_client
                    .fail_solana_outbox(
                        &entry.id,
                        &entry.aggregate_id,
                        retry_count,
                        outbox_status,
                        item_status,
                        &e.to_string(),
                        next_retry,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Perform health check on all dependencies
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> HealthResponse {
        let db_health = match self.db_client.health_check().await {
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
    use crate::domain::{BlockchainStatus, ValidationError};
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};
    use chrono::Utc;
    use std::sync::Arc;

    // --- Tests ---

    #[tokio::test]
    async fn test_create_item_validation_error() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        // Name too short/empty assumes validation logic in CreateItemRequest
        // We simulate a request that fails validator::Validate
        let request = CreateItemRequest {
            name: "".to_string(), // Invalid
            description: None,
            content: "content".to_string(),
            metadata: None,
        };

        let result = service.create_and_submit_item(&request).await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn test_create_item_does_not_submit_blockchain() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::failing("Chain down"));
        let service = AppService::new(db, bc);

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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db.clone(), bc);

        let request = CreateItemRequest::new("Test".to_string(), "Content".to_string());
        let created = db.create_item(&request).await.unwrap();
        db.update_blockchain_status(
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
            Err(AppError::Validation(ValidationError::InvalidField { field, .. })) => {
                assert_eq!(field, "blockchain_status");
            }
            _ => panic!("Expected validation error for invalid item status"),
        }
    }

    #[tokio::test]
    async fn test_retry_submission_failed_requeues() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db.clone(), bc);

        let request = CreateItemRequest::new("Retry".to_string(), "Content".to_string());
        let created = db.create_item(&request).await.unwrap();
        db.update_blockchain_status(
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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db.clone(), bc);

        let request1 = CreateItemRequest::new("Item1".to_string(), "Content".to_string());
        let request2 = CreateItemRequest::new("Item2".to_string(), "Content".to_string());
        let item1 = service.create_and_submit_item(&request1).await.unwrap();
        let item2 = service.create_and_submit_item(&request2).await.unwrap();

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 2);

        let updated1 = db.get_item(&item1.id).await.unwrap().unwrap();
        let updated2 = db.get_item(&item2.id).await.unwrap().unwrap();

        assert_eq!(updated1.blockchain_status, BlockchainStatus::Submitted);
        assert_eq!(updated2.blockchain_status, BlockchainStatus::Submitted);
        assert!(updated1.blockchain_signature.is_some());
        assert!(updated2.blockchain_signature.is_some());
    }

    #[tokio::test]
    async fn test_health_check_mixed() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::failing("unhealthy"));

        let service = AppService::new(db, bc);
        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_retry_blockchain_submission_item_not_found() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        let result = service.retry_blockchain_submission("nonexistent").await;

        assert!(matches!(
            result,
            Err(AppError::Database(crate::domain::DatabaseError::NotFound(
                _
            )))
        ));
    }

    #[tokio::test]
    async fn test_retry_blockchain_submission_failed_status() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db.clone(), bc);

        let request = CreateItemRequest::new("Failed".to_string(), "Content".to_string());
        let created = db.create_item(&request).await.unwrap();
        db.update_blockchain_status(
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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_get_item_success() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        let request = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        let result = service.get_item(&created.id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_list_items_success() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        let result = service.list_items(10, None).await.unwrap();
        assert!(result.items.is_empty());
        assert!(!result.has_more);
    }

    #[tokio::test]
    async fn test_create_item_blockchain_success() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db.clone(), bc);
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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);
        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Healthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_process_pending_submissions_failure_updates_retry() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::failing("rpc error"));
        let service = AppService::new(db.clone(), bc);

        let request = CreateItemRequest::new("Retry Item".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 1);

        let updated = db.get_item(&created.id).await.unwrap().unwrap();
        assert_eq!(
            updated.blockchain_status,
            BlockchainStatus::PendingSubmission
        );
        assert!(updated.blockchain_last_error.is_some());
        assert!(updated.blockchain_next_retry_at.is_some());
        assert_eq!(updated.blockchain_retry_count, 1);
        assert!(updated.blockchain_next_retry_at.unwrap() > Utc::now());
    }
}
