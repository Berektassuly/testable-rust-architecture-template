//! The API layer, containing web handlers and routing.

pub mod handlers;
pub mod router;

pub use handlers::ApiDoc;
pub use router::{RateLimitConfig, create_router, create_router_with_rate_limit};
