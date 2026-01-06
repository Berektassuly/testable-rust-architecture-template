//! Application service layer with graceful degradation.

use chrono::{Duration, Utc};
use std::sync::Arc;
use tracing::{error, info, instrument, warn};
use validator::Validate;

use crate::domain::{
    AppError, BlockchainClient, BlockchainStatus, CreateItemRequest, DatabaseClient,
    HealthResponse, HealthStatus, Item, PaginatedResponse, ValidationError,
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

    /// Create a new item and attempt blockchain submission.
    /// If blockchain is unavailable, stores item with pending_submission status.
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
        let mut item = self.db_client.create_item(request).await?;
        info!(item_id = %item.id, "Item created in database");

        let hash = self.generate_hash(&item);

        // Attempt blockchain submission with graceful degradation
        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(item_id = %item.id, signature = %signature, "Submitted to blockchain");
                self.db_client
                    .update_blockchain_status(
                        &item.id,
                        BlockchainStatus::Submitted,
                        Some(&signature),
                        None,
                        None,
                    )
                    .await?;
                item.blockchain_status = BlockchainStatus::Submitted;
                item.blockchain_signature = Some(signature);
            }
            Err(e) => {
                warn!(item_id = %item.id, error = ?e, "Blockchain submission failed, queuing for retry");
                let next_retry = Utc::now() + Duration::seconds(1);
                self.db_client
                    .update_blockchain_status(
                        &item.id,
                        BlockchainStatus::PendingSubmission,
                        None,
                        Some(&e.to_string()),
                        Some(next_retry),
                    )
                    .await?;
                item.blockchain_status = BlockchainStatus::PendingSubmission;
                item.blockchain_last_error = Some(e.to_string());
                item.blockchain_next_retry_at = Some(next_retry);
            }
        }

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

        let hash = self.generate_hash(&item);

        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(item_id = %item.id, signature = %signature, "Retry submission successful");
                self.db_client
                    .update_blockchain_status(
                        id,
                        BlockchainStatus::Submitted,
                        Some(&signature),
                        None,
                        None,
                    )
                    .await?;
                let mut updated_item = item;
                updated_item.blockchain_status = BlockchainStatus::Submitted;
                updated_item.blockchain_signature = Some(signature);
                updated_item.blockchain_last_error = None;
                updated_item.blockchain_next_retry_at = None;
                Ok(updated_item)
            }
            Err(e) => {
                warn!(item_id = %item.id, error = ?e, "Retry submission failed");
                let retry_count = self.db_client.increment_retry_count(id).await?;
                let (status, next_retry) = if retry_count >= MAX_RETRY_ATTEMPTS {
                    (BlockchainStatus::Failed, None)
                } else {
                    let backoff = calculate_backoff(retry_count);
                    (
                        BlockchainStatus::PendingSubmission,
                        Some(Utc::now() + Duration::seconds(backoff)),
                    )
                };

                self.db_client
                    .update_blockchain_status(id, status, None, Some(&e.to_string()), next_retry)
                    .await?;

                Err(e)
            }
        }
    }

    /// Process pending blockchain submissions (called by background worker)
    #[instrument(skip(self))]
    pub async fn process_pending_submissions(&self, batch_size: i64) -> Result<usize, AppError> {
        let pending_items = self
            .db_client
            .get_pending_blockchain_items(batch_size)
            .await?;
        let count = pending_items.len();

        if count == 0 {
            return Ok(0);
        }

        info!(count = count, "Processing pending blockchain submissions");

        for item in pending_items {
            if let Err(e) = self.process_single_submission(&item).await {
                error!(item_id = %item.id, error = ?e, "Failed to process pending submission");
            }
        }

        Ok(count)
    }

    /// Process a single pending submission
    async fn process_single_submission(&self, item: &Item) -> Result<(), AppError> {
        let hash = self.generate_hash(item);

        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(item_id = %item.id, signature = %signature, "Background submission successful");
                self.db_client
                    .update_blockchain_status(
                        &item.id,
                        BlockchainStatus::Submitted,
                        Some(&signature),
                        None,
                        None,
                    )
                    .await?;
            }
            Err(e) => {
                warn!(item_id = %item.id, error = ?e, "Background submission failed");
                let retry_count = self.db_client.increment_retry_count(&item.id).await?;
                let (status, next_retry) = if retry_count >= MAX_RETRY_ATTEMPTS {
                    (BlockchainStatus::Failed, None)
                } else {
                    let backoff = calculate_backoff(retry_count);
                    (
                        BlockchainStatus::PendingSubmission,
                        Some(Utc::now() + Duration::seconds(backoff)),
                    )
                };

                self.db_client
                    .update_blockchain_status(
                        &item.id,
                        status,
                        None,
                        Some(&e.to_string()),
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

    /// Generate a content hash for blockchain submission
    fn generate_hash(&self, item: &Item) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(item.id.as_bytes());
        hasher.update(item.name.as_bytes());
        hasher.update(item.content.as_bytes());
        if let Some(ref desc) = item.description {
            hasher.update(desc.as_bytes());
        }
        let result = hasher.finalize();
        result.iter().map(|b| format!("{:02x}", b)).collect()
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
    use crate::domain::{BlockchainError, BlockchainStatus, ValidationError};
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};
    use async_trait::async_trait;
    use std::sync::Mutex;
    // --- Local Mocks for specific scenario testing ---
    // We define local mocks to have precise control over return values and call tracking
    // without relying on the generic behavior of test_utils.

    struct ScenarioBlockchainClient {
        should_fail: bool,
        error_to_return: Option<BlockchainError>,
    }

    #[async_trait]
    impl BlockchainClient for ScenarioBlockchainClient {
        async fn submit_transaction(&self, _hash: &str) -> Result<String, AppError> {
            if self.should_fail {
                let err = self
                    .error_to_return
                    .clone()
                    .unwrap_or(BlockchainError::Connection("Simulated failure".into()));
                Err(AppError::Blockchain(err))
            } else {
                Ok("test_signature".to_string())
            }
        }

        async fn health_check(&self) -> Result<(), AppError> {
            if self.should_fail {
                Err(AppError::Blockchain(BlockchainError::Connection(
                    "Unhealthy".into(),
                )))
            } else {
                Ok(())
            }
        }
    }

    struct ScenarioDatabaseClient {
        items: Mutex<Vec<Item>>,
        retry_count_to_return: i32,
        get_item_response: Option<Item>,
    }

    #[async_trait]
    impl DatabaseClient for ScenarioDatabaseClient {
        async fn create_item(&self, req: &CreateItemRequest) -> Result<Item, AppError> {
            let item = Item {
                id: "test_id".to_string(),
                hash: "test_hash".to_string(),
                name: req.name.clone(),
                description: req.description.clone(),
                content: req.content.clone(),
                blockchain_status: BlockchainStatus::Pending,
                blockchain_signature: None,
                blockchain_last_error: None,
                blockchain_next_retry_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                blockchain_retry_count: 0,
                metadata: None,
            };
            self.items.lock().unwrap().push(item.clone());
            Ok(item)
        }

        async fn get_item(&self, _id: &str) -> Result<Option<Item>, AppError> {
            Ok(self.get_item_response.clone())
        }

        async fn list_items(
            &self,
            _limit: i64,
            _cursor: Option<&str>,
        ) -> Result<PaginatedResponse<Item>, AppError> {
            Ok(PaginatedResponse {
                items: vec![],
                next_cursor: None,
                has_more: false,
            })
        }

        async fn update_blockchain_status(
            &self,
            id: &str,
            status: BlockchainStatus,
            signature: Option<&str>,
            error: Option<&str>,
            next_retry: Option<chrono::DateTime<Utc>>,
        ) -> Result<(), AppError> {
            let mut items = self.items.lock().unwrap();
            if let Some(item) = items.iter_mut().find(|i| i.id == id) {
                item.blockchain_status = status;
                item.blockchain_signature = signature.map(String::from);
                item.blockchain_last_error = error.map(String::from);
                item.blockchain_next_retry_at = next_retry;
                Ok(())
            } else {
                Ok(())
            }
        }

        async fn increment_retry_count(&self, _id: &str) -> Result<i32, AppError> {
            Ok(self.retry_count_to_return)
        }

        async fn get_pending_blockchain_items(&self, _limit: i64) -> Result<Vec<Item>, AppError> {
            let items = self.items.lock().unwrap();
            Ok(items
                .iter()
                .filter(|i| i.blockchain_status == BlockchainStatus::PendingSubmission)
                .cloned()
                .collect())
        }

        async fn health_check(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

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
    async fn test_create_item_blockchain_failure_graceful_degradation() {
        // Setup DB that works
        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![]),
            retry_count_to_return: 0,
            get_item_response: None,
        });

        // Setup BC that fails
        let bc = Arc::new(ScenarioBlockchainClient {
            should_fail: true,
            error_to_return: Some(BlockchainError::Timeout("Connection timeout".into())),
        });

        let service = AppService::new(db, bc);

        let request = CreateItemRequest {
            name: "Test Item".to_string(),
            description: None,
            content: "Content".to_string(),
            metadata: None,
        };

        // Execution
        let result = service.create_and_submit_item(&request).await;

        // Verification
        assert!(result.is_ok());
        let item = result.unwrap();

        // Should return the item, but with PendingSubmission status instead of Submitted
        assert_eq!(item.blockchain_status, BlockchainStatus::PendingSubmission);
        assert!(item.blockchain_last_error.is_some());
        assert!(item.blockchain_next_retry_at.is_some());
        assert_eq!(item.blockchain_signature, None);
    }

    #[tokio::test]
    async fn test_retry_submission_invalid_state() {
        let mut valid_item = Item::default();
        valid_item.id = "valid_id".to_string();
        valid_item.blockchain_status = BlockchainStatus::Submitted; // Already submitted

        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![]),
            retry_count_to_return: 0,
            get_item_response: Some(valid_item),
        });
        let bc = Arc::new(MockBlockchainClient::new());
        let service = AppService::new(db, bc);

        let result = service.retry_blockchain_submission("valid_id").await;

        match result {
            Err(AppError::Validation(ValidationError::InvalidField { field, .. })) => {
                assert_eq!(field, "blockchain_status");
            }
            _ => panic!("Expected validation error for invalid item status"),
        }
    }

    #[tokio::test]
    async fn test_retry_submission_max_retries_reached() {
        // Prepare item in DB
        let mut item = Item::default();
        item.id = "retry_id".to_string();
        item.blockchain_status = BlockchainStatus::PendingSubmission;
        item.name = "Test".to_string();
        item.content = "Content".to_string();

        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![item.clone()]),
            retry_count_to_return: 10, // Max retries hit
            get_item_response: Some(item),
        });

        // Blockchain continues to fail
        let bc = Arc::new(ScenarioBlockchainClient {
            should_fail: true,
            error_to_return: None,
        });

        let service = AppService::new(db.clone(), bc);

        let result = service.retry_blockchain_submission("retry_id").await;

        assert!(result.is_err()); // Should return the underlying error

        // Verify DB update was called with Failed status
        let items = db.items.lock().unwrap();
        let updated_item = items.iter().find(|i| i.id == "retry_id").unwrap();
        assert_eq!(updated_item.blockchain_status, BlockchainStatus::Failed);
        assert!(updated_item.blockchain_next_retry_at.is_none());
    }

    #[tokio::test]
    async fn test_retry_submission_backoff_calculation() {
        // Prepare item
        let mut item = Item::default();
        item.id = "backoff_id".to_string();
        item.blockchain_status = BlockchainStatus::PendingSubmission;
        item.name = "Test".to_string();
        item.content = "Content".to_string();

        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![item.clone()]),
            retry_count_to_return: 3, // Should result in 2^3 = 8 seconds
            get_item_response: Some(item),
        });

        let bc = Arc::new(ScenarioBlockchainClient {
            should_fail: true,
            error_to_return: None,
        });

        let service = AppService::new(db.clone(), bc);

        let _ = service.retry_blockchain_submission("backoff_id").await;

        let items = db.items.lock().unwrap();
        let updated_item = items.iter().find(|i| i.id == "backoff_id").unwrap();

        assert_eq!(
            updated_item.blockchain_status,
            BlockchainStatus::PendingSubmission
        );
        assert!(updated_item.blockchain_next_retry_at.is_some());

        // Basic check that next_retry is in the future
        let next = updated_item.blockchain_next_retry_at.unwrap();
        assert!(next > Utc::now());
    }

    #[tokio::test]
    async fn test_process_pending_submissions_batch() {
        let mut item1 = Item::default();
        item1.id = "1".to_string();
        item1.name = "1".to_string();
        item1.content = "c".to_string();
        item1.blockchain_status = BlockchainStatus::PendingSubmission;

        let mut item2 = Item::default();
        item2.id = "2".to_string();
        item2.name = "2".to_string();
        item2.content = "c".to_string();
        item2.blockchain_status = BlockchainStatus::PendingSubmission;

        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![item1, item2]),
            retry_count_to_return: 0,
            get_item_response: None,
        });

        let bc = Arc::new(ScenarioBlockchainClient {
            should_fail: false,
            error_to_return: None,
        });

        let service = AppService::new(db.clone(), bc);

        let count = service.process_pending_submissions(10).await.unwrap();
        assert_eq!(count, 2);

        let items = db.items.lock().unwrap();
        for item in items.iter() {
            assert_eq!(item.blockchain_status, BlockchainStatus::Submitted);
            assert!(item.blockchain_signature.is_some());
        }
    }

    #[tokio::test]
    async fn test_health_check_mixed() {
        let db = Arc::new(ScenarioDatabaseClient {
            items: Mutex::new(vec![]),
            retry_count_to_return: 0,
            get_item_response: None,
        });

        // Blockchain is down
        let bc = Arc::new(ScenarioBlockchainClient {
            should_fail: true,
            error_to_return: None,
        });

        let service = AppService::new(db, bc);
        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Unhealthy);
    }
}
