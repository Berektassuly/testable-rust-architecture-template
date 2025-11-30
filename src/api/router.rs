use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};

use crate::app::AppState;

use super::handlers::{create_item_handler, health_check_handler};

/// Creates the application router with all routes configured.
///
/// This function sets up the URL routing for the application and attaches
/// the shared application state, making it available to all handlers via
/// Axum's `State` extractor.
///
/// # Arguments
///
/// * `app_state` - The shared application state containing database and blockchain clients.
///
/// # Returns
///
/// A fully configured `Router` ready to be served.
pub fn create_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/items", post(create_item_handler))
        .route("/health", get(health_check_handler))
        .with_state(app_state)
}