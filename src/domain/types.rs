//! Domain types with validation support.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use utoipa::ToSchema;
use validator::Validate;

/// Status of blockchain submission for an item
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockchainStatus {
    /// Initial state, not yet processed
    #[default]
    Pending,
    /// Waiting to be submitted to blockchain
    PendingSubmission,
    /// Transaction submitted, awaiting confirmation
    Submitted,
    /// Transaction confirmed on blockchain
    Confirmed,
    /// Submission failed after max retries
    Failed,
}

impl BlockchainStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::PendingSubmission => "pending_submission",
            Self::Submitted => "submitted",
            Self::Confirmed => "confirmed",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for BlockchainStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "pending_submission" => Ok(Self::PendingSubmission),
            "submitted" => Ok(Self::Submitted),
            "confirmed" => Ok(Self::Confirmed),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Invalid blockchain status: {}", s)),
        }
    }
}

impl std::fmt::Display for BlockchainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Status of a Solana outbox record
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    /// Waiting to be processed
    #[default]
    Pending,
    /// Claimed by a worker
    Processing,
    /// Successfully completed
    Completed,
    /// Failed after retries
    Failed,
}

impl OutboxStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for OutboxStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Invalid outbox status: {}", s)),
        }
    }
}

impl std::fmt::Display for OutboxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Payload stored in the Solana outbox
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SolanaOutboxPayload {
    /// Hash/memo to submit to Solana
    pub hash: String,
}

/// Outbox entry for Solana submissions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SolanaOutboxEntry {
    /// Unique outbox ID (UUID)
    pub id: String,
    /// Item ID (aggregate root)
    pub aggregate_id: String,
    /// Submission payload
    pub payload: SolanaOutboxPayload,
    /// Processing status
    pub status: OutboxStatus,
    /// Retry attempts
    pub retry_count: i32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

/// Core item entity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct Item {
    /// Unique identifier (format: item_<uuid>)
    #[schema(example = "item_abc123")]
    pub id: String,
    /// Content hash
    #[schema(example = "hash_def456")]
    pub hash: String,
    /// Item name
    #[schema(example = "My Item")]
    pub name: String,
    /// Optional description
    #[schema(example = "A detailed description")]
    pub description: Option<String>,
    /// Item content
    #[schema(example = "The actual content here")]
    pub content: String,
    /// Optional metadata
    pub metadata: Option<ItemMetadata>,
    /// Blockchain submission status
    pub blockchain_status: BlockchainStatus,
    /// Blockchain transaction signature (if submitted)
    #[schema(example = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d")]
    pub blockchain_signature: Option<String>,
    /// Number of retry attempts for blockchain submission
    pub blockchain_retry_count: i32,
    /// Last error message from blockchain submission
    pub blockchain_last_error: Option<String>,
    /// Next scheduled retry time
    pub blockchain_next_retry_at: Option<DateTime<Utc>>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl Item {
    #[must_use]
    pub fn new(id: String, hash: String, name: String, content: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            hash,
            name,
            description: None,
            content,
            metadata: None,
            blockchain_status: BlockchainStatus::Pending,
            blockchain_signature: None,
            blockchain_retry_count: 0,
            blockchain_last_error: None,
            blockchain_next_retry_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Compute the deterministic blockchain hash used for submission
#[must_use]
pub fn compute_blockchain_hash(
    item_id: &str,
    name: &str,
    content: &str,
    description: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(item_id.as_bytes());
    hasher.update(name.as_bytes());
    hasher.update(content.as_bytes());
    if let Some(desc) = description {
        hasher.update(desc.as_bytes());
    }
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Build a Solana outbox payload from a create request
#[must_use]
pub fn build_solana_outbox_payload_from_request(
    item_id: &str,
    request: &CreateItemRequest,
) -> SolanaOutboxPayload {
    let hash = compute_blockchain_hash(
        item_id,
        &request.name,
        &request.content,
        request.description.as_deref(),
    );
    SolanaOutboxPayload { hash }
}

/// Build a Solana outbox payload from an existing item
#[must_use]
pub fn build_solana_outbox_payload_from_item(item: &Item) -> SolanaOutboxPayload {
    let hash = compute_blockchain_hash(
        &item.id,
        &item.name,
        &item.content,
        item.description.as_deref(),
    );
    SolanaOutboxPayload { hash }
}

impl Default for Item {
    fn default() -> Self {
        Self::new(
            "default_id".to_string(),
            "default_hash".to_string(),
            "default_name".to_string(),
            "default_content".to_string(),
        )
    }
}

/// Item metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, ToSchema)]
pub struct ItemMetadata {
    /// Author name
    #[schema(example = "John Doe")]
    pub author: Option<String>,
    /// Version string
    #[schema(example = "1.0.0")]
    pub version: Option<String>,
    /// Tags for categorization
    #[schema(example = json!(["rust", "blockchain"]))]
    pub tags: Vec<String>,
    /// Custom key-value fields
    pub custom_fields: HashMap<String, String>,
}

/// Request to create a new item
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct CreateItemRequest {
    /// Item name (1-255 characters)
    #[validate(length(
        min = 1,
        max = 255,
        message = "Name must be between 1 and 255 characters"
    ))]
    #[schema(example = "My New Item")]
    pub name: String,
    /// Optional description (max 10000 characters)
    #[validate(length(max = 10000, message = "Description must not exceed 10000 characters"))]
    #[schema(example = "A detailed description of the item")]
    pub description: Option<String>,
    /// Item content (1-1MB)
    #[validate(length(
        min = 1,
        max = 1048576,
        message = "Content must be between 1 and 1048576 characters"
    ))]
    #[schema(example = "The content of the item")]
    pub content: String,
    /// Optional metadata
    #[validate(nested)]
    pub metadata: Option<ItemMetadataRequest>,
}

