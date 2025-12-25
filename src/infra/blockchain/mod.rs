//! Blockchain client implementations.

pub mod solana;

pub use solana::{RpcBlockchainClient, RpcClientConfig, signing_key_from_base58};
