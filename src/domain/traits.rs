//! Domain traits defining contracts for external systems.

use async_trait::async_trait;

use super::error::{BlockchainError, HealthCheckError, ItemError};
use super::types::{
    BlockchainStatus, CreateItemRequest, Item, OutboxStatus, PaginatedResponse, SolanaOutboxEntry,
    SolanaOutboxPayload,
};
use chrono::{DateTime, Utc};

/// Database client trait for persistence operations
#[async_trait]
pub trait DatabaseClient: Send + Sync {
    /// Check database connectivity
    async fn health_check(&self) -> Result<(), HealthCheckError>;

    /// Get a single item by ID
    async fn get_item(&self, id: &str) -> Result<Option<Item>, ItemError>;

    /// Create a new item
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, ItemError>;

    /// List items with cursor-based pagination
    async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, ItemError>;

    /// Update an existing item
    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, ItemError> {
        let _ = (id, data);
        Err(ItemError::InvalidState(
            "update_item not implemented".to_string(),
        ))
    }

    /// Delete an item
    async fn delete_item(&self, id: &str) -> Result<bool, ItemError> {
        let _ = id;
        Err(ItemError::InvalidState(
            "delete_item not implemented".to_string(),
        ))
    }

    /// Update blockchain status for an item
    async fn update_blockchain_status(
        &self,
        id: &str,
        status: BlockchainStatus,
        signature: Option<&str>,
        error: Option<&str>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ItemError>;

    /// Claim pending Solana outbox entries for processing
    async fn claim_pending_solana_outbox(
        &self,
        limit: i64,
    ) -> Result<Vec<SolanaOutboxEntry>, ItemError>;

    /// Mark a Solana outbox entry as completed and update item status
    async fn complete_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        signature: &str,
    ) -> Result<(), ItemError>;

    /// Mark a Solana outbox entry as failed and update item status
    async fn fail_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        retry_count: i32,
        outbox_status: OutboxStatus,
        item_status: BlockchainStatus,
        error: &str,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ItemError>;

    /// Enqueue a new Solana outbox entry for an existing item
    async fn enqueue_solana_outbox_for_item(
        &self,
        item_id: &str,
        payload: &SolanaOutboxPayload,
    ) -> Result<Item, ItemError>;

    /// Get items pending blockchain submission
    async fn get_pending_blockchain_items(&self, limit: i64) -> Result<Vec<Item>, ItemError>;

    /// Increment retry count for an item
    async fn increment_retry_count(&self, id: &str) -> Result<i32, ItemError>;
}

/// Blockchain client trait for chain operations
#[async_trait]
pub trait BlockchainClient: Send + Sync {
    /// Check blockchain RPC connectivity
    async fn health_check(&self) -> Result<(), HealthCheckError>;

    /// Submit a transaction with the given hash/memo
    async fn submit_transaction(&self, hash: &str) -> Result<String, BlockchainError>;

    /// Get transaction confirmation status
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, BlockchainError> {
        let _ = signature;
        Err(BlockchainError::SubmissionFailed(
            "get_transaction_status not implemented".to_string(),
        ))
    }

    /// Get current block height
    async fn get_block_height(&self) -> Result<u64, BlockchainError> {
        Err(BlockchainError::SubmissionFailed(
            "get_block_height not implemented".to_string(),
        ))
    }

    /// Get latest blockhash for transaction construction
    async fn get_latest_blockhash(&self) -> Result<String, BlockchainError> {
        Err(BlockchainError::SubmissionFailed(
            "get_latest_blockhash not implemented".to_string(),
        ))
    }