impl CreateItemRequest {
    #[must_use]
    pub fn new(name: String, content: String) -> Self {
        Self {
            name,
            description: None,
            content,
            metadata: None,
        }
    }
}

/// Metadata for item creation request
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct ItemMetadataRequest {
    /// Author name (max 255 characters)
    #[validate(length(max = 255))]
    #[schema(example = "John Doe")]
    pub author: Option<String>,
    /// Version string (max 50 characters)
    #[validate(length(max = 50))]
    #[schema(example = "1.0.0")]
    pub version: Option<String>,
    /// Tags (max 20 tags)
    #[validate(length(max = 20))]
    pub tags: Vec<String>,
    /// Custom fields
    pub custom_fields: HashMap<String, String>,
}

/// Pagination parameters for list requests
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct PaginationParams {
    /// Maximum number of items to return (1-100, default: 20)
    #[validate(range(min = 1, max = 100, message = "Limit must be between 1 and 100"))]
    #[serde(default = "default_limit")]
    #[schema(example = 20)]
    pub limit: i64,
    /// Cursor for pagination (item ID to start after)
    #[schema(example = "item_abc123")]
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            cursor: None,
        }
    }
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaginatedResponse<T: ToSchema> {
    /// List of items
    pub items: Vec<T>,
    /// Cursor for next page (null if no more items)
    #[schema(example = "item_xyz789")]
    pub next_cursor: Option<String>,
    /// Whether more items exist
    pub has_more: bool,
}

impl<T: ToSchema> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, next_cursor: Option<String>, has_more: bool) -> Self {
        Self {
            items,
            next_cursor,
            has_more,
        }
    }

    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
        }
    }
}

/// Health status enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// All systems operational
    Healthy,
    /// Some systems degraded but functional
    Degraded,
    /// Critical systems unavailable
    Unhealthy,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Overall system status
    pub status: HealthStatus,
    /// Database health status
    pub database: HealthStatus,
    /// Blockchain client health status
    pub blockchain: HealthStatus,
    /// Current server timestamp
    pub timestamp: DateTime<Utc>,
    /// Application version
    #[schema(example = "0.3.0")]
    pub version: String,
}

impl HealthResponse {
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

/// Error response structure
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    /// Error details
    pub error: ErrorDetail,
}

/// Error detail structure
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorDetail {
    /// Error type identifier
    #[schema(example = "validation_error")]
    pub r#type: String,
    /// Human-readable error message
    #[schema(example = "Name must be between 1 and 255 characters")]
    pub message: String,
}

