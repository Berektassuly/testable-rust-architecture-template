// Allow unused code warnings - this is a template project where many types
// and methods are provided for users to choose from based on their needs.
#![allow(dead_code)]

//! Testable Rust Architecture Template
//!
//! This is the application entry point that wires together all components.

mod api;
mod app;
mod domain;
mod infra;

use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

use crate::api::create_router;
use crate::app::AppState;
use crate::infra::{PostgresDatabase, RpcBlockchainClient};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file (optional)
    dotenv().ok();

    println!("🏗️  Testable Rust Architecture Template");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Read environment variables with helpful error messages
    let database_url = env::var("DATABASE_URL").context(
        "DATABASE_URL environment variable is not set.\n\
         \n\
         To fix this:\n\
         1. Copy .env.example to .env: cp .env.example .env\n\
         2. Edit .env and set DATABASE_URL to your PostgreSQL connection string\n\
         \n\
         Example: DATABASE_URL=postgres://postgres:postgres@localhost:5432/app_dev"
    )?;

    let blockchain_rpc_url = env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| {
            println!("ℹ️  SOLANA_RPC_URL not set, using default: https://api.devnet.solana.com");
            "https://api.devnet.solana.com".to_string()
        });

    let issuer_private_key = env::var("ISSUER_PRIVATE_KEY").ok();

    // Initialize the signing key
    let signing_key = match issuer_private_key {
        Some(key_str) if !key_str.is_empty() 
            && key_str != "YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE" => {
            println!("🔐 Loading signing key from environment...");
            
            let key_bytes = bs58::decode(&key_str)
                .into_vec()
                .context("ISSUER_PRIVATE_KEY must be valid Base58")?;
            
            let key_array: [u8; 32] = key_bytes
                .try_into()
                .map_err(|v: Vec<u8>| anyhow::anyhow!(
                    "ISSUER_PRIVATE_KEY must be 32 bytes, got {} bytes", v.len()
                ))?;
            
            SigningKey::from_bytes(&key_array)
        }
        _ => {
            println!("⚠️  No valid ISSUER_PRIVATE_KEY provided");
            println!("   Generating ephemeral keypair (fine for development)");
            SigningKey::generate(&mut OsRng)
        }
    };

    // Display the public key
    let public_key = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();
    println!("🔑 Public key: {}", public_key);

    // Instantiate infrastructure components
    println!("\n📦 Initializing infrastructure...");
    
    let postgres_db = PostgresDatabase::new(&database_url).await;
    println!("   ✓ Database client created");
    
    let blockchain_client = RpcBlockchainClient::new(&blockchain_rpc_url, signing_key)
        .context("Failed to create blockchain client")?;
    println!("   ✓ Blockchain client created");

    // Wrap infrastructure in Arc for thread-safe sharing
    let db_client = Arc::new(postgres_db);
    let blockchain_client = Arc::new(blockchain_client);

    // Create shared application state
    let app_state = Arc::new(AppState::new(db_client, blockchain_client));
    println!("   ✓ Application state initialized");

    // Create the router with all routes configured
    let router = create_router(app_state);

    // Define server address
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("{}:{}", host, port);

    // Create TCP listener
    let listener = tokio::net::TcpListener::bind(&addr).await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    println!("\n🚀 Server starting on http://{}", addr);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("\nAvailable endpoints:");
    println!("   POST /items  - Create a new item");
    println!("   GET  /health - Health check");
    println!("\nPress Ctrl+C to stop the server\n");

    // Run the server
    axum::serve(listener, router).await?;

    Ok(())
}