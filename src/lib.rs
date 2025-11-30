//! Testable Rust Architecture Template
//!
//! A production-ready Rust template demonstrating testable architecture
//! through trait-based abstraction and dependency injection.
//!
//! # Architecture Overview
//!
//! This crate is organized into three main layers:
//!
//! - **Domain Layer** (`domain`): Core traits, types, and error definitions.
//!   This layer has no dependencies on infrastructure or framework-specific code.
//!
//! - **Application Layer** (`app`): Business logic and shared state management.
//!   This layer orchestrates operations using trait abstractions from the domain layer.
//!
//! - **Infrastructure Layer** (`infra`): Concrete implementations of domain traits.
//!   This layer contains adapters for databases, blockchains, and external services.
//!
//! - **API Layer** (`api`): HTTP handlers and routing using Axum.
//!
//! # Example
//!
//! ```ignore
//! use testable_rust_architecture_template::app::{AppState, AppService};
//! use testable_rust_architecture_template::api::create_router;
//! use std::sync::Arc;
//!
//! // Create application state with injected dependencies
//! let state = Arc::new(AppState::new(db_client, blockchain_client));
//!
//! // Create router
//! let router = create_router(state);
//! ```

pub mod api;
pub mod app;
pub mod domain;
pub mod infra;