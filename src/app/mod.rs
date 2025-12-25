//! Application layer containing business logic and shared state.

pub mod service;
pub mod state;

pub use service::AppService;
pub use state::AppState;
