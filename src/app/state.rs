//! Application state management.

use std::sync::Arc;

use crate::domain::{BlockchainClient, DatabaseClient};

use super::service::AppService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppService>,
    pub db_client: Arc<dyn DatabaseClient>,
    pub blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppState {
    /// Create a new application state
    #[must_use]
    pub fn new(
        db_client: Arc<dyn DatabaseClient>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        let service = Arc::new(AppService::new(
            Arc::clone(&db_client),
            Arc::clone(&blockchain_client),
        ));
        Self {
            service,
            db_client,
            blockchain_client,
        }
    }
}
