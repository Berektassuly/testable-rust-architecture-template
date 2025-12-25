//! Application service layer.
//!
//! This module contains the core business logic that orchestrates
//! operations between infrastructure components using trait abstractions.

use std::sync::Arc;
use tracing::{info, instrument, warn};
use validator::Validate;

use crate::domain::{
    AppError, BlockchainClient, CreateItemRequest, DatabaseClient, HealthResponse, HealthStatus,
    Item, ValidationError,
};

/// Application service containing core business logic.
///
/// This service orchestrates operations between the database and blockchain
/// clients, implementing the application's use cases. It holds references
/// to the trait abstractions, enabling dependency injection and testability.
///
/// # Example
///
/// ```ignore
/// let db = Arc::new(PostgresClient::new(&config)?);
/// let blockchain = Arc::new(SolanaClient::new(&url)?);
/// let service = AppService::new(db, blockchain);
///
/// let item = service.create_and_submit_item(&request).await?;
/// ```
pub struct AppService {
    db_client: Arc<dyn DatabaseClient>,
    blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppService {
    /// Creates a new `AppService` instance.
    ///
    /// # Arguments
    ///
    /// * `db_client` - Database client for persistence operations.
    /// * `blockchain_client` - Blockchain client for on-chain operations.
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

    /// Creates a new item and submits its hash to the blockchain.
    ///
    /// This method orchestrates the following workflow:
    /// 1. Validates the input data
    /// 2. Persists the item to the database
    /// 3. Generates a hash from the created item
    /// 4. Submits the hash to the blockchain
    /// 5. Returns the created item on success
    ///
    /// # Arguments
    ///
    /// * `request` - The request data for creating a new item.
    ///
    /// # Returns
    ///
    /// Returns the created `Item` if both database and blockchain operations succeed.
    ///
    /// # Errors
    ///
    /// Returns an `AppError` if:
    /// - Validation fails
    /// - The database operation fails
    /// - The blockchain submission fails
    #[instrument(skip(self, request), fields(item_name = %request.name))]
    pub async fn create_and_submit_item(
        &self,
        request: &CreateItemRequest,
    ) -> Result<Item, AppError> {
        // Step 1: Validate input
        request.validate().map_err(|e| {
            warn!(error = %e, "Validation failed for create item request");
            AppError::Validation(ValidationError::Multiple(e.to_string()))
        })?;

        info!("Creating new item: {}", request.name);

        // Step 2: Create the item in the database
        let item = self.db_client.create_item(request).await?;
        info!(item_id = %item.id, "Item created in database");

        // Step 3: Generate hash from the item
        let hash = self.generate_hash(&item);

        // Step 4: Submit to blockchain
        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(
                    item_id = %item.id,
                    signature = %signature,
                    "Item hash submitted to blockchain"
                );
            }
            Err(e) => {
                warn!(
                    item_id = %item.id,
                    error = ?e,
                    "Failed to submit to blockchain, but item was created"
                );
                // Note: In a real application, you might want to:
                // - Retry the blockchain submission
                // - Mark the item as pending blockchain confirmation
                // - Use a saga pattern for distributed transactions
                return Err(e);
            }
        }

        Ok(item)
    }

    /// Gets an item by ID.
    #[instrument(skip(self))]
    pub async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        info!(item_id = %id, "Fetching item");
        self.db_client.get_item(id).await
    }

    /// Performs a health check on all dependencies.
    ///
    /// Returns the health status of the database and blockchain clients.
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> HealthResponse {
        let db_health = match self.db_client.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(e) => {
                warn!(error = ?e, "Database health check failed");
                HealthStatus::Unhealthy
            }
        };

        let blockchain_health = match self.blockchain_client.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(e) => {
                warn!(error = ?e, "Blockchain health check failed");
                HealthStatus::Unhealthy
            }
        };

        HealthResponse::new(db_health, blockchain_health)
    }

    /// Generates a hash for an item.
    ///
    /// In a real implementation, this would use a proper hashing algorithm
    /// (e.g., SHA256) to create a content-addressable identifier.
    fn generate_hash(&self, item: &Item) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(item.id.as_bytes());
        hasher.update(item.name.as_bytes());
        if let Some(ref desc) = item.description {
            hasher.update(desc.as_bytes());
        }

        let result = hasher.finalize();
        hex::encode(result)
    }
}

// Add hex encoding helper since we're using sha2
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            hex.push(HEX_CHARS[(byte >> 4) as usize] as char);
            hex.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
        }
        hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    #[tokio::test]
    async fn test_create_and_submit_item_success() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client.clone(), blockchain_client.clone());

        let request = CreateItemRequest::new("Test Item".to_string(), "Test content".to_string());

        let result = service.create_and_submit_item(&request).await;

        assert!(result.is_ok());
        let item = result.unwrap();
        assert_eq!(item.name, "Test Item");

        // Verify blockchain was called
        let transactions = blockchain_client.get_transactions();
        assert_eq!(transactions.len(), 1);
    }

    #[tokio::test]
    async fn test_create_and_submit_item_validation_failure() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client, blockchain_client.clone());

        // Empty name should fail validation
        let request = CreateItemRequest::new("".to_string(), "Test content".to_string());

        let result = service.create_and_submit_item(&request).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Validation(_)));

        // Verify blockchain was NOT called
        let transactions = blockchain_client.get_transactions();
        assert!(transactions.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_submit_item_database_failure() {
        let db_client = Arc::new(MockDatabaseClient::failing("Database error"));
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client, blockchain_client.clone());

        let request = CreateItemRequest::new("Test Item".to_string(), "Test content".to_string());

        let result = service.create_and_submit_item(&request).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Database(_)));

        // Verify blockchain was NOT called
        let transactions = blockchain_client.get_transactions();
        assert!(transactions.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_submit_item_blockchain_failure() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::failing("RPC error"));

        let service = AppService::new(db_client, blockchain_client);

        let request = CreateItemRequest::new("Test Item".to_string(), "Test content".to_string());

        let result = service.create_and_submit_item(&request).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Blockchain(_)));
    }

    #[tokio::test]
    async fn test_get_item() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client.clone(), blockchain_client);

        // First create an item
        let request = CreateItemRequest::new("Test".to_string(), "Content".to_string());
        let created = service.create_and_submit_item(&request).await.unwrap();

        // Then fetch it
        let fetched = service.get_item(&created.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, created.id);

        // Non-existent item
        let not_found = service.get_item("non-existent").await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_health_check_all_healthy() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client, blockchain_client);

        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Healthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_health_check_db_unhealthy() {
        let db_client = Arc::new(MockDatabaseClient::new());
        db_client.set_healthy(false);
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client, blockchain_client);

        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert_eq!(health.database, HealthStatus::Unhealthy);
        assert_eq!(health.blockchain, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_health_check_blockchain_unhealthy() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());
        blockchain_client.set_healthy(false);

        let service = AppService::new(db_client, blockchain_client);

        let health = service.health_check().await;

        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert_eq!(health.database, HealthStatus::Healthy);
        assert_eq!(health.blockchain, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_generate_hash() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let service = AppService::new(db_client, blockchain_client);

        let item = Item::new(
            "test-id".to_string(),
            "old-hash".to_string(),
            "Test Name".to_string(),
        );

        let hash = service.generate_hash(&item);

        // Should be a 64-character hex string (SHA256)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
