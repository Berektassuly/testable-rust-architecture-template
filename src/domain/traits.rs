//! Domain traits defining contracts for external systems.

use async_trait::async_trait;

use super::error::AppError;
use super::types::{CreateItemRequest, Item};

#[async_trait]
pub trait DatabaseClient: Send + Sync {
    async fn health_check(&self) -> Result<(), AppError>;
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError>;
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError>;
    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, AppError> {
        let _ = (id, data);
        Err(AppError::NotSupported(
            "update_item not implemented".to_string(),
        ))
    }
    async fn delete_item(&self, id: &str) -> Result<bool, AppError> {
        let _ = id;
        Err(AppError::NotSupported(
            "delete_item not implemented".to_string(),
        ))
    }
}

#[async_trait]
pub trait BlockchainClient: Send + Sync {
    async fn health_check(&self) -> Result<(), AppError>;
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError>;
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        let _ = signature;
        Err(AppError::NotSupported(
            "get_transaction_status not implemented".to_string(),
        ))
    }
    async fn get_block_height(&self) -> Result<u64, AppError> {
        Err(AppError::NotSupported(
            "get_block_height not implemented".to_string(),
        ))
    }
}
