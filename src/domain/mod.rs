//! Domain layer containing core business types, traits, and error definitions.
//!
//! This module defines the contracts (traits) that external systems must implement,
//! as well as the shared domain types used throughout the application. The domain
//! layer has no dependencies on infrastructure or framework-specific code.
//!
//! # Structure
//!
//! - `error` - Hierarchical error types with proper context preservation
//! - `traits` - Contracts for database and blockchain operations
//! - `types` - Domain models with validation support

pub mod error;
pub mod traits;
pub mod types;

// Re-export commonly used types
pub use error::{
    AppError, AppResult, BlockchainError, ConfigError, DatabaseError, ExternalServiceError,
    ResultExt, ValidationError,
};
pub use traits::{BlockchainClient, DatabaseClient};
pub use types::{
    BlockchainRecord, CreateItemRequest, HealthResponse, HealthStatus, Item, ItemMetadata,
    ItemMetadataRequest, ItemResponse, PaginatedResponse, PaginationParams, UpdateItemRequest,
    WriteResult,
};
