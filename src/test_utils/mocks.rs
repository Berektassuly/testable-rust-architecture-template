//! Mock implementations for testing.
//!
//! These mocks provide in-memory implementations of domain traits
//! that can be configured to simulate various scenarios including
//! success, failure, and edge cases.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::{
    AppError, BlockchainClient, BlockchainError, CreateItemRequest, DatabaseClient, DatabaseError,
    Item, ItemMetadata,
};

/// Configuration for mock behavior.
#[derive(Debug, Clone, Default)]
pub struct MockConfig {
    /// If true, operations will fail.
    pub should_fail: bool,
    /// Custom error message for failures.
    pub error_message: Option<String>,
    /// Simulated latency in milliseconds.
    pub latency_ms: Option<u64>,
}

impl MockConfig {
    /// Creates a config that always succeeds.
    #[must_use]
    pub fn success() -> Self {
        Self::default()
    }

    /// Creates a config that always fails.
    #[must_use]
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            should_fail: true,
            error_message: Some(message.into()),
            latency_ms: None,
        }
    }

    /// Adds simulated latency.
    #[must_use]
    pub fn with_latency(mut self, ms: u64) -> Self {
        self.latency_ms = Some(ms);
        self
    }
}

/// Mock database client for testing.
///
/// Uses an in-memory HashMap for storage and supports
/// configurable failure modes.
///
/// # Example
///
/// ```
/// use testable_rust_architecture_template::test_utils::{MockDatabaseClient, mocks::MockConfig};
///
/// // Create a mock that succeeds
/// let mock = MockDatabaseClient::new();
///
/// // Create a mock that fails
/// let failing_mock = MockDatabaseClient::with_config(MockConfig::failure("DB error"));
/// ```
pub struct MockDatabaseClient {
    storage: Arc<Mutex<HashMap<String, Item>>>,
    config: MockConfig,
    call_count: AtomicU64,
    is_healthy: AtomicBool,
}

impl MockDatabaseClient {
    /// Creates a new mock with default (success) configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::success())
    }

    /// Creates a new mock with the given configuration.
    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            config,
            call_count: AtomicU64::new(0),
            is_healthy: AtomicBool::new(true),
        }
    }

    /// Creates a mock that always fails.
    #[must_use]
    pub fn failing(message: impl Into<String>) -> Self {
        Self::with_config(MockConfig::failure(message))
    }

    /// Gets the number of times any method was called.
    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Sets the health status.
    pub fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::Relaxed);
    }

    /// Gets all stored items.
    pub fn get_all_items(&self) -> Vec<Item> {
        self.storage.lock().unwrap().values().cloned().collect()
    }

    /// Clears all stored items.
    pub fn clear(&self) {
        self.storage.lock().unwrap().clear();
    }

    fn increment_call_count(&self) {
        self.call_count.fetch_add(1, Ordering::Relaxed);
    }

    fn check_should_fail(&self) -> Result<(), AppError> {
        if self.config.should_fail {
            let msg = self
                .config
                .error_message
                .clone()
                .unwrap_or_else(|| "Mock database error".to_string());
            return Err(AppError::Database(DatabaseError::Query(msg)));
        }
        Ok(())
    }
}

impl Default for MockDatabaseClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabaseClient for MockDatabaseClient {
    async fn health_check(&self) -> Result<(), AppError> {
        self.increment_call_count();

        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(AppError::Database(DatabaseError::Connection(
                "Mock database unhealthy".to_string(),
            )));
        }

        self.check_should_fail()
    }

    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        let storage = self.storage.lock().unwrap();
        Ok(storage.get(id).cloned())
    }

    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        let id = format!("item_{}", uuid::Uuid::new_v4());
        let now = Utc::now();

        let metadata = data.metadata.as_ref().map(|m| ItemMetadata {
            author: m.author.clone(),
            version: m.version.clone(),
            tags: m.tags.clone(),
            custom_fields: m.custom_fields.clone(),
        });

        let item = Item {
            id: id.clone(),
            hash: format!("hash_{}", id),
            name: data.name.clone(),
            description: data.description.clone(),
            metadata,
            created_at: now,
            updated_at: now,
        };

        let mut storage = self.storage.lock().unwrap();
        storage.insert(id, item.clone());

        Ok(item)
    }

    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        let mut storage = self.storage.lock().unwrap();

        if let Some(existing) = storage.get_mut(id) {
            existing.name = data.name.clone();
            existing.description = data.description.clone();
            existing.updated_at = Utc::now();
            Ok(existing.clone())
        } else {
            Err(AppError::Database(DatabaseError::NotFound(format!(
                "Item {} not found",
                id
            ))))
        }
    }

    async fn delete_item(&self, id: &str) -> Result<bool, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        let mut storage = self.storage.lock().unwrap();
        Ok(storage.remove(id).is_some())
    }
}

