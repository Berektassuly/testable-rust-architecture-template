use crate::domain::{AppError, CreateItemRequest, Item};

use super::state::AppState;

/// Application service containing core business logic.
///
/// This service orchestrates operations between the database and blockchain
/// clients, implementing the application's use cases. It depends only on
/// the trait abstractions defined in the domain layer, making it easily
/// testable with mock implementations.
pub struct AppService;

impl AppService {
    /// Creates a new `AppService` instance.
    pub fn new() -> Self {
        Self
    }

    /// Creates a new item in the database and submits its hash to the blockchain.
    ///
    /// This method orchestrates the following workflow:
    /// 1. Persists the item data to the database
    /// 2. Generates a hash from the created item's ID
    /// 3. Submits the hash to the blockchain for immutable recording
    /// 4. Returns the created item on success
    ///
    /// # Arguments
    ///
    /// * `state` - The shared application state containing client references.
    /// * `item_data` - The request data for creating a new item.
    ///
    /// # Returns
    ///
    /// Returns the created `Item` if both database and blockchain operations succeed.
    ///
    /// # Errors
    ///
    /// Returns an `AppError` if:
    /// - The database operation fails
    /// - The blockchain submission fails
    pub async fn create_and_submit_item(
        &self,
        state: &AppState,
        item_data: &CreateItemRequest,
    ) -> Result<Item, AppError> {
        // Step 1: Create the item in the database
        let item = state.db_client.create_item(item_data).await?;

        // Step 2: Generate a placeholder hash from the item's ID
        let hash = format!("hash_{}", item.id);

        // Step 3: Submit the hash to the blockchain
        let _signature = state.blockchain_client.submit_transaction(&hash).await?;

        // Step 4: Return the created item
        Ok(item)
    }
}

impl Default for AppService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BlockchainClient, DatabaseClient};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use chrono::Utc;

    /// Mock database client for testing.
    struct MockDatabaseClient {
        storage: Mutex<HashMap<String, Item>>,
        should_fail: bool,
    }

    impl MockDatabaseClient {
        fn new() -> Self {
            Self {
                storage: Mutex::new(HashMap::new()),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                storage: Mutex::new(HashMap::new()),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl DatabaseClient for MockDatabaseClient {
        async fn health_check(&self) -> Result<(), AppError> {
            if self.should_fail {
                Err(AppError::Database("Mock database unhealthy".to_string()))
            } else {
                Ok(())
            }
        }

        async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
            if self.should_fail {
                return Err(AppError::Database("Mock database error".to_string()));
            }
            let storage = self.storage.lock().unwrap();
            Ok(storage.get(id).cloned())
        }

        async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError> {
            if self.should_fail {
                return Err(AppError::Database("Mock database error".to_string()));
            }

            let id = format!("item_{}", uuid::Uuid::new_v4());
            let now = Utc::now();
            let item = Item {
                id: id.clone(),
                hash: format!("hash_{}", id),
                name: data.name.clone(),
                description: data.description.clone(),
                metadata: data.metadata.clone(),
                created_at: now,
                updated_at: now,
            };

            let mut storage = self.storage.lock().unwrap();
            storage.insert(id, item.clone());

            Ok(item)
        }
    }

    /// Mock blockchain client for testing.
    struct MockBlockchainClient {
        transactions: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockBlockchainClient {
        fn new() -> Self {
            Self {
                transactions: Mutex::new(Vec::new()),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                transactions: Mutex::new(Vec::new()),
                should_fail: true,
            }
        }

        fn get_transactions(&self) -> Vec<String> {
            self.transactions.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl BlockchainClient for MockBlockchainClient {
        async fn health_check(&self) -> Result<(), AppError> {
            if self.should_fail {
                Err(AppError::Blockchain("Mock blockchain unhealthy".to_string()))
            } else {
                Ok(())
            }
        }

        async fn submit_transaction(&self, hash: &str) -> Result<String, AppError> {
            if self.should_fail {
                return Err(AppError::TransactionFailed("Mock blockchain error".to_string()));
            }

            let signature = format!("sig_{}", hash);
            let mut transactions = self.transactions.lock().unwrap();
            transactions.push(hash.to_string());

            Ok(signature)
        }
    }

    #[tokio::test]
    async fn test_create_and_submit_item_success() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let state = AppState::new(db_client.clone(), blockchain_client.clone());
        let service = AppService::new();

        let request = CreateItemRequest::new(
            "Test Item".to_string(),
            "Test content".to_string(),
        );

        let result = service.create_and_submit_item(&state, &request).await;

        assert!(result.is_ok());
        let item = result.unwrap();
        assert_eq!(item.name, "Test Item");

        // Verify blockchain was called
        let transactions = blockchain_client.get_transactions();
        assert_eq!(transactions.len(), 1);
        assert!(transactions[0].contains(&item.id));
    }

    #[tokio::test]
    async fn test_create_and_submit_item_database_failure() {
        let db_client = Arc::new(MockDatabaseClient::with_failure());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let state = AppState::new(db_client, blockchain_client.clone());
        let service = AppService::new();

        let request = CreateItemRequest::new(
            "Test Item".to_string(),
            "Test content".to_string(),
        );

        let result = service.create_and_submit_item(&state, &request).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Database(_)));

        // Verify blockchain was NOT called
        let transactions = blockchain_client.get_transactions();
        assert!(transactions.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_submit_item_blockchain_failure() {
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::with_failure());

        let state = AppState::new(db_client, blockchain_client);
        let service = AppService::new();

        let request = CreateItemRequest::new(
            "Test Item".to_string(),
            "Test content".to_string(),
        );

        let result = service.create_and_submit_item(&state, &request).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::TransactionFailed(_)));
    }

    #[tokio::test]
    async fn test_app_service_default() {
        let service = AppService::default();
        let db_client = Arc::new(MockDatabaseClient::new());
        let blockchain_client = Arc::new(MockBlockchainClient::new());

        let state = AppState::new(db_client, blockchain_client);

        let request = CreateItemRequest::new(
            "Default Test".to_string(),
            "Content".to_string(),
        );

        let result = service.create_and_submit_item(&state, &request).await;
        assert!(result.is_ok());
    }
}