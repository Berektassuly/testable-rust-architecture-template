//! Domain types with validation support.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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