//! Application layer containing business logic and shared state.
//!
//! This module orchestrates operations between infrastructure components
//! using the trait abstractions defined in the domain layer. It contains
//! the application services and shared state management.

pub mod service;
pub mod state;

pub use service::AppService;
pub use state::AppState;