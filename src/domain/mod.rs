//! Domain layer containing core business types, traits, and error definitions.
//!
//! This module defines the contracts (traits) that external systems must implement,
//! as well as the shared domain types used throughout the application. The domain
//! layer has no dependencies on infrastructure or framework-specific code.

pub mod error;
pub mod traits;
pub mod types;

pub use error::AppError;
pub use traits::{BlockchainClient, DatabaseClient};
pub use types::{CreateItemRequest, Item};