//! Testable Rust Architecture Template
//!
//! A production-ready Rust template demonstrating testable architecture
//! through trait-based abstraction and dependency injection.
//!
//! # Architecture Overview
//!
//! This crate is organized into four main layers:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │                   API Layer                  │
//! │  HTTP handlers, routing, request validation  │
//! ├─────────────────────────────────────────────┤
//! │               Application Layer              │
//! │    Business logic, service orchestration     │
//! ├─────────────────────────────────────────────┤
//! │                 Domain Layer                 │
//! │   Traits, types, errors (no dependencies)    │
//! ├─────────────────────────────────────────────┤
//! │             Infrastructure Layer             │
//! │  Database adapters, blockchain clients, etc. │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Key Features
//!
//! - **Trait-based abstraction**: All external dependencies are abstracted behind traits
//! - **Dependency injection**: Components receive their dependencies through constructors
//! - **Testability**: Mock implementations enable fast, isolated unit tests
//! - **Error handling**: Hierarchical error types with proper context preservation
//! - **Validation**: Input validation using the `validator` crate
//! - **Logging**: Structured logging with `tracing`
//! - **Security**: Secret management with `secrecy` crate
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use testable_rust_architecture_template::api::create_router;
//! use testable_rust_architecture_template::app::AppState;
//! use testable_rust_architecture_template::infra::{PostgresClient, RpcBlockchainClient};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create infrastructure clients
//!     let db = Arc::new(PostgresClient::with_defaults(&database_url).await?);
//!     let blockchain = Arc::new(RpcBlockchainClient::with_defaults(&rpc_url, signing_key)?);
//!
//!     // Create application state
//!     let state = Arc::new(AppState::new(db, blockchain));
//!
//!     // Create and serve the router
//!     let router = create_router(state);
//!     axum::serve(listener, router).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod app;
pub mod domain;
pub mod infra;

// Test utilities are available in tests
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
