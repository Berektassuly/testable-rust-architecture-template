use async_trait::async_trait;

use super::error::AppError;
use super::types::{CreateItemRequest, Item};

/// A trait defining the contract for database operations.
///
/// This trait abstracts database interactions, allowing for different
/// implementations (e.g., PostgreSQL, in-memory mock) to be swapped
/// at runtime without changing the business logic.
#[async_trait]
pub trait DatabaseClient: Send + Sync {
    /// Checks the health of the database connection.
    async fn health_check(&self) -> Result<(), AppError>;

    /// Retrieves an item by its unique ID.
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError>;

    /// Creates a new item in the database.
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError>;
}

/// A trait defining the contract for blockchain operations.
///
/// This trait abstracts blockchain interactions, allowing for different
/// implementations (e.g., Solana, Ethereum, in-memory mock) to be swapped
/// at runtime without changing the business logic.
#[async_trait]
pub trait BlockchainClient: Send + Sync {
    /// Checks the health of the blockchain client connection.
    async fn health_check(&self) -> Result<(), AppError>;

    /// Submits a transaction hash and returns a signature.
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError>;
}