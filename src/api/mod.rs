//! The API layer, containing web handlers and routing.
//!
//! This module provides the HTTP interface for the application using Axum.
//! It handles request parsing, response formatting, and delegates business
//! logic to the application layer.

pub mod handlers;
pub mod router;

pub use router::{create_router, create_router_with_rate_limit};
