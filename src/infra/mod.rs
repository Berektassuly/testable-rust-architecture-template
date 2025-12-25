//! The infrastructure layer, containing concrete implementations of domain traits.
//!
//! This module provides production-ready adapters for external systems such as
//! databases and blockchains. Each adapter implements the corresponding trait
//! defined in the domain layer, enabling dependency injection and testability.
//!
//! # Available Implementations
//!
//! ## Database
//!
//! - `PostgresClient` - PostgreSQL with connection pooling via SQLx
//!
//! ## Blockchain
//!
//! - `RpcBlockchainClient` - JSON-RPC client for Solana-compatible chains

pub mod blockchain;
pub mod database;

pub use blockchain::{signing_key_from_base58, RpcBlockchainClient, RpcClientConfig};
pub use database::{PostgresClient, PostgresConfig};