    /// Wait for transaction confirmation with timeout
    async fn wait_for_confirmation(
        &self,
        signature: &str,
        timeout_secs: u64,
    ) -> Result<bool, BlockchainError> {
        let _ = (signature, timeout_secs);
        Err(BlockchainError::SubmissionFailed(
            "wait_for_confirmation not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal implementation for testing default methods
    struct MinimalDatabaseClient;

    #[async_trait]
    impl DatabaseClient for MinimalDatabaseClient {
        async fn health_check(&self) -> Result<(), HealthCheckError> {
            Ok(())
        }

        async fn get_item(&self, _id: &str) -> Result<Option<Item>, ItemError> {
            Ok(None)
        }

        async fn create_item(&self, _data: &CreateItemRequest) -> Result<Item, ItemError> {
            Ok(Item::default())
        }

        async fn list_items(
            &self,
            _limit: i64,
            _cursor: Option<&str>,
        ) -> Result<PaginatedResponse<Item>, ItemError> {
            Ok(PaginatedResponse::empty())
        }

        async fn update_blockchain_status(
            &self,
            _id: &str,
            _status: BlockchainStatus,
            _signature: Option<&str>,
            _error: Option<&str>,
            _next_retry_at: Option<DateTime<Utc>>,
        ) -> Result<(), ItemError> {
            Ok(())
        }

        async fn claim_pending_solana_outbox(
            &self,
            _limit: i64,
        ) -> Result<Vec<SolanaOutboxEntry>, ItemError> {
            Ok(vec![])
        }

        async fn complete_solana_outbox(
            &self,
            _outbox_id: &str,
            _item_id: &str,
            _signature: &str,
        ) -> Result<(), ItemError> {
            Ok(())
        }

        async fn fail_solana_outbox(
            &self,
            _outbox_id: &str,
            _item_id: &str,
            _retry_count: i32,
            _outbox_status: OutboxStatus,
            _item_status: BlockchainStatus,
            _error: &str,
            _next_retry_at: Option<DateTime<Utc>>,
        ) -> Result<(), ItemError> {
            Ok(())
        }

        async fn enqueue_solana_outbox_for_item(
            &self,
            _item_id: &str,
            _payload: &SolanaOutboxPayload,
        ) -> Result<Item, ItemError> {
            Ok(Item::default())
        }

        async fn get_pending_blockchain_items(&self, _limit: i64) -> Result<Vec<Item>, ItemError> {
            Ok(vec![])
        }

        async fn increment_retry_count(&self, _id: &str) -> Result<i32, ItemError> {
            Ok(1)
        }
    }

    struct MinimalBlockchainClient;

    #[async_trait]
    impl BlockchainClient for MinimalBlockchainClient {
        async fn health_check(&self) -> Result<(), HealthCheckError> {
            Ok(())
        }

        async fn submit_transaction(&self, _hash: &str) -> Result<String, BlockchainError> {
            Ok("sig_123".to_string())
        }
    }

    #[tokio::test]
    async fn test_database_client_update_item_not_supported() {
        let client = MinimalDatabaseClient;
        let request = CreateItemRequest {
            name: "test".to_string(),
            description: None,
            content: "content".to_string(),
            metadata: None,
        };

        let result = client.update_item("id", &request).await;
        assert!(matches!(result, Err(ItemError::InvalidState(_))));
    }

    #[tokio::test]
    async fn test_database_client_delete_item_not_supported() {
        let client = MinimalDatabaseClient;
        let result = client.delete_item("id").await;
        assert!(matches!(result, Err(ItemError::InvalidState(_))));
    }

    #[tokio::test]
    async fn test_blockchain_client_get_transaction_status_not_supported() {
        let client = MinimalBlockchainClient;
        let result = client.get_transaction_status("sig").await;
        assert!(matches!(result, Err(BlockchainError::SubmissionFailed(_))));
    }

    #[tokio::test]
    async fn test_blockchain_client_get_block_height_not_supported() {
        let client = MinimalBlockchainClient;
        let result = client.get_block_height().await;
        assert!(matches!(result, Err(BlockchainError::SubmissionFailed(_))));
    }

    #[tokio::test]
    async fn test_blockchain_client_get_latest_blockhash_not_supported() {
        let client = MinimalBlockchainClient;
        let result = client.get_latest_blockhash().await;
        assert!(matches!(result, Err(BlockchainError::SubmissionFailed(_))));
    }

    #[tokio::test]
    async fn test_blockchain_client_wait_for_confirmation_not_supported() {
        let client = MinimalBlockchainClient;
        let result = client.wait_for_confirmation("sig", 30).await;
        assert!(matches!(result, Err(BlockchainError::SubmissionFailed(_))));
    }
}
