//! Testable Rust Architecture Template
//!
//! A production-ready Rust template demonstrating testable architecture through
//! trait-based abstraction and dependency injection.
//!
//! # Features
//!
//! - Clean Architecture with layered design
//! - Trait-based dependency injection for testability
//! - PostgreSQL with SQLx migrations
//! - Solana blockchain integration
//! - OpenAPI documentation with Swagger UI
//! - Rate limiting
//! - Graceful degradation with retry queues
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │              API Layer                  │
//! │   HTTP handlers, routing, validation    │
//! ├─────────────────────────────────────────┤
//! │           Application Layer             │
//! │    Business logic, orchestration        │
//! ├─────────────────────────────────────────┤
//! │             Domain Layer                │
//! │     Traits, types, errors (pure Rust)   │
//! ├─────────────────────────────────────────┤
//! │          Infrastructure Layer           │
//! │   Database, blockchain, external APIs   │
//! └─────────────────────────────────────────┘
//! ```

pub mod api;
pub mod app;
pub mod domain;
pub mod infra;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
