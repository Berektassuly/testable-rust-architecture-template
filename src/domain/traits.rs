//! Domain traits defining contracts for external systems.
//!
//! These traits abstract infrastructure concerns, enabling
//! dependency injection and testability.

use async_trait::async_trait;

use super::error::AppError;
use super::types::{CreateItemRequest, Item};

/// Contract for database operations.
///
/// Implementations must be thread-safe (`Send + Sync`) as they
/// will be shared across async tasks.
///
/// # Example Implementation
///
/// ```ignore
/// use async_trait::async_trait;
///
/// struct PostgresClient { pool: PgPool }
///
/// #[async_trait]
/// impl DatabaseClient for PostgresClient {
///     async fn health_check(&self) -> Result<(), AppError> {
///         sqlx::query("SELECT 1")
///             .execute(&self.pool)
///             .await
///             .map(|_| ())
///             .map_err(|e| e.into())
///     }
///     // ...
/// }
/// ```
#[async_trait]
pub trait DatabaseClient: Send + Sync {
    /// Checks the health of the database connection.
    ///
    /// Returns `Ok(())` if the database is reachable and responding.
    async fn health_check(&self) -> Result<(), AppError>;

    /// Retrieves an item by its unique ID.
    ///
    /// Returns `Ok(None)` if the item doesn't exist.
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError>;

    /// Creates a new item in the database.
    ///
    /// Returns the created item with its generated ID and timestamps.
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError>;

    /// Updates an existing item.
    ///
    /// Returns the updated item or an error if it doesn't exist.
    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, AppError> {
        // Default implementation - can be overridden
        let _ = (id, data);
        Err(AppError::NotSupported(
            "update_item not implemented".to_string(),
        ))
    }

    /// Deletes an item by ID.
    ///
    /// Returns `true` if the item was deleted, `false` if it didn't exist.
    async fn delete_item(&self, id: &str) -> Result<bool, AppError> {
        let _ = id;
        Err(AppError::NotSupported(
            "delete_item not implemented".to_string(),
        ))
    }
}

/// Contract for blockchain operations.
///
/// Implementations must be thread-safe (`Send + Sync`) as they
/// will be shared across async tasks.
///
/// # Example Implementation
///
/// ```ignore
/// use async_trait::async_trait;
///
/// struct SolanaClient { rpc_url: String }
///
/// #[async_trait]
/// impl BlockchainClient for SolanaClient {
///     async fn health_check(&self) -> Result<(), AppError> {
///         // Check RPC node health
///         Ok(())
///     }
///     // ...
/// }
/// ```
#[async_trait]
pub trait BlockchainClient: Send + Sync {
    /// Checks the health of the blockchain client connection.
    ///
    /// Returns `Ok(())` if the blockchain node is reachable.
    async fn health_check(&self) -> Result<(), AppError>;

    /// Submits a transaction with the given hash/data.
    ///
    /// Returns the transaction signature on success.
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError>;

    /// Gets the status of a transaction.
    ///
    /// Returns `Ok(true)` if confirmed, `Ok(false)` if pending/not found.
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        let _ = signature;
        Err(AppError::NotSupported(
            "get_transaction_status not implemented".to_string(),
        ))
    }

    /// Gets the current block height.
    async fn get_block_height(&self) -> Result<u64, AppError> {
        Err(AppError::NotSupported(
            "get_block_height not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Verify traits are object-safe
    #[test]
    fn test_database_client_is_object_safe() {
        fn _assert_object_safe(_: &dyn DatabaseClient) {}
    }

    #[test]
    fn test_blockchain_client_is_object_safe() {
        fn _assert_object_safe(_: &dyn BlockchainClient) {}
    }

    // Verify traits can be wrapped in Arc
    #[test]
    fn test_traits_are_arc_compatible() {
        struct MockDb;

        #[async_trait]
        impl DatabaseClient for MockDb {
            async fn health_check(&self) -> Result<(), AppError> {
                Ok(())
            }
            async fn get_item(&self, _id: &str) -> Result<Option<Item>, AppError> {
                Ok(None)
            }
            async fn create_item(&self, _data: &CreateItemRequest) -> Result<Item, AppError> {
                Err(AppError::NotSupported("mock".to_string()))
            }
        }

        let _: Arc<dyn DatabaseClient> = Arc::new(MockDb);
    }
}
