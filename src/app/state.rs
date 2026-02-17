//! Application state management.

use std::sync::Arc;

use secrecy::SecretString;

use crate::domain::{BlockchainClient, ItemRepository, OutboxRepository};

use super::service::AppService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppService>,
    pub item_repo: Arc<dyn ItemRepository>,
    pub outbox_repo: Arc<dyn OutboxRepository>,
    pub blockchain_client: Arc<dyn BlockchainClient>,
    /// API key for authenticating write requests (POST /items, POST /items/{id}/retry).
    /// Used by auth middleware for constant-time comparison.
    pub api_auth_key: SecretString,
}

impl AppState {
    /// Create a new application state
    #[must_use]
    pub fn new(
        item_repo: Arc<dyn ItemRepository>,
        outbox_repo: Arc<dyn OutboxRepository>,
        blockchain_client: Arc<dyn BlockchainClient>,
        api_auth_key: SecretString,
    ) -> Self {
        let service = Arc::new(AppService::new(
            Arc::clone(&item_repo),
            Arc::clone(&outbox_repo),
            Arc::clone(&blockchain_client),
        ));
        Self {
            service,
            item_repo,
            outbox_repo,
            blockchain_client,
            api_auth_key,
        }
    }
}
