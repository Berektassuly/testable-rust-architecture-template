//! Domain types with validation support.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub hash: String,
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<ItemMetadata>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Item {
    #[must_use]
    pub fn new(id: String, hash: String, name: String) -> Self {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ItemMetadata {
    pub author: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub custom_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateItemRequest {
    #[validate(length(
        min = 1,
        max = 255,
        message = "Name must be between 1 and 255 characters"
    ))]
    pub name: String,
    #[validate(length(max = 10000, message = "Description must not exceed 10000 characters"))]
    pub description: Option<String>,
    #[validate(length(
        min = 1,
        max = 1048576,
        message = "Content must be between 1 and 1048576 characters"
    ))]
    pub content: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ItemMetadataRequest {
    #[validate(length(max = 255))]
    pub author: Option<String>,
    #[validate(length(max = 50))]
    pub version: Option<String>,
    #[validate(length(max = 20))]
    pub tags: Vec<String>,
    pub custom_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub database: HealthStatus,
    pub blockchain: HealthStatus,
    pub timestamp: DateTime<Utc>,
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
