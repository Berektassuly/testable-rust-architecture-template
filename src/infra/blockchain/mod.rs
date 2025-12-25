//! Concrete blockchain client implementations.
//!
//! This module contains production-ready blockchain adapters that implement
//! the `BlockchainClient` trait defined in the domain layer.

pub mod solana;

pub use solana::{signing_key_from_base58, RpcBlockchainClient, RpcClientConfig};
