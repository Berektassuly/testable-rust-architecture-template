use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a unique identifier for domain entities.
pub type EntityId = String;

/// Represents a hash string used for blockchain records.
pub type HashString = String;

/// Represents a transaction ID from the blockchain.
pub type TransactionId = String;

/// Core domain entity representing an item stored in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Item {
    pub id: EntityId,
    pub hash: HashString,
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<ItemMetadata>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Item {
    pub fn new(id: EntityId, hash: HashString, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            hash,
            name,
            description: None,
            metadata: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_metadata(mut self, metadata: ItemMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Additional metadata that can be attached to an item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemMetadata {
    pub author: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub custom_fields: std::collections::HashMap<String, String>,
}

impl Default for ItemMetadata {
    fn default() -> Self {
        Self {
            author: None,
            version: None,
            tags: Vec::new(),
            custom_fields: std::collections::HashMap::new(),
        }
    }
}

/// Request payload for creating a new item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateItemRequest {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub metadata: Option<ItemMetadata>,
}

impl CreateItemRequest {
    pub fn new(name: String, content: String) -> Self {
        Self {
            name,
            description: None,
            content,
            metadata: None,
        }
    }
}

/// Request payload for updating an existing item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateItemRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<ItemMetadata>,
}

/// Represents a record written to the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockchainRecord {
    pub tx_id: TransactionId,
    pub hash: HashString,
    pub block_time: Option<DateTime<Utc>>,
    pub block_height: Option<u64>,
    pub raw_data: Option<String>,
}

impl BlockchainRecord {
    pub fn new(tx_id: TransactionId, hash: HashString) -> Self {
        Self {
            tx_id,
            hash,
            block_time: None,
            block_height: None,
            raw_data: None,
        }
    }

    pub fn with_block_time(mut self, block_time: DateTime<Utc>) -> Self {
        self.block_time = Some(block_time);
        self
    }

    pub fn with_block_height(mut self, block_height: u64) -> Self {
        self.block_height = Some(block_height);
        self
    }
}

/// Result of a write operation to the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub success: bool,
    pub record: Option<BlockchainRecord>,
    pub message: String,
}

impl WriteResult {
    pub fn success(record: BlockchainRecord) -> Self {
        Self {
            success: true,
            record: Some(record),
            message: "Record written successfully".to_string(),
        }
    }

    pub fn failure(message: String) -> Self {
        Self {
            success: false,
            record: None,
            message,
        }
    }
}

/// Response payload for item operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemResponse {
    pub item: Item,
    pub blockchain_record: Option<BlockchainRecord>,
}

impl ItemResponse {
    pub fn new(item: Item) -> Self {
        Self {
            item,
            blockchain_record: None,
        }
    }

    pub fn with_blockchain_record(mut self, record: BlockchainRecord) -> Self {
        self.blockchain_record = Some(record);
        self
    }
}

/// Pagination parameters for list queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    pub page: u32,
    pub per_page: u32,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 20,
        }
    }
}

impl PaginationParams {
    pub fn offset(&self) -> u32 {
        (self.page.saturating_sub(1)) * self.per_page
    }
}

/// Paginated response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

impl<T> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, total: u64, params: &PaginationParams) -> Self {
        let total_pages = ((total as f64) / (params.per_page as f64)).ceil() as u32;
        Self {
            items,
            total,
            page: params.page,
            per_page: params.per_page,
            total_pages,
        }
    }
}

/// Health check status for services.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Health check response for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub database: HealthStatus,
    pub blockchain: HealthStatus,
    pub timestamp: DateTime<Utc>,
}