/// Mock blockchain client for testing.
///
/// Simulates blockchain operations without actual network calls.
///
/// # Example
///
/// ```
/// use testable_rust_architecture_template::test_utils::{MockBlockchainClient, mocks::MockConfig};
///
/// // Create a mock that succeeds
/// let mock = MockBlockchainClient::new();
///
/// // Create a mock that fails
/// let failing_mock = MockBlockchainClient::with_config(MockConfig::failure("RPC error"));
/// ```
pub struct MockBlockchainClient {
    transactions: Arc<Mutex<Vec<String>>>,
    config: MockConfig,
    call_count: AtomicU64,
    is_healthy: AtomicBool,
    block_height: AtomicU64,
}

impl MockBlockchainClient {
    /// Creates a new mock with default (success) configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::success())
    }

    /// Creates a new mock with the given configuration.
    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        Self {
            transactions: Arc::new(Mutex::new(Vec::new())),
            config,
            call_count: AtomicU64::new(0),
            is_healthy: AtomicBool::new(true),
            block_height: AtomicU64::new(1000),
        }
    }

    /// Creates a mock that always fails.
    #[must_use]
    pub fn failing(message: impl Into<String>) -> Self {
        Self::with_config(MockConfig::failure(message))
    }

    /// Gets the number of times any method was called.
    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Sets the health status.
    pub fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::Relaxed);
    }

    /// Gets all submitted transaction hashes.
    pub fn get_transactions(&self) -> Vec<String> {
        self.transactions.lock().unwrap().clone()
    }

    /// Clears all recorded transactions.
    pub fn clear_transactions(&self) {
        self.transactions.lock().unwrap().clear();
    }

    /// Sets the mock block height.
    pub fn set_block_height(&self, height: u64) {
        self.block_height.store(height, Ordering::Relaxed);
    }

    fn increment_call_count(&self) {
        self.call_count.fetch_add(1, Ordering::Relaxed);
    }

    fn check_should_fail(&self) -> Result<(), AppError> {
        if self.config.should_fail {
            let msg = self
                .config
                .error_message
                .clone()
                .unwrap_or_else(|| "Mock blockchain error".to_string());
            return Err(AppError::Blockchain(BlockchainError::TransactionFailed(msg)));
        }
        Ok(())
    }
}

impl Default for MockBlockchainClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BlockchainClient for MockBlockchainClient {
    async fn health_check(&self) -> Result<(), AppError> {
        self.increment_call_count();

        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(AppError::Blockchain(BlockchainError::Connection(
                "Mock blockchain unhealthy".to_string(),
            )));
        }

        self.check_should_fail()
    }

    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        let signature = format!("sig_{}", hash);
        let mut transactions = self.transactions.lock().unwrap();
        transactions.push(hash.to_string());

        Ok(signature)
    }

    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        // Check if we have this transaction recorded
        let transactions = self.transactions.lock().unwrap();
        Ok(transactions.iter().any(|t| signature.contains(t)))
    }

    async fn get_block_height(&self) -> Result<u64, AppError> {
        self.increment_call_count();
        self.check_should_fail()?;

        Ok(self.block_height.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_database_create_and_get() {
        let mock = MockDatabaseClient::new();
        let request = CreateItemRequest::new("Test".to_string(), "Content".to_string());

        let created = mock.create_item(&request).await.unwrap();
        assert_eq!(created.name, "Test");

        let fetched = mock.get_item(&created.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn test_mock_database_failure() {
        let mock = MockDatabaseClient::failing("Connection timeout");
        let request = CreateItemRequest::new("Test".to_string(), "Content".to_string());

        let result = mock.create_item(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_database_call_count() {
        let mock = MockDatabaseClient::new();
        assert_eq!(mock.call_count(), 0);

        let _ = mock.health_check().await;
        assert_eq!(mock.call_count(), 1);

        let _ = mock.get_item("test").await;
        assert_eq!(mock.call_count(), 2);
    }

    #[tokio::test]
    async fn test_mock_blockchain_submit() {
        let mock = MockBlockchainClient::new();

        let sig = mock.submit_transaction("test_hash").await.unwrap();
        assert!(sig.contains("test_hash"));

        let transactions = mock.get_transactions();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0], "test_hash");
    }

    #[tokio::test]
    async fn test_mock_blockchain_failure() {
        let mock = MockBlockchainClient::failing("RPC timeout");

        let result = mock.submit_transaction("hash").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_health_check() {
        let db_mock = MockDatabaseClient::new();
        let bc_mock = MockBlockchainClient::new();

        assert!(db_mock.health_check().await.is_ok());
        assert!(bc_mock.health_check().await.is_ok());

        db_mock.set_healthy(false);
        bc_mock.set_healthy(false);

        assert!(db_mock.health_check().await.is_err());
        assert!(bc_mock.health_check().await.is_err());
    }
}
