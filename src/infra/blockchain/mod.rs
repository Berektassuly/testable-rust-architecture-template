//! Concrete blockchain client implementations.
//!
//! This module contains production-ready blockchain adapters that implement
//! the `BlockchainClient` trait defined in the domain layer.
//!
//! The default implementation uses a generic JSON-RPC client that works
//! with Solana and other compatible blockchain nodes without requiring
//! platform-specific dependencies like OpenSSL.

pub mod solana;

pub use solana::RpcBlockchainClient;