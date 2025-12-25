//! Test utilities and mock implementations.
//!
//! This module provides reusable mock implementations of domain traits
//! for use in unit and integration tests.

pub mod mocks;

pub use mocks::{MockBlockchainClient, MockDatabaseClient};