impl HealthResponse {
    pub fn new(database: HealthStatus, blockchain: HealthStatus) -> Self {
        let status = match (&database, &blockchain) {
            (HealthStatus::Healthy, HealthStatus::Healthy) => HealthStatus::Healthy,
            (HealthStatus::Unhealthy, _) | (_, HealthStatus::Unhealthy) => HealthStatus::Unhealthy,
            _ => HealthStatus::Degraded,
        };

        Self {
            status,
            database,
            blockchain,
            timestamp: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_creation() {
        let item = Item::new(
            "item-123".to_string(),
            "abc123hash".to_string(),
            "Test Item".to_string(),
        );

        assert_eq!(item.id, "item-123");
        assert_eq!(item.hash, "abc123hash");
        assert_eq!(item.name, "Test Item");
        assert!(item.description.is_none());
        assert!(item.metadata.is_none());
    }

    #[test]
    fn test_item_builder_pattern() {
        let metadata = ItemMetadata {
            author: Some("Alice".to_string()),
            version: Some("1.0".to_string()),
            tags: vec!["test".to_string()],
            custom_fields: std::collections::HashMap::new(),
        };

        let item = Item::new(
            "item-456".to_string(),
            "def456hash".to_string(),
            "Another Item".to_string(),
        )
        .with_description("A test description".to_string())
        .with_metadata(metadata);

        assert!(item.description.is_some());
        assert!(item.metadata.is_some());
        assert_eq!(item.metadata.unwrap().author, Some("Alice".to_string()));
    }

    #[test]
    fn test_blockchain_record_creation() {
        let record = BlockchainRecord::new(
            "tx-abc123".to_string(),
            "hash-xyz".to_string(),
        )
        .with_block_height(12345);

        assert_eq!(record.tx_id, "tx-abc123");
        assert_eq!(record.hash, "hash-xyz");
        assert_eq!(record.block_height, Some(12345));
        assert!(record.block_time.is_none());
    }

    #[test]
    fn test_write_result_success() {
        let record = BlockchainRecord::new("tx-123".to_string(), "hash-abc".to_string());
        let result = WriteResult::success(record);

        assert!(result.success);
        assert!(result.record.is_some());
    }

    #[test]
    fn test_write_result_failure() {
        let result = WriteResult::failure("Transaction failed".to_string());

        assert!(!result.success);
        assert!(result.record.is_none());
        assert_eq!(result.message, "Transaction failed");
    }

    #[test]
    fn test_pagination_offset() {
        let params = PaginationParams {
            page: 3,
            per_page: 10,
        };

        assert_eq!(params.offset(), 20);
    }

    #[test]
    fn test_pagination_offset_first_page() {
        let params = PaginationParams {
            page: 1,
            per_page: 10,
        };

        assert_eq!(params.offset(), 0);
    }

    #[test]
    fn test_paginated_response() {
        let items = vec!["a", "b", "c"];
        let params = PaginationParams {
            page: 1,
            per_page: 10,
        };
        let response = PaginatedResponse::new(items, 25, &params);

        assert_eq!(response.items.len(), 3);
        assert_eq!(response.total, 25);
        assert_eq!(response.total_pages, 3);
    }

    #[test]
    fn test_health_response_all_healthy() {
        let response = HealthResponse::new(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_eq!(response.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_response_degraded() {
        let response = HealthResponse::new(HealthStatus::Healthy, HealthStatus::Degraded);
        assert_eq!(response.status, HealthStatus::Degraded);
    }

    #[test]
    fn test_health_response_unhealthy() {
        let response = HealthResponse::new(HealthStatus::Unhealthy, HealthStatus::Healthy);
        assert_eq!(response.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_create_item_request() {
        let request = CreateItemRequest::new(
            "New Item".to_string(),
            "Some content here".to_string(),
        );

        assert_eq!(request.name, "New Item");
        assert_eq!(request.content, "Some content here");
        assert!(request.description.is_none());
        assert!(request.metadata.is_none());
    }

    #[test]
    fn test_item_serialization() {
        let item = Item::new(
            "item-789".to_string(),
            "hash789".to_string(),
            "Serializable Item".to_string(),
        );

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: Item = serde_json::from_str(&json).unwrap();

        assert_eq!(item, deserialized);
    }
}