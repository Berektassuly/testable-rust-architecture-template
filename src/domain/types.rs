//! Domain types with validation support.
//!
//! All request types include validation rules to ensure
//! data integrity at the API boundary.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

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
    /// Creates a new Item with the given parameters.
    #[must_use]
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

    /// Adds a description to the item.
    #[must_use]
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Adds metadata to the item.
    #[must_use]
    pub fn with_metadata(mut self, metadata: ItemMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Additional metadata that can be attached to an item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ItemMetadata {
    pub author: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub custom_fields: HashMap<String, String>,
}

impl ItemMetadata {
    /// Creates new empty metadata.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the author.
    #[must_use]
    pub fn with_author(mut self, author: String) -> Self {
        self.author = Some(author);
        self
    }

    /// Sets the version.
    #[must_use]
    pub fn with_version(mut self, version: String) -> Self {
        self.version = Some(version);
        self
    }

    /// Adds tags.
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Request payload for creating a new item.
///
/// Includes validation rules for all fields.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateItemRequest {
    /// Name of the item (1-255 characters).
    #[validate(length(min = 1, max = 255, message = "Name must be between 1 and 255 characters"))]
    pub name: String,

    /// Optional description (max 10000 characters).
    #[validate(length(max = 10000, message = "Description must not exceed 10000 characters"))]
    pub description: Option<String>,

    /// Content of the item (max 1MB).
    #[validate(length(min = 1, max = 1048576, message = "Content must be between 1 and 1048576 characters"))]
    pub content: String,

    /// Optional metadata with nested validation.
    #[validate(nested)]
    pub metadata: Option<ItemMetadataRequest>,
}

impl CreateItemRequest {
    /// Creates a new CreateItemRequest with required fields.
    #[must_use]
    pub fn new(name: String, content: String) -> Self {
        Self {
            name,
            description: None,
            content,
            metadata: None,
        }
    }

    /// Adds a description.
    #[must_use]
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Adds metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: ItemMetadataRequest) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Validated metadata request.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ItemMetadataRequest {
    #[validate(length(max = 255, message = "Author must not exceed 255 characters"))]
    pub author: Option<String>,

    #[validate(length(max = 50, message = "Version must not exceed 50 characters"))]
    pub version: Option<String>,

    #[validate(length(max = 20, message = "Maximum 20 tags allowed"))]
    pub tags: Vec<String>,

    pub custom_fields: HashMap<String, String>,
}

impl From<ItemMetadataRequest> for ItemMetadata {
    fn from(req: ItemMetadataRequest) -> Self {
        Self {
            author: req.author,
            version: req.version,
            tags: req.tags,
            custom_fields: req.custom_fields,
        }
    }
}

/// Request payload for updating an existing item.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateItemRequest {
    #[validate(length(min = 1, max = 255, message = "Name must be between 1 and 255 characters"))]
    pub name: Option<String>,

    #[validate(length(max = 10000, message = "Description must not exceed 10000 characters"))]
    pub description: Option<String>,

    #[validate(nested)]
    pub metadata: Option<ItemMetadataRequest>,
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
    /// Creates a new BlockchainRecord.
    #[must_use]
    pub fn new(tx_id: TransactionId, hash: HashString) -> Self {
        Self {
            tx_id,
            hash,
            block_time: None,
            block_height: None,
            raw_data: None,
        }
    }

    /// Sets the block time.
    #[must_use]
    pub fn with_block_time(mut self, block_time: DateTime<Utc>) -> Self {
        self.block_time = Some(block_time);
        self
    }

    /// Sets the block height.
    #[must_use]
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
    /// Creates a successful write result.
    #[must_use]
    pub fn success(record: BlockchainRecord) -> Self {
        Self {
            success: true,
            record: Some(record),
            message: "Record written successfully".to_string(),
        }
    }

    /// Creates a failed write result.
    #[must_use]
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
    /// Creates a new ItemResponse.
    #[must_use]
    pub fn new(item: Item) -> Self {
        Self {
            item,
            blockchain_record: None,
        }
    }

    /// Adds a blockchain record.
    #[must_use]
    pub fn with_blockchain_record(mut self, record: BlockchainRecord) -> Self {
        self.blockchain_record = Some(record);
        self
    }
}

/// Pagination parameters for list queries.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct PaginationParams {
    #[validate(range(min = 1, message = "Page must be at least 1"))]
    pub page: u32,

    #[validate(range(min = 1, max = 100, message = "Per page must be between 1 and 100"))]
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
    /// Calculates the offset for database queries.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.page.saturating_sub(1).saturating_mul(self.per_page)
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
    /// Creates a new paginated response.
    #[must_use]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    pub version: String,
}

impl HealthResponse {
    /// Creates a new health response based on component statuses.
    #[must_use]
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
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use validator::Validate;

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
        let metadata = ItemMetadata::new()
            .with_author("Alice".to_string())
            .with_version("1.0".to_string())
            .with_tags(vec!["test".to_string()]);

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
    fn test_create_item_request_validation_success() {
        let request = CreateItemRequest::new("Valid Name".to_string(), "Valid content".to_string());

        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_create_item_request_validation_empty_name() {
        let request = CreateItemRequest::new("".to_string(), "Valid content".to_string());

        let result = request.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_create_item_request_validation_name_too_long() {
        let long_name = "a".repeat(256);
        let request = CreateItemRequest::new(long_name, "Valid content".to_string());

        let result = request.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_create_item_request_validation_empty_content() {
        let request = CreateItemRequest::new("Valid Name".to_string(), "".to_string());

        let result = request.validate();
        assert!(result.is_err());
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
    fn test_blockchain_record_creation() {
        let record = BlockchainRecord::new("tx-abc123".to_string(), "hash-xyz".to_string())
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
