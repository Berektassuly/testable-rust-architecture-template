//! Domain layer containing core business types, traits, and error definitions.

pub mod error;
pub mod traits;
pub mod types;

pub use error::{BlockchainError, ConfigError, HealthCheckError, ItemError, ValidationError};
pub use traits::{BlockchainClient, ItemRepository, OutboxRepository};
pub use types::{
    BlockchainStatus, CreateItemRequest, ErrorDetail, ErrorResponse, HealthResponse, HealthStatus,
    Item, ItemMetadata, ItemMetadataRequest, OutboxStatus, PaginatedResponse, PaginationParams,
    RateLimitResponse, SolanaOutboxEntry, SolanaOutboxPayload,
    build_solana_outbox_payload_from_item, build_solana_outbox_payload_from_request,
    compute_blockchain_hash,
};