/// Rate limit exceeded response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RateLimitResponse {
    /// Error details
    pub error: ErrorDetail,
    /// Seconds until rate limit resets
    #[schema(example = 60)]
    pub retry_after: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_blockchain_status_display_and_parsing() {
        let statuses = vec![
            (BlockchainStatus::Pending, "pending"),
            (BlockchainStatus::PendingSubmission, "pending_submission"),
            (BlockchainStatus::Submitted, "submitted"),
            (BlockchainStatus::Confirmed, "confirmed"),
            (BlockchainStatus::Failed, "failed"),
        ];

        for (status, string) in statuses {
            assert_eq!(status.as_str(), string);
            assert_eq!(status.to_string(), string);
            assert_eq!(BlockchainStatus::from_str(string).unwrap(), status);
        }

        assert!(BlockchainStatus::from_str("invalid").is_err());
    }

    #[test]
    fn test_create_item_request_validation() {
        // Valid request
        let req = CreateItemRequest::new("Valid Name".to_string(), "Valid Content".to_string());
        assert!(req.validate().is_ok());

        // Invalid Name (empty)
        let req = CreateItemRequest::new("".to_string(), "Content".to_string());
        assert!(req.validate().is_err());

        // Invalid Name (too long)
        let name = "a".repeat(256);
        let req = CreateItemRequest::new(name, "Content".to_string());
        assert!(req.validate().is_err());

        // Invalid Content (empty)
        let req = CreateItemRequest::new("Name".to_string(), "".to_string());
        assert!(req.validate().is_err());

        // Invalid Content (too long)
        let content = "a".repeat(1_048_577);
        let req = CreateItemRequest::new("Name".to_string(), content);
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_pagination_params_validation() {
        // Valid
        let params = PaginationParams {
            limit: 20,
            cursor: None,
        };
        assert!(params.validate().is_ok());

        // Invalid limit (too small)
        let params = PaginationParams {
            limit: 0,
            cursor: None,
        };
        assert!(params.validate().is_err());

        // Invalid limit (too large)
        let params = PaginationParams {
            limit: 101,
            cursor: None,
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_health_response_logic() {
        let healthy = HealthResponse::new(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_eq!(healthy.status, HealthStatus::Healthy);

        let degraded = HealthResponse::new(HealthStatus::Healthy, HealthStatus::Unhealthy);
        assert_eq!(degraded.status, HealthStatus::Unhealthy);

        // Ensure version is present
        assert!(!healthy.version.is_empty());
    }
    #[test]
    fn test_item_initialization_defaults() {
        let item = Item::new(
            "id_123".to_string(),
            "hash_abc".to_string(),
            "Name".to_string(),
            "Content".to_string(),
        );

        assert_eq!(item.blockchain_status, BlockchainStatus::Pending);
        assert!(item.blockchain_signature.is_none());
        assert_eq!(item.blockchain_retry_count, 0);
        assert!(item.blockchain_last_error.is_none());
        assert!(item.blockchain_next_retry_at.is_none());
        assert!(item.metadata.is_none());
        assert!(item.description.is_none());
    }

    #[test]
    fn test_item_default_impl() {
        let item = Item::default();
        assert_eq!(item.id, "default_id");
        assert_eq!(item.blockchain_status, BlockchainStatus::Pending);
    }

    #[test]
    fn test_create_item_request_description_validation() {
        // Valid description
        let mut req = CreateItemRequest::new("Name".to_string(), "Content".to_string());
        req.description = Some("Valid description".to_string());
        assert!(req.validate().is_ok());

        // Invalid description (too long)
        let long_desc = "a".repeat(10001);
        req.description = Some(long_desc);
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_metadata_request_nested_validation() {
        // Prepare base valid request
        let mut req = CreateItemRequest::new("Name".to_string(), "Content".to_string());

        // Case 1: Invalid Author Length
        let invalid_metadata_author = ItemMetadataRequest {
            author: Some("a".repeat(256)),
            version: Some("1.0".to_string()),
            tags: vec![],
            custom_fields: HashMap::new(),
        };
        req.metadata = Some(invalid_metadata_author);
        assert!(req.validate().is_err());

        // Reset
        let mut req = CreateItemRequest::new("Name".to_string(), "Content".to_string());

        // Case 2: Invalid Version Length
        let invalid_metadata_version = ItemMetadataRequest {
            author: Some("Author".to_string()),
            version: Some("v".repeat(51)),
            tags: vec![],
            custom_fields: HashMap::new(),
        };
        req.metadata = Some(invalid_metadata_version);
        assert!(req.validate().is_err());

        // Reset
        let mut req = CreateItemRequest::new("Name".to_string(), "Content".to_string());

        // Case 3: Too many tags
        let tags: Vec<String> = (0..21).map(|i| i.to_string()).collect();
        let invalid_metadata_tags = ItemMetadataRequest {
            author: None,
            version: None,
            tags,
            custom_fields: HashMap::new(),
        };
        req.metadata = Some(invalid_metadata_tags);
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_pagination_params_default() {
        let params = PaginationParams::default();
        assert_eq!(params.limit, 20);
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_paginated_response_constructors() {
        // Test empty
        let empty: PaginatedResponse<Item> = PaginatedResponse::empty();
        assert!(empty.items.is_empty());
        assert!(empty.next_cursor.is_none());
        assert!(!empty.has_more);

        // Test explicit new
        let items = vec![Item::default()];
        let response = PaginatedResponse::new(items.clone(), Some("cursor".to_string()), true);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.next_cursor, Some("cursor".to_string()));
        assert!(response.has_more);
    }

    #[test]
    fn test_health_response_status_combinations() {
        // Healthy + Degraded = Degraded
        let res = HealthResponse::new(HealthStatus::Healthy, HealthStatus::Degraded);
        assert_eq!(res.status, HealthStatus::Degraded);

        // Degraded + Healthy = Degraded
        let res = HealthResponse::new(HealthStatus::Degraded, HealthStatus::Healthy);
        assert_eq!(res.status, HealthStatus::Degraded);

        // Degraded + Degraded = Degraded
        let res = HealthResponse::new(HealthStatus::Degraded, HealthStatus::Degraded);
        assert_eq!(res.status, HealthStatus::Degraded);

        // Unhealthy + Degraded = Unhealthy (Unhealthy takes precedence)
        let res = HealthResponse::new(HealthStatus::Unhealthy, HealthStatus::Degraded);
        assert_eq!(res.status, HealthStatus::Unhealthy);

        // Degraded + Unhealthy = Unhealthy (Unhealthy takes precedence)
        let res = HealthResponse::new(HealthStatus::Degraded, HealthStatus::Unhealthy);
        assert_eq!(res.status, HealthStatus::Unhealthy);

        // Unhealthy + Healthy = Unhealthy
        let res = HealthResponse::new(HealthStatus::Unhealthy, HealthStatus::Healthy);
        assert_eq!(res.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_item_metadata_serialization_roundtrip() {
        let mut custom_fields = HashMap::new();
        custom_fields.insert("key1".to_string(), "value1".to_string());
        custom_fields.insert("key2".to_string(), "value2".to_string());

        let metadata = ItemMetadata {
            author: Some("John Doe".to_string()),
            version: Some("1.0.0".to_string()),
            tags: vec!["rust".to_string(), "test".to_string()],
            custom_fields,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ItemMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.author, Some("John Doe".to_string()));
        assert_eq!(deserialized.version, Some("1.0.0".to_string()));
        assert_eq!(deserialized.tags, vec!["rust", "test"]);
        assert_eq!(
            deserialized.custom_fields.get("key1"),
            Some(&"value1".to_string())
        );
    }

    #[test]
    fn test_item_metadata_default() {
        let metadata = ItemMetadata::default();
        assert!(metadata.author.is_none());
        assert!(metadata.version.is_none());
        assert!(metadata.tags.is_empty());
        assert!(metadata.custom_fields.is_empty());
    }

    #[test]
    fn test_error_response_construction() {
        let error = ErrorResponse {
            error: ErrorDetail {
                r#type: "validation_error".to_string(),
                message: "Name is required".to_string(),
            },
        };

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("validation_error"));
        assert!(json.contains("Name is required"));
    }

    #[test]
    fn test_rate_limit_response_construction() {
        let response = RateLimitResponse {
            error: ErrorDetail {
                r#type: "rate_limited".to_string(),
                message: "Too many requests".to_string(),
            },
            retry_after: 60,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("rate_limited"));
        assert!(json.contains("60"));
    }

    #[test]
    fn test_item_serialization_roundtrip() {
        let item = Item::new(
            "item_123".to_string(),
            "hash_abc".to_string(),
            "Test Item".to_string(),
            "Test Content".to_string(),
        );

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: Item = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "item_123");
        assert_eq!(deserialized.hash, "hash_abc");
        assert_eq!(deserialized.name, "Test Item");
        assert_eq!(deserialized.content, "Test Content");
    }

    #[test]
    fn test_blockchain_status_default() {
        let status = BlockchainStatus::default();
        assert_eq!(status, BlockchainStatus::Pending);
    }

    #[test]
    fn test_create_item_request_with_all_fields() {
        let mut req = CreateItemRequest::new("Name".to_string(), "Content".to_string());
        req.description = Some("Description".to_string());
        req.metadata = Some(ItemMetadataRequest {
            author: Some("Author".to_string()),
            version: Some("1.0".to_string()),
            tags: vec!["tag1".to_string()],
            custom_fields: HashMap::new(),
        });

        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_pagination_params_with_cursor() {
        let params = PaginationParams {
            limit: 50,
            cursor: Some("item_abc".to_string()),
        };

        assert!(params.validate().is_ok());
        assert_eq!(params.cursor, Some("item_abc".to_string()));
    }

    #[test]
    fn test_paginated_response_with_items() {
        let items = vec![
            Item::default(),
            Item::new(
                "id2".to_string(),
                "hash2".to_string(),
                "name2".to_string(),
                "content2".to_string(),
            ),
        ];
        let response = PaginatedResponse::new(items, Some("id2".to_string()), true);

        assert_eq!(response.items.len(), 2);
        assert_eq!(response.next_cursor, Some("id2".to_string()));
        assert!(response.has_more);
    }

    #[test]
    fn test_health_status_serialization() {
        assert_eq!(
            serde_json::to_string(&HealthStatus::Healthy).unwrap(),
            "\"healthy\""
        );
        assert_eq!(
            serde_json::to_string(&HealthStatus::Degraded).unwrap(),
            "\"degraded\""
        );
        assert_eq!(
            serde_json::to_string(&HealthStatus::Unhealthy).unwrap(),
            "\"unhealthy\""
        );
    }
}
