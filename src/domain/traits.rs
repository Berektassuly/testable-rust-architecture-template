//! Domain traits defining contracts for external systems.

use async_trait::async_trait;

use super::error::AppError;
use super::types::{BlockchainStatus, CreateItemRequest, Item, PaginatedResponse};
use chrono::{DateTime, Utc};

/// Database client trait for persistence operations
#[async_trait]
pub trait DatabaseClient: Send + Sync {
    /// Check database connectivity
    async fn health_check(&self) -> Result<(), AppError>;

    /// Get a single item by ID
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError>;

    /// Create a new item
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError>;

    /// List items with cursor-based pagination
    async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, AppError>;

    /// Update an existing item
    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, AppError> {
        let _ = (id, data);
        Err(AppError::NotSupported(
            "update_item not implemented".to_string(),
        ))
    }

    /// Delete an item
    async fn delete_item(&self, id: &str) -> Result<bool, AppError> {
        let _ = id;
        Err(AppError::NotSupported(
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
    ) -> Result<(), AppError>;

    /// Get items pending blockchain submission
    async fn get_pending_blockchain_items(&self, limit: i64) -> Result<Vec<Item>, AppError>;

    /// Increment retry count for an item
    async fn increment_retry_count(&self, id: &str) -> Result<i32, AppError>;
}

/// Blockchain client trait for chain operations
#[async_trait]
pub trait BlockchainClient: Send + Sync {
    /// Check blockchain RPC connectivity
    async fn health_check(&self) -> Result<(), AppError>;

    /// Submit a transaction with the given hash/memo
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError>;

    /// Get transaction confirmation status
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        let _ = signature;
        Err(AppError::NotSupported(
            "get_transaction_status not implemented".to_string(),
        ))
    }

    /// Get current block height
    async fn get_block_height(&self) -> Result<u64, AppError> {
        Err(AppError::NotSupported(
            "get_block_height not implemented".to_string(),
        ))
    }

    /// Get latest blockhash for transaction construction
    async fn get_latest_blockhash(&self) -> Result<String, AppError> {
        Err(AppError::NotSupported(
            "get_latest_blockhash not implemented".to_string(),
        ))
    }

    /// Wait for transaction confirmation with timeout
    async fn wait_for_confirmation(
        &self,
        signature: &str,
        timeout_secs: u64,
    ) -> Result<bool, AppError> {
        let _ = (signature, timeout_secs);
        Err(AppError::NotSupported(
            "wait_for_confirmation not implemented".to_string(),
        ))
    }
}
