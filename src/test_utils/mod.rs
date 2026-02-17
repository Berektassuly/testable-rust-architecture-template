//! Test utilities and mock implementations.

pub mod mocks;

pub use mocks::{MockBlockchainClient, MockConfig, MockProvider, mock_repos};

use secrecy::SecretString;

/// API key used in tests. Use with `x-api-key` header when making authenticated requests.
#[must_use]
pub fn test_api_key() -> SecretString {
    SecretString::from("test-api-key".to_string())
}
