//! HTTP routing configuration.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    routing::{get, post},
};
use tower::ServiceBuilder;
use tower_http::{
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::app::AppState;

use super::handlers::{
    create_item_handler, health_check_handler, liveness_handler, readiness_handler,
};

#[must_use]
pub fn create_router(app_state: Arc<AppState>) -> Router {
    let middleware = ServiceBuilder::new()
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    Router::new()
        .route("/items", post(create_item_handler))
        .route("/health", get(health_check_handler))
        .route("/health/live", get(liveness_handler))
        .route("/health/ready", get(readiness_handler))
        .layer(middleware)
        .with_state(app_state)
}

/// Creates a router with rate limiting enabled.
/// Note: Rate limiting temporarily disabled due to tower_governor incompatibility with axum 0.8
#[must_use]
pub fn create_router_with_rate_limit(app_state: Arc<AppState>) -> Router {
    // TODO: Re-enable when tower_governor supports axum 0.8
    create_router(app_state)
}
