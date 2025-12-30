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
