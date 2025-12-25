//! Mock implementations for testing.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::{
    AppError, BlockchainClient, BlockchainError, CreateItemRequest, DatabaseClient, DatabaseError,
    Item, ItemMetadata,
};

#[derive(Debug, Clone, Default)]
pub struct MockConfig {
    pub should_fail: bool,
    pub error_message: Option<String>,
}

impl MockConfig {
    #[must_use]
    pub fn success() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            should_fail: true,
            error_message: Some(message.into()),
        }
    }
}

pub struct MockDatabaseClient {
    storage: Arc<Mutex<HashMap<String, Item>>>,
    config: MockConfig,
    is_healthy: AtomicBool,
}

impl MockDatabaseClient {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::success())
    }

    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            config,
            is_healthy: AtomicBool::new(true),
        }
    }

    #[must_use]
    pub fn failing(message: impl Into<String>) -> Self {
        Self::with_config(MockConfig::failure(message))
    }

    pub fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::Relaxed);
    }

    fn check_should_fail(&self) -> Result<(), AppError> {
        if self.config.should_fail {
            let msg = self
                .config
                .error_message
                .clone()
                .unwrap_or_else(|| "Mock error".to_string());
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
        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(AppError::Database(DatabaseError::Connection(
                "Unhealthy".to_string(),
            )));
        }
        self.check_should_fail()
    }

    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        self.check_should_fail()?;
        let storage = self.storage.lock().unwrap();
        Ok(storage.get(id).cloned())
    }

    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError> {
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
}

pub struct MockBlockchainClient {
    transactions: Arc<Mutex<Vec<String>>>,
    config: MockConfig,
    is_healthy: AtomicBool,
}

impl MockBlockchainClient {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::success())
    }

    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        Self {
            transactions: Arc::new(Mutex::new(Vec::new())),
            config,
            is_healthy: AtomicBool::new(true),
        }
    }

    #[must_use]
    pub fn failing(message: impl Into<String>) -> Self {
        Self::with_config(MockConfig::failure(message))
    }

    pub fn set_healthy(&self, healthy: bool) {
        self.is_healthy.store(healthy, Ordering::Relaxed);
    }

    pub fn get_transactions(&self) -> Vec<String> {
        self.transactions.lock().unwrap().clone()
    }

    fn check_should_fail(&self) -> Result<(), AppError> {
        if self.config.should_fail {
            let msg = self
                .config
                .error_message
                .clone()
                .unwrap_or_else(|| "Mock error".to_string());
            return Err(AppError::Blockchain(BlockchainError::TransactionFailed(
                msg,
            )));
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
        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(AppError::Blockchain(BlockchainError::Connection(
                "Unhealthy".to_string(),
            )));
        }
        self.check_should_fail()
    }

    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError> {
        self.check_should_fail()?;
        let signature = format!("sig_{}", hash);
        let mut transactions = self.transactions.lock().unwrap();
        transactions.push(hash.to_string());
        Ok(signature)
    }
}
