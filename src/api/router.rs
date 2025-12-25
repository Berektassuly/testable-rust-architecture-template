//! HTTP routing configuration.
//!
//! This module sets up all routes and middleware for the application.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    routing::{get, post},
    Router,
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

/// Creates the application router with all routes and middleware configured.
///
/// # Middleware Stack
///
/// The router is configured with the following middleware (applied in order):
/// 1. Request tracing (logging)
/// 2. Request timeout (30 seconds default)
///
/// # Routes
///
/// | Method | Path           | Handler                | Description           |
/// |--------|----------------|------------------------|-----------------------|
/// | POST   | /items         | create_item_handler    | Create a new item     |
/// | GET    | /health        | health_check_handler   | Detailed health check |
/// | GET    | /health/live   | liveness_handler       | Liveness probe        |
/// | GET    | /health/ready  | readiness_handler      | Readiness probe       |
///
/// # Arguments
///
/// * `app_state` - The shared application state.
///
/// # Returns
///
/// A fully configured `Router` ready to be served.
///
/// # Example
///
/// ```ignore
/// let state = Arc::new(AppState::new(db, blockchain));
/// let router = create_router(state);
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await?;
/// axum::serve(listener, router).await?;
/// ```
#[must_use]
pub fn create_router(app_state: Arc<AppState>) -> Router {
    // Build the middleware stack
    let middleware = ServiceBuilder::new()
        // Add request tracing
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // Add request timeout
        .layer(TimeoutLayer::new(Duration::from_secs(30)));

    // Build the router
    Router::new()
        // API routes
        .route("/items", post(create_item_handler))
        // Health check routes
        .route("/health", get(health_check_handler))
        .route("/health/live", get(liveness_handler))
        .route("/health/ready", get(readiness_handler))
        // Apply middleware
        .layer(middleware)
        // Attach state
        .with_state(app_state)
}

/// Creates a router with rate limiting enabled.
///
/// Use this for production deployments where you want to protect
/// against abuse and ensure fair resource allocation.
///
/// # Rate Limit Configuration
///
/// - Burst size: 50 requests
/// - Replenish rate: 10 requests per second
///
/// # Arguments
///
/// * `app_state` - The shared application state.
///
/// # Returns
///
/// A router with rate limiting middleware applied.
#[must_use]
pub fn create_router_with_rate_limit(app_state: Arc<AppState>) -> Router {
    use governor::{Quota, RateLimiter};
    use std::num::NonZeroU32;
    use tower_governor::{GovernorLayer, GovernorConfigBuilder};

    // Configure rate limiting
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(50)
        .finish()
        .expect("Failed to build rate limiter config");

    let governor_limiter = governor_conf.limiter().clone();

    // Build the middleware stack with rate limiting
    let middleware = ServiceBuilder::new()
        // Add request tracing
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // Add request timeout
        .layer(TimeoutLayer::new(Duration::from_secs(30)));

    // Build the router
    Router::new()
        // API routes (with rate limiting)
        .route("/items", post(create_item_handler))
        .layer(GovernorLayer {
            config: Box::leak(Box::new(governor_conf)),
        })
        // Health check routes (no rate limiting)
        .route("/health", get(health_check_handler))
        .route("/health/live", get(liveness_handler))
        .route("/health/ready", get(readiness_handler))
        // Apply common middleware
        .layer(middleware)
        // Attach state
        .with_state(app_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    fn create_test_state() -> Arc<AppState> {
        let db = Arc::new(MockDatabaseClient::new());
        let blockchain = Arc::new(MockBlockchainClient::new());
        Arc::new(AppState::new(db, blockchain))
    }

    #[tokio::test]
    async fn test_router_health_endpoint() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert!(response.status().is_success());
    }

    #[tokio::test]
    async fn test_router_liveness_endpoint() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .method("GET")
            .uri("/health/live")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert!(response.status().is_success());
    }

    #[tokio::test]
    async fn test_router_readiness_endpoint() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .method("GET")
            .uri("/health/ready")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert!(response.status().is_success());
    }

    #[tokio::test]
    async fn test_router_items_endpoint() {
        let state = create_test_state();
        let router = create_router(state);

        let payload = serde_json::json!({
            "name": "Test",
            "content": "Content"
        });

        let request = Request::builder()
            .method("POST")
            .uri("/items")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert!(response.status().is_success());
    }

    #[tokio::test]
    async fn test_router_not_found() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .method("GET")
            .uri("/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
