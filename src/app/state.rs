//! Application state management.
//!
//! This module provides the shared application state that is
//! accessible to all request handlers via Axum's State extractor.

use std::sync::Arc;

use crate::domain::{BlockchainClient, DatabaseClient};

use super::service::AppService;

/// Shared application state for the Axum web server.
///
/// This struct holds thread-safe references to all application services
/// and clients, allowing handlers to access them without knowing their
/// concrete implementations.
///
/// # Thread Safety
///
/// All contained types are wrapped in `Arc` and implement `Send + Sync`,
/// making `AppState` safe to share across async tasks.
///
/// # Example
///
/// ```ignore
/// use std::sync::Arc;
///
/// let db = Arc::new(PostgresClient::new(&config)?);
/// let blockchain = Arc::new(SolanaClient::new(&rpc_url)?);
/// let state = AppState::new(db, blockchain);
///
/// // Use with Axum
/// let router = Router::new()
///     .route("/items", post(create_item))
///     .with_state(Arc::new(state));
/// ```
#[derive(Clone)]
pub struct AppState {
    /// The application service containing business logic.
    pub service: Arc<AppService>,

    /// Database client for persistence operations.
    pub db_client: Arc<dyn DatabaseClient>,

    /// Blockchain client for on-chain operations.
    pub blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppState {
    /// Creates a new `AppState` instance with the provided clients.
    ///
    /// This constructor also creates the `AppService` internally,
    /// wiring it to the provided clients.
    ///
    /// # Arguments
    ///
    /// * `db_client` - A thread-safe reference to a database client implementation.
    /// * `blockchain_client` - A thread-safe reference to a blockchain client implementation.
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

    /// Creates a new `AppState` with a custom service.
    ///
    /// This is useful for testing when you want to inject a pre-configured service.
    #[must_use]
    pub fn with_service(
        service: Arc<AppService>,
        db_client: Arc<dyn DatabaseClient>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            service,
            db_client,
            blockchain_client,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    #[test]
    fn test_app_state_creation() {
        let db = Arc::new(MockDatabaseClient::new());
        let blockchain = Arc::new(MockBlockchainClient::new());

        let state = AppState::new(db, blockchain);

        // Verify state is created and service is accessible
        assert!(Arc::strong_count(&state.service) >= 1);
    }

    #[test]
    fn test_app_state_is_clone() {
        let db = Arc::new(MockDatabaseClient::new());
        let blockchain = Arc::new(MockBlockchainClient::new());

        let state = AppState::new(db, blockchain);
        let cloned = state.clone();

        // Both should point to the same service
        assert!(Arc::ptr_eq(&state.service, &cloned.service));
    }
}
