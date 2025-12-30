//! HTTP routing configuration with rate limiting and OpenAPI documentation.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post},
};
use governor::{
    Quota, RateLimiter,
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
};
use tower::ServiceBuilder;
use tower_http::{
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::app::AppState;
use crate::domain::{ErrorDetail, ErrorResponse, RateLimitResponse};

use super::handlers::{
    ApiDoc, create_item_handler, get_item_handler, health_check_handler, list_items_handler,
    liveness_handler, readiness_handler, retry_blockchain_handler,
};

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per second for general endpoints
    pub general_rps: u32,
    /// Burst size for general endpoints
    pub general_burst: u32,
    /// Requests per second for health endpoints
    pub health_rps: u32,
    /// Burst size for health endpoints
    pub health_burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            general_rps: 10,
            general_burst: 20,
            health_rps: 100,
            health_burst: 100,
        }
    }
}

impl RateLimitConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        let general_rps = std::env::var("RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let general_burst = std::env::var("RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);

        Self {
            general_rps,
            general_burst,
            health_rps: 100,
            health_burst: 100,
        }
    }
}

/// Shared rate limiter state
pub struct RateLimitState {
    items_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    health_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    config: RateLimitConfig,
}

impl RateLimitState {
    pub fn new(config: RateLimitConfig) -> Self {
        let items_quota = Quota::per_second(NonZeroU32::new(config.general_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.general_burst).unwrap());
        let health_quota = Quota::per_second(NonZeroU32::new(config.health_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.health_burst).unwrap());

        Self {
            items_limiter: RateLimiter::direct(items_quota),
            health_limiter: RateLimiter::direct(health_quota),
            config,
        }
    }
}

/// Rate limit middleware for items endpoints
async fn rate_limit_items_middleware(
    State(rate_limit): State<Arc<RateLimitState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    match rate_limit.items_limiter.check() {
        Ok(_) => {
            let mut response = next.run(request).await;
            // Add rate limit headers
            let headers = response.headers_mut();
            headers.insert(
                "X-RateLimit-Limit",
                rate_limit.config.general_rps.to_string().parse().unwrap(),
            );
            response
        }
        Err(not_until) => {
            let wait_time = not_until.wait_time_from(governor::clock::Clock::now(
                &governor::clock::DefaultClock::default(),
            ));
            let retry_after = wait_time.as_secs();

            let body = RateLimitResponse {
                error: ErrorDetail {
                    r#type: "rate_limited".to_string(),
                    message: "Rate limit exceeded. Please slow down your requests.".to_string(),
                },
                retry_after,
            };

            let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            let headers = response.headers_mut();
            headers.insert(
                "X-RateLimit-Limit",
                rate_limit.config.general_rps.to_string().parse().unwrap(),
            );
            headers.insert("X-RateLimit-Remaining", "0".parse().unwrap());
            headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
            response
        }
    }
}

/// Rate limit middleware for health endpoints
async fn rate_limit_health_middleware(
    State(rate_limit): State<Arc<RateLimitState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    match rate_limit.health_limiter.check() {
        Ok(_) => next.run(request).await,
        Err(not_until) => {
            let wait_time = not_until.wait_time_from(governor::clock::Clock::now(
                &governor::clock::DefaultClock::default(),
            ));
            let retry_after = wait_time.as_secs();

            let body = ErrorResponse {
                error: ErrorDetail {
                    r#type: "rate_limited".to_string(),
                    message: "Rate limit exceeded".to_string(),
                },
            };

            let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            response
                .headers_mut()
                .insert("Retry-After", retry_after.to_string().parse().unwrap());
            response
        }
    }
}

/// Create router without rate limiting
pub fn create_router(app_state: Arc<AppState>) -> Router {
    let middleware = ServiceBuilder::new()
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    // Items routes
    let items_routes = Router::new()
        .route("/", post(create_item_handler).get(list_items_handler))
        .route("/{id}", get(get_item_handler))
        .route("/{id}/retry", post(retry_blockchain_handler));

    // Health routes
    let health_routes = Router::new()
        .route("/", get(health_check_handler))
        .route("/live", get(liveness_handler))
        .route("/ready", get(readiness_handler));

    Router::new()
        .nest("/items", items_routes)
        .nest("/health", health_routes)
        .route(
            "/api-docs/openapi.json",
            get(|| async { Json(ApiDoc::openapi()) }),
        )
        .layer(middleware)
        .with_state(app_state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

/// Create router with rate limiting enabled
pub fn create_router_with_rate_limit(app_state: Arc<AppState>, config: RateLimitConfig) -> Router {
    let rate_limit_state = Arc::new(RateLimitState::new(config));

    let middleware = ServiceBuilder::new()
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    // Items routes with rate limiting
    let items_routes = Router::new()
        .route("/", post(create_item_handler).get(list_items_handler))
        .route("/{id}", get(get_item_handler))
        .route("/{id}/retry", post(retry_blockchain_handler))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&rate_limit_state),
            rate_limit_items_middleware,
        ));

    // Health routes with separate rate limiting
    let health_routes = Router::new()
        .route("/", get(health_check_handler))
        .route("/live", get(liveness_handler))
        .route("/ready", get(readiness_handler))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&rate_limit_state),
            rate_limit_health_middleware,
        ));

    Router::new()
        .nest("/items", items_routes)
        .nest("/health", health_routes)
        .route(
            "/api-docs/openapi.json",
            get(|| async { Json(ApiDoc::openapi()) }),
        )
        .layer(middleware)
        .with_state(app_state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}
