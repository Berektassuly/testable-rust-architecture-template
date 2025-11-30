use std::sync::Arc;

use crate::domain::{BlockchainClient, DatabaseClient};

/// Shared application state for the Axum web server.
///
/// This struct holds thread-safe references to the database and blockchain
/// clients, allowing handlers to access these services without knowing
/// their concrete implementations. The use of `Arc<dyn Trait>` enables
/// runtime polymorphism and easy swapping of implementations for testing.
pub struct AppState {
    /// Database client for persistence operations.
    pub db_client: Arc<dyn DatabaseClient>,

    /// Blockchain client for on-chain operations.
    pub blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppState {
    /// Creates a new `AppState` instance with the provided clients.
    ///
    /// # Arguments
    ///
    /// * `db_client` - A thread-safe reference to a database client implementation.
    /// * `blockchain_client` - A thread-safe reference to a blockchain client implementation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let db = Arc::new(PostgresDatabase::new(config)?);
    /// let blockchain = Arc::new(SolanaClient::new(rpc_url)?);
    /// let state = AppState::new(db, blockchain);
    /// ```
    pub fn new(
        db_client: Arc<dyn DatabaseClient>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            db_client,
            blockchain_client,
        }
    }
}