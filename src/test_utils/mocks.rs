//! Mock implementations for testing.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::{
    BlockchainClient, BlockchainError, BlockchainStatus, CreateItemRequest, HealthCheckError, Item,
    ItemError, ItemMetadata, ItemRepository, OutboxRepository, OutboxStatus, PaginatedResponse,
    SolanaOutboxEntry, SolanaOutboxPayload, build_solana_outbox_payload_from_request,
};

/// Configuration for mock behavior
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

/// Mock provider implementing both ItemRepository and OutboxRepository with shared state.
/// Creating an item via ItemRepository populates the outbox accessed via OutboxRepository.
pub struct MockProvider {
    storage: Arc<Mutex<HashMap<String, Item>>>,
    outbox: Arc<Mutex<HashMap<String, SolanaOutboxEntry>>>,
    config: MockConfig,
    is_healthy: AtomicBool,
}

impl MockProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::success())
    }

    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            outbox: Arc::new(Mutex::new(HashMap::new())),
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

    /// Get all stored items (for testing)
    pub fn get_all_items(&self) -> Vec<Item> {
        self.storage.lock().unwrap().values().cloned().collect()
    }

    fn check_should_fail(&self) -> Result<(), ItemError> {
        if self.config.should_fail {
            return Err(ItemError::RepositoryFailure);
        }
        Ok(())
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to use a single `MockProvider` as both repositories (shared state for tests).
#[must_use]
pub fn mock_repos(
    mock: &Arc<MockProvider>,
) -> (Arc<dyn ItemRepository>, Arc<dyn OutboxRepository>) {
    (
        Arc::clone(mock) as Arc<dyn ItemRepository>,
        Arc::clone(mock) as Arc<dyn OutboxRepository>,
    )
}

#[async_trait]
impl ItemRepository for MockProvider {
    async fn health_check(&self) -> Result<(), HealthCheckError> {
        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(HealthCheckError::DatabaseUnavailable);
        }
        self.check_should_fail()
            .map_err(|_| HealthCheckError::DatabaseUnavailable)
    }

    async fn get_item(&self, id: &str) -> Result<Option<Item>, ItemError> {
        self.check_should_fail()?;
        let storage = self.storage.lock().unwrap();
        Ok(storage.get(id).cloned())
    }

    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, ItemError> {
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
            content: data.content.clone(),
            metadata,
            blockchain_status: BlockchainStatus::PendingSubmission,
            blockchain_signature: None,
            blockchain_retry_count: 0,
            blockchain_last_error: None,
            blockchain_next_retry_at: None,
            created_at: now,
            updated_at: now,
        };
        let outbox_entry = SolanaOutboxEntry {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: id.clone(),
            payload: build_solana_outbox_payload_from_request(&id, data),
            status: OutboxStatus::Pending,
            retry_count: 0,
            created_at: now,
        };
        let mut storage = self.storage.lock().unwrap();
        storage.insert(id, item.clone());
        let mut outbox = self.outbox.lock().unwrap();
        outbox.insert(outbox_entry.id.clone(), outbox_entry);
        Ok(item)
    }

    async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, ItemError> {
        self.check_should_fail()?;
        let storage = self.storage.lock().unwrap();
        let mut items: Vec<Item> = storage.values().cloned().collect();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply cursor
        let items = if let Some(cursor_id) = cursor {
            let pos = items.iter().position(|i| i.id == cursor_id);
            match pos {
                Some(p) => items.into_iter().skip(p + 1).collect(),
                None => {
                    return Err(ItemError::InvalidState("Invalid cursor".to_string()));
                }
            }
        } else {
            items
        };

        let limit = limit.clamp(1, 100) as usize;
        let has_more = items.len() > limit;
        let items: Vec<Item> = items.into_iter().take(limit).collect();
        let next_cursor = if has_more {
            items.last().map(|i| i.id.clone())
        } else {
            None
        };

        Ok(PaginatedResponse::new(items, next_cursor, has_more))
    }

    async fn update_blockchain_status(
        &self,
        id: &str,
        status: BlockchainStatus,
        signature: Option<&str>,
        error: Option<&str>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ItemError> {
        self.check_should_fail()?;
        let mut storage = self.storage.lock().unwrap();
        if let Some(item) = storage.get_mut(id) {
            item.blockchain_status = status;
            if let Some(sig) = signature {
                item.blockchain_signature = Some(sig.to_string());
            }
            item.blockchain_last_error = error.map(|e| e.to_string());
            item.blockchain_next_retry_at = next_retry_at;
            item.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn enqueue_solana_outbox_for_item(
        &self,
        item_id: &str,
        payload: &SolanaOutboxPayload,
    ) -> Result<Item, ItemError> {
        self.check_should_fail()?;
        let now = Utc::now();
        let mut storage = self.storage.lock().unwrap();
        let item = storage
            .get_mut(item_id)
            .ok_or_else(|| ItemError::NotFound(item_id.to_string()))?;

        let outbox_entry = SolanaOutboxEntry {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: item_id.to_string(),
            payload: payload.clone(),
            status: OutboxStatus::Pending,
            retry_count: 0,
            created_at: now,
        };
        let mut outbox = self.outbox.lock().unwrap();
        outbox.insert(outbox_entry.id.clone(), outbox_entry);

        item.blockchain_status = BlockchainStatus::PendingSubmission;
        item.blockchain_last_error = None;
        item.blockchain_next_retry_at = None;
        item.blockchain_retry_count = 0;
        item.updated_at = now;

        Ok(item.clone())
    }

    async fn get_pending_blockchain_items(&self, limit: i64) -> Result<Vec<Item>, ItemError> {
        self.check_should_fail()?;
        let storage = self.storage.lock().unwrap();
        let now = Utc::now();
        let mut items: Vec<Item> = storage
            .values()
            .filter(|i| {
                i.blockchain_status == BlockchainStatus::PendingSubmission
                    && i.blockchain_retry_count < 10
                    && i.blockchain_next_retry_at.map(|t| t <= now).unwrap_or(true)
            })
            .cloned()
            .collect();
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(items.into_iter().take(limit as usize).collect())
    }

    async fn increment_retry_count(&self, id: &str) -> Result<i32, ItemError> {
        self.check_should_fail()?;
        let mut storage = self.storage.lock().unwrap();
        if let Some(item) = storage.get_mut(id) {
            item.blockchain_retry_count += 1;
            item.updated_at = Utc::now();
            Ok(item.blockchain_retry_count)
        } else {
            Err(ItemError::NotFound(id.to_string()))
        }
    }
}

#[async_trait]
impl OutboxRepository for MockProvider {
    async fn health_check(&self) -> Result<(), HealthCheckError> {
        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(HealthCheckError::DatabaseUnavailable);
        }
        self.check_should_fail()
            .map_err(|_| HealthCheckError::DatabaseUnavailable)
    }

    async fn claim_pending_solana_outbox(
        &self,
        limit: i64,
    ) -> Result<Vec<SolanaOutboxEntry>, ItemError> {
        self.check_should_fail()?;
        let now = Utc::now();
        let storage = self.storage.lock().unwrap();
        let mut outbox = self.outbox.lock().unwrap();
        let mut entries: Vec<SolanaOutboxEntry> = outbox
            .values()
            .filter(|e| e.status == OutboxStatus::Pending)
            .filter(|e| {
                storage
                    .get(&e.aggregate_id)
                    .map(|i| i.blockchain_next_retry_at.map(|t| t <= now).unwrap_or(true))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        let mut selected: Vec<SolanaOutboxEntry> =
            entries.into_iter().take(limit as usize).collect();

        for entry in &mut selected {
            entry.status = OutboxStatus::Processing;
            if let Some(stored) = outbox.get_mut(&entry.id) {
                stored.status = OutboxStatus::Processing;
            }
        }

        Ok(selected)
    }

    async fn complete_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        signature: &str,
    ) -> Result<(), ItemError> {
        self.check_should_fail()?;
        let mut storage = self.storage.lock().unwrap();
        if let Some(item) = storage.get_mut(item_id) {
            item.blockchain_status = BlockchainStatus::Submitted;
            item.blockchain_signature = Some(signature.to_string());
            item.blockchain_last_error = None;
            item.blockchain_next_retry_at = None;
            item.updated_at = Utc::now();
        }
        drop(storage);

        let mut outbox = self.outbox.lock().unwrap();
        if let Some(entry) = outbox.get_mut(outbox_id) {
            entry.status = OutboxStatus::Completed;
        }
        Ok(())
    }

    async fn fail_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        retry_count: i32,
        outbox_status: OutboxStatus,
        item_status: BlockchainStatus,
        error: &str,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ItemError> {
        self.check_should_fail()?;
        let mut storage = self.storage.lock().unwrap();
        if let Some(item) = storage.get_mut(item_id) {
            item.blockchain_status = item_status;
            item.blockchain_last_error = Some(error.to_string());
            item.blockchain_next_retry_at = next_retry_at;
            item.blockchain_retry_count = retry_count;
            item.updated_at = Utc::now();
        }
        drop(storage);

        let mut outbox = self.outbox.lock().unwrap();
        if let Some(entry) = outbox.get_mut(outbox_id) {
            entry.status = outbox_status;
            entry.retry_count = retry_count;
        }
        Ok(())
    }
}

/// Mock blockchain client for testing
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

    fn check_should_fail(&self) -> Result<(), BlockchainError> {
        if self.config.should_fail {
            let msg = self
                .config
                .error_message
                .clone()
                .unwrap_or_else(|| "Mock error".to_string());
            return Err(BlockchainError::SubmissionFailed(msg));
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
    async fn health_check(&self) -> Result<(), HealthCheckError> {
        if !self.is_healthy.load(Ordering::Relaxed) {
            return Err(HealthCheckError::BlockchainUnavailable);
        }
        self.check_should_fail()
            .map_err(|_| HealthCheckError::BlockchainUnavailable)
    }

    async fn submit_transaction(&self, hash: &str) -> Result<String, BlockchainError> {
        self.check_should_fail()?;
        let signature = format!("sig_{}", hash);
        let mut transactions = self.transactions.lock().unwrap();
        transactions.push(hash.to_string());
        Ok(signature)
    }

    async fn get_transaction_status(&self, signature: &str) -> Result<bool, BlockchainError> {
        self.check_should_fail()?;
        let transactions = self.transactions.lock().unwrap();
        Ok(transactions.iter().any(|t| signature.contains(t)))
    }

    async fn get_block_height(&self) -> Result<u64, BlockchainError> {
        self.check_should_fail()?;
        Ok(12345678)
    }

    async fn get_latest_blockhash(&self) -> Result<String, BlockchainError> {
        self.check_should_fail()?;
        Ok("mock_blockhash_abc123".to_string())
    }

    async fn wait_for_confirmation(
        &self,
        signature: &str,
        _timeout_secs: u64,
    ) -> Result<bool, BlockchainError> {
        self.check_should_fail()?;
        let transactions = self.transactions.lock().unwrap();
        Ok(transactions.iter().any(|t| signature.contains(t)))
    }
}
