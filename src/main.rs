//! Application entry point.

use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use rand::rngs::OsRng;
use secrecy::SecretString;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use testable_rust_architecture_template::api::{
    RateLimitConfig, create_router, create_router_with_rate_limit,
};
use testable_rust_architecture_template::app::{AppState, WorkerConfig, spawn_worker};
use testable_rust_architecture_template::domain::TransactionSigner;
use testable_rust_architecture_template::infra::{
    AwsKmsSigner, LocalSigner, PostgresClient, PostgresConfig, RpcBlockchainClient,
    init_metrics_handle,
};

/// Application configuration
struct Config {
    database_url: String,
    blockchain_rpc_url: String,
    signer: Arc<dyn TransactionSigner>,
    api_auth_key: SecretString,
    host: String,
    port: u16,
    enable_rate_limiting: bool,
    rate_limit_config: RateLimitConfig,
    enable_background_worker: bool,
    worker_config: WorkerConfig,
}

impl Config {
    async fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL not set")?;
        let blockchain_rpc_url = env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
        let signer = Self::load_signer().await?;
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let enable_rate_limiting = env::var("ENABLE_RATE_LIMITING")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let enable_background_worker = env::var("ENABLE_BACKGROUND_WORKER")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);

        let api_auth_key = env::var("API_AUTH_KEY")
            .context("API_AUTH_KEY not set - security requires this environment variable")?;
        let api_auth_key = SecretString::from(api_auth_key);

        let rate_limit_config = RateLimitConfig::from_env();
        let worker_config = WorkerConfig {
            enabled: enable_background_worker,
            ..Default::default()
        };

        Ok(Self {
            database_url,
            blockchain_rpc_url,
            signer,
            api_auth_key,
            host,
            port,
            enable_rate_limiting,
            rate_limit_config,
            enable_background_worker,
            worker_config,
        })
    }

    async fn load_signer() -> Result<Arc<dyn TransactionSigner>> {
        let signer_type = env::var("SIGNER_TYPE").unwrap_or_else(|_| "LOCAL".to_string());
        let signer: Arc<dyn TransactionSigner> = match signer_type.to_uppercase().as_str() {
            "LOCAL" => {
                let key_str = match env::var("ISSUER_PRIVATE_KEY").ok() {
                    Some(s) if !s.is_empty() && s != "YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE" => s,
                    _ => {
                        warn!("No valid ISSUER_PRIVATE_KEY, generating ephemeral keypair");
                        let ephemeral = ed25519_dalek::SigningKey::generate(&mut OsRng);
                        bs58::encode(ephemeral.to_bytes()).into_string()
                    }
                };
                let secret = SecretString::from(key_str);
                Arc::new(LocalSigner::new(secret).context("Failed to parse ISSUER_PRIVATE_KEY")?)
            }
            "KMS" => {
                let key_id =
                    env::var("KMS_KEY_ID").context("KMS_KEY_ID required when SIGNER_TYPE=KMS")?;
                info!(key_id = %key_id, "Initializing AWS KMS signer...");
                let kms_signer = AwsKmsSigner::new(key_id)
                    .await
                    .context("Failed to initialize AWS KMS signer")?;
                Arc::new(kms_signer)
            }
            other => {
                anyhow::bail!("Invalid SIGNER_TYPE '{}': must be LOCAL or KMS", other);
            }
        };
        Ok(signer)
    }
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug,sqlx=warn"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

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
        _ = ctrl_c => info!("Received Ctrl+C"),
        _ = terminate => info!("Received SIGTERM"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    init_tracing();

    info!(
        "🏗️  Testable Rust Architecture Template v{}",
        env!("CARGO_PKG_VERSION")
    );

    let config = Config::from_env().await?;

    let public_key = config.signer.public_key();
    info!("🔑 Public key: {}", public_key);

    info!("📦 Initializing infrastructure...");

    // Initialize database
    let db_config = PostgresConfig::default();
    let postgres_client = PostgresClient::new(&config.database_url, db_config).await?;
    postgres_client.run_migrations().await?;
    info!("   ✓ Database connected and migrations applied");

    // Initialize blockchain client (signer injected; no raw key in client)
    let blockchain_client =
        RpcBlockchainClient::with_defaults(&config.blockchain_rpc_url, Arc::clone(&config.signer))?;
    info!("   ✓ Blockchain client created");

    // Create application state (PostgresClient implements both ItemRepository and OutboxRepository)
    let db = Arc::new(postgres_client);
    let item_repo =
        Arc::clone(&db) as Arc<dyn testable_rust_architecture_template::domain::ItemRepository>;
    let outbox_repo =
        Arc::clone(&db) as Arc<dyn testable_rust_architecture_template::domain::OutboxRepository>;
    let metrics_handle = init_metrics_handle();
    let app_state = Arc::new(AppState::new_with_metrics(
        item_repo,
        outbox_repo,
        Arc::new(blockchain_client),
        config.api_auth_key,
        metrics_handle,
    ));

    // Start background worker if enabled
    let worker_shutdown_tx = if config.enable_background_worker {
        let (_handle, shutdown_tx) =
            spawn_worker(Arc::clone(&app_state.service), config.worker_config);
        info!("   ✓ Background worker started");
        Some(shutdown_tx)
    } else {
        info!("   ○ Background worker disabled");
        None
    };

    // Create router
    let router = if config.enable_rate_limiting {
        info!("   ✓ Rate limiting enabled");
        create_router_with_rate_limit(app_state, config.rate_limit_config)
    } else {
        info!("   ○ Rate limiting disabled");
        create_router(app_state)
    };

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("🚀 Server starting on http://{}", addr);
    info!("📖 Swagger UI available at http://{}/swagger-ui", addr);
    info!("📄 OpenAPI spec at http://{}/api-docs/openapi.json", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Signal worker to shutdown
    if let Some(tx) = worker_shutdown_tx {
        let _ = tx.send(true);
    }

    info!("Server shutdown complete");
    Ok(())
}
