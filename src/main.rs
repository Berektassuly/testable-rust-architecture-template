// Allow unused code warnings - this is a template project where many types
// and methods are provided for users to choose from based on their needs.
#![allow(dead_code)]

// NOTE: Ensure the following dependencies are added to Cargo.toml:
// - anyhow = "1.0"
// - tokio = { version = "1.48", features = ["full"] }
// - dotenvy = "0.15"
// - axum = "0.8"
// - sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres"] }
// - ed25519-dalek = { version = "2.1", features = ["rand_core"] }
// - rand = "0.8"

mod api;
mod app;
mod domain;
mod infra;

use std::env;
use std::sync::Arc;

use anyhow::Result;
use dotenvy::dotenv;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

use crate::api::create_router;
use crate::app::AppState;
use crate::infra::{PostgresDatabase, RpcBlockchainClient};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    // Read required environment variables
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let blockchain_rpc_url = env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
    let issuer_private_key = env::var("ISSUER_PRIVATE_KEY").ok();

    // Initialize the signing key
    // If a private key is provided, decode it from Base58
    // Otherwise, generate a new keypair (useful for development/testing)
    let signing_key = if let Some(key_str) = issuer_private_key {
        if key_str == "YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE" || key_str.is_empty() {
            println!("⚠️  No valid ISSUER_PRIVATE_KEY provided, generating ephemeral keypair");
            println!("   This is fine for development, but set a real key for production!");
            SigningKey::generate(&mut OsRng)
        } else {
            // Decode Base58 private key
            let key_bytes = bs58::decode(&key_str)
                .into_vec()
                .expect("ISSUER_PRIVATE_KEY must be valid Base58");
            
            // Ed25519 secret keys are 32 bytes
            let key_array: [u8; 32] = key_bytes
                .try_into()
                .expect("ISSUER_PRIVATE_KEY must be 32 bytes");
            
            SigningKey::from_bytes(&key_array)
        }
    } else {
        println!("⚠️  ISSUER_PRIVATE_KEY not set, generating ephemeral keypair");
        SigningKey::generate(&mut OsRng)
    };

    // Log the public key for reference
    let public_key = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();
    println!("🔑 Using public key: {}", public_key);

    // Instantiate infrastructure components
    let postgres_db = PostgresDatabase::new(&database_url).await;
    let blockchain_client = RpcBlockchainClient::new(&blockchain_rpc_url, signing_key)
        .expect("Failed to create blockchain client");

    // Wrap infrastructure in Arc for thread-safe sharing
    let db_client = Arc::new(postgres_db);
    let blockchain_client = Arc::new(blockchain_client);

    // Create shared application state
    let app_state = Arc::new(AppState::new(db_client, blockchain_client));

    // Create the router with all routes configured
    let router = create_router(app_state);

    // Define server address
    let addr = "0.0.0.0:3000";

    // Create TCP listener
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("🚀 Server starting on http://{}", addr);

    // Run the server
    axum::serve(listener, router).await?;

    Ok(())
}

// IMPORTANT: This code will compile but will panic at runtime if the `todo!()`
// macros in the infrastructure layer (PostgresDatabase) are not replaced with
// actual implementation logic. The `todo!()` macros are placeholders indicating
// where real database queries need to be implemented.
//
// The RpcBlockchainClient provides a working implementation for basic operations,
// but full transaction submission requires additional blockchain-specific logic.