//! Application layer containing business logic and shared state.
//!
//! This module orchestrates operations between infrastructure components
//! using the trait abstractions defined in the domain layer. It contains
//! the application services and shared state management.
//!
//! # Architecture
//!
//! The application layer sits between the API layer and the domain/infrastructure
//! layers:
//!
//! ```text
//! ┌─────────────┐
//! │   API       │  ← HTTP handlers
//! ├─────────────┤
//! │   App       │  ← Business logic (this module)
//! ├─────────────┤
//! │  Domain     │  ← Traits and types
//! ├─────────────┤
//! │   Infra     │  ← Concrete implementations
//! └─────────────┘
//! ```

pub mod service;
pub mod state;

pub use service::AppService;
pub use state::AppState;
