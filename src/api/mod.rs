//! The API layer, containing web handlers and routing.

pub mod handlers;
pub mod router;

pub use router::create_router;