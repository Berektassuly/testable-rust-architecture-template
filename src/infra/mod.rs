//! The infrastructure layer, containing concrete implementations of domain traits.
//!
//! This module provides production-ready adapters for external systems such as
//! databases and blockchains. Each adapter implements the corresponding trait
//! defined in the domain layer, enabling dependency injection and testability.

pub mod blockchain;
pub mod database;

pub use blockchain::RpcBlockchainClient;
pub use database::PostgresDatabase;