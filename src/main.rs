//! Testable Rust Architecture Template
//!
//! Application entry point that wires together all components.

mod api;
mod app;
mod domain;
mod infra;
mod test_utils;

use std::env;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use secrecy::Secret;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::api::{create_router, create_router_with_rate_limit};
use crate::app::AppState;
use crate::infra::{signing_key_from_base58, PostgresClient, PostgresConfig, RpcBlockchainClient};

/// Application configuration loaded from environment variables.
struct Config {
    database_url: String,
    blockchain_rpc_url: String,
    signing_key: SigningKey,
    host: String,
    port: u16,
    enable_rate_limiting: bool,
}

impl Config {
    /// Loads configuration from environment variables.
    fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL").context(
            "DATABASE_URL environment variable is not set.\n\
             \n\
             To fix this:\n\
             1. Copy .env.example to .env: cp .env.example .env\n\
             2. Edit .env and set DATABASE_URL to your PostgreSQL connection string\n\
             \n\
             Example: DATABASE_URL=postgres://postgres:postgres@localhost:5432/app_dev",
        )?;

        let blockchain_rpc_url = env::var("SOLANA_RPC_URL").unwrap_or_else(|_| {
            info!("SOLANA_RPC_URL not set, using default devnet");
            "https://api.devnet.solana.com".to_string()
        });

        let signing_key = Self::load_signing_key()?;

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);

        let enable_rate_limiting = env::var("ENABLE_RATE_LIMITING")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        Ok(Self {
            database_url,
            blockchain_rpc_url,
            signing_key,
            host,
            port,
            enable_rate_limiting,
        })
    }

    fn load_signing_key() -> Result<SigningKey> {
        let issuer_private_key = env::var("ISSUER_PRIVATE_KEY").ok();

        match issuer_private_key {
            Some(key_str)
                if !key_str.is_empty() && key_str != "YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE" =>
            {
                info!("Loading signing key from environment");
                let secret = Secret::new(key_str);
                signing_key_from_base58(&secret).context("Failed to parse ISSUER_PRIVATE_KEY")
            }
            _ => {
                warn!("No valid ISSUER_PRIVATE_KEY provided, generating ephemeral keypair");
                warn!("This is fine for development, but set a real key for production!");
                Ok(SigningKey::generate(&mut OsRng))
            }
        }
    }
}

/// Initializes the tracing subscriber for structured logging.
fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Default log levels
        EnvFilter::new("info,tower_http=debug,sqlx=warn")
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
}

/// Handles graceful shutdown signals.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, starting graceful shutdown");
        }
        _ = terminate => {
            info!("Received SIGTERM, starting graceful shutdown");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file (optional)
    dotenv().ok();

    // Initialize tracing
    init_tracing();

    info!("🏗️  Testable Rust Architecture Template v{}", env!("CARGO_PKG_VERSION"));
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Load configuration
    let config = Config::from_env()?;

    // Display the public key
    let public_key = bs58::encode(config.signing_key.verifying_key().as_bytes()).into_string();
    info!("🔑 Public key: {}", public_key);

    // Initialize infrastructure
    info!("📦 Initializing infrastructure...");

    let db_config = if cfg!(debug_assertions) {
        PostgresConfig::development()
    } else {
        PostgresConfig::production()
    };

    let postgres_client = PostgresClient::new(&config.database_url, db_config)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    postgres_client
        .run_migrations()
        .await
        .context("Failed to run database migrations")?;

    info!("   ✓ Database connected and migrated");

    let blockchain_client =
        RpcBlockchainClient::with_defaults(&config.blockchain_rpc_url, config.signing_key)
            .context("Failed to create blockchain client")?;
    info!("   ✓ Blockchain client created");

    // Create application state
    let db_client = Arc::new(postgres_client);
    let blockchain_client = Arc::new(blockchain_client);
    let app_state = Arc::new(AppState::new(db_client, blockchain_client));
    info!("   ✓ Application state initialized");

    // Create the router
    let router = if config.enable_rate_limiting {
        info!("   ✓ Rate limiting enabled");
        create_router_with_rate_limit(app_state)
    } else {
        create_router(app_state)
    };

    // Create TCP listener
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    info!("");
    info!("🚀 Server starting on http://{}", addr);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("");
    info!("Available endpoints:");
    info!("   POST /items        - Create a new item");
    info!("   GET  /health       - Detailed health check");
    info!("   GET  /health/live  - Liveness probe");
    info!("   GET  /health/ready - Readiness probe");
    info!("");
    info!("Press Ctrl+C to stop the server");
    info!("");

    // Run the server with graceful shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shutdown complete");

    Ok(())
}
