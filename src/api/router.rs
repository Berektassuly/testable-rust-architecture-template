//! HTTP routing configuration with rate limiting and OpenAPI documentation.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
use governor::{Quota, RateLimiter};
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

/// Shared rate limiter state (keyed by client IP to prevent global DoS)
pub struct RateLimitState {
    items_limiter: governor::RateLimiter<
        IpAddr,
        governor::state::keyed::DashMapStateStore<IpAddr>,
        governor::clock::DefaultClock,
    >,
    health_limiter: governor::RateLimiter<
        IpAddr,
        governor::state::keyed::DashMapStateStore<IpAddr>,
        governor::clock::DefaultClock,
    >,
    config: RateLimitConfig,
}

impl RateLimitState {
    pub fn new(config: RateLimitConfig) -> Self {
        let items_quota = Quota::per_second(NonZeroU32::new(config.general_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.general_burst).unwrap());
        let health_quota = Quota::per_second(NonZeroU32::new(config.health_rps).unwrap())
            .allow_burst(NonZeroU32::new(config.health_burst).unwrap());

        Self {
            items_limiter: RateLimiter::dashmap(items_quota),
            health_limiter: RateLimiter::dashmap(health_quota),
            config,
        }
    }
}

/// Extract client IP from request (X-Forwarded-For, X-Real-IP, or ConnectInfo).
/// Falls back to 0.0.0.0 when unknown to avoid blocking; unknown clients share one bucket.
fn client_ip_from_request<B>(request: &Request<B>) -> IpAddr {
    // Prefer proxy headers (client is first in X-Forwarded-For)
    if let Some(forwarded) = request.headers().get("x-forwarded-for") {
        if let Ok(s) = forwarded.to_str() {
            if let Some(first) = s.split(',').next() {
                let trimmed = first.trim();
                if let Ok(ip) = trimmed.parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }
    if let Some(real_ip) = request.headers().get("x-real-ip") {
        if let Ok(s) = real_ip.to_str() {
            if let Ok(ip) = s.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }
    // ConnectInfo may inject SocketAddr when using into_make_service_with_connect_info
    if let Some(addr) = request.extensions().get::<SocketAddr>() {
        return addr.ip();
    }
    // Fallback: unknown clients share one bucket (prevents total global DoS)
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

/// Rate limit middleware for items endpoints (per-IP to prevent global DoS)
async fn rate_limit_items_middleware(
    State(rate_limit): State<Arc<RateLimitState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let client_ip = client_ip_from_request(&request);
    match rate_limit.items_limiter.check_key(&client_ip) {
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

/// Rate limit middleware for health endpoints (per-IP to prevent global DoS)
async fn rate_limit_health_middleware(
    State(rate_limit): State<Arc<RateLimitState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let client_ip = client_ip_from_request(&request);
    match rate_limit.health_limiter.check_key(&client_ip) {
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
        .layer(middleware)
        .with_state(app_state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::get,
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::*;

    mod test_utils {
        use std::sync::Arc;

        use crate::app::AppState;
        use crate::test_utils::{MockBlockchainClient, MockProvider, mock_repos};

        impl AppState {
            pub fn new_for_test() -> Arc<Self> {
                let mock = Arc::new(MockProvider::new());
                let (item_repo, outbox_repo) = mock_repos(&mock);
                let bc = Arc::new(MockBlockchainClient::new());
                Arc::new(AppState::new(item_repo, outbox_repo, bc))
            }
        }
    }

    mod rate_limit_config_tests {
        use super::*;

        #[test]
        fn test_rate_limit_config_default() {
            let config = RateLimitConfig::default();
            assert_eq!(config.general_rps, 10);
            assert_eq!(config.general_burst, 20);
        }

        #[test]
        fn test_rate_limit_config_default_health_values() {
            let config = RateLimitConfig::default();
            assert_eq!(config.health_rps, 100);
            assert_eq!(config.health_burst, 100);
        }

        #[test]
        fn test_rate_limit_config_custom() {
            let config = RateLimitConfig {
                general_rps: 50,
                general_burst: 100,
                health_rps: 200,
                health_burst: 200,
            };
            assert_eq!(config.general_rps, 50);
            assert_eq!(config.general_burst, 100);
            assert_eq!(config.health_rps, 200);
            assert_eq!(config.health_burst, 200);
        }

        // Note: from_env tests are skipped because std::env::set_var/remove_var
        // are unsafe in Rust 2024 edition

        #[test]
        fn test_rate_limit_config_debug() {
            let config = RateLimitConfig::default();
            let debug_str = format!("{:?}", config);
            assert!(debug_str.contains("RateLimitConfig"));
            assert!(debug_str.contains("general_rps"));
        }

        #[test]
        fn test_rate_limit_config_clone() {
            let config1 = RateLimitConfig {
                general_rps: 42,
                general_burst: 84,
                health_rps: 100,
                health_burst: 100,
            };
            let config2 = config1.clone();
            assert_eq!(config1.general_rps, config2.general_rps);
            assert_eq!(config1.general_burst, config2.general_burst);
        }
    }

    mod middleware_tests {
        use super::*;
        use http_body_util::BodyExt;

        async fn dummy_handler() -> impl IntoResponse {
            StatusCode::OK
        }

        #[tokio::test]
        async fn test_rate_limit_items_middleware_blocks_request() {
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                ..Default::default()
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_items_middleware,
                    ));

            app.clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        #[tokio::test]
        async fn test_rate_limit_success_includes_limit_header() {
            let config = RateLimitConfig {
                general_rps: 100,
                general_burst: 100,
                ..Default::default()
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_items_middleware,
                    ));

            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().contains_key("X-RateLimit-Limit"));
            assert_eq!(response.headers().get("X-RateLimit-Limit").unwrap(), "100");
        }

        #[tokio::test]
        async fn test_rate_limit_exceeded_includes_headers() {
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                ..Default::default()
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_items_middleware,
                    ));

            // Exhaust the limit
            app.clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            // This should be rate limited
            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            assert!(response.headers().contains_key("X-RateLimit-Limit"));
            assert!(response.headers().contains_key("X-RateLimit-Remaining"));
            assert!(response.headers().contains_key("Retry-After"));
            assert_eq!(
                response.headers().get("X-RateLimit-Remaining").unwrap(),
                "0"
            );
        }

        #[tokio::test]
        async fn test_rate_limit_exceeded_response_body() {
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                ..Default::default()
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_items_middleware,
                    ));

            // Exhaust the limit
            app.clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body_str = String::from_utf8_lossy(&body_bytes);
            assert!(body_str.contains("rate_limited"));
            assert!(body_str.contains("slow down"));
        }

        #[tokio::test]
        async fn test_health_rate_limit_middleware_allows_high_volume() {
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                health_rps: 100,
                health_burst: 100,
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_health_middleware,
                    ));

            // Should allow multiple requests
            for _ in 0..10 {
                let response = app
                    .clone()
                    .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        }

        #[tokio::test]
        async fn test_health_rate_limit_eventually_blocks() {
            let config = RateLimitConfig {
                general_rps: 100,
                general_burst: 100,
                health_rps: 1,
                health_burst: 1,
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_health_middleware,
                    ));

            // First request should succeed
            let response = app
                .clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);

            // Second should be blocked
            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        /// Verifies per-IP rate limiting: one IP exhausting limit does not block another.
        #[tokio::test]
        async fn test_rate_limit_per_ip_prevents_global_dos() {
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                ..Default::default()
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_items_middleware,
                    ));

            // Exhaust limit for IP 192.168.1.1
            let req1 = Request::builder()
                .uri("/")
                .header("X-Forwarded-For", "192.168.1.1")
                .body(Body::empty())
                .unwrap();
            app.clone().oneshot(req1).await.unwrap();

            // Second request from same IP should be blocked
            let req2 = Request::builder()
                .uri("/")
                .header("X-Forwarded-For", "192.168.1.1")
                .body(Body::empty())
                .unwrap();
            let res2 = app.clone().oneshot(req2).await.unwrap();
            assert_eq!(res2.status(), StatusCode::TOO_MANY_REQUESTS);

            // Different IP should still be allowed
            let req3 = Request::builder()
                .uri("/")
                .header("X-Forwarded-For", "10.0.0.1")
                .body(Body::empty())
                .unwrap();
            let res3 = app.oneshot(req3).await.unwrap();
            assert_eq!(res3.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_health_rate_limit_includes_retry_after() {
            let config = RateLimitConfig {
                general_rps: 100,
                general_burst: 100,
                health_rps: 1,
                health_burst: 1,
            };

            let state = Arc::new(RateLimitState::new(config));

            let app =
                Router::new()
                    .route("/", get(dummy_handler))
                    .layer(middleware::from_fn_with_state(
                        state,
                        rate_limit_health_middleware,
                    ));

            // Exhaust
            app.clone()
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            let response = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert!(response.headers().contains_key("Retry-After"));
        }
    }

    mod router_tests {
        use super::*;
        use crate::app::AppState;

        #[tokio::test]
        async fn test_router_without_rate_limit_routes() {
            let app_state = AppState::new_for_test();
            let router = create_router(app_state);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/health/live")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_router_health_endpoint() {
            let app_state = AppState::new_for_test();
            let router = create_router(app_state);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_router_readiness_endpoint() {
            let app_state = AppState::new_for_test();
            let router = create_router(app_state);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/health/ready")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_router_items_get_nonexistent() {
            let app_state = AppState::new_for_test();
            let router = create_router(app_state);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/items/nonexistent-id")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            // Should return 404 for non-existent item
            assert_eq!(res.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn test_router_with_rate_limit_health_accessible() {
            let app_state = AppState::new_for_test();
            let config = RateLimitConfig::default();
            let router = create_router_with_rate_limit(app_state, config);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/health/live")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(res.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn test_router_with_rate_limit_items_accessible() {
            let app_state = AppState::new_for_test();
            let config = RateLimitConfig::default();
            let router = create_router_with_rate_limit(app_state, config);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/items/test-id")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            // Should return 404 (not found), not forbidden or error
            assert_eq!(res.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn test_router_with_rate_limit_applies_limits() {
            let app_state = AppState::new_for_test();
            let config = RateLimitConfig {
                general_rps: 1,
                general_burst: 1,
                health_rps: 100,
                health_burst: 100,
            };
            let router = create_router_with_rate_limit(app_state, config);

            // First request should succeed
            let res = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/items/test")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            // Returns 404 (not found) but that's fine - it means it got through
            assert!(res.status() == StatusCode::NOT_FOUND || res.status() == StatusCode::OK);

            // Second request should be rate limited
            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/items/test2")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        }

        #[tokio::test]
        async fn test_router_swagger_ui_accessible() {
            let app_state = AppState::new_for_test();
            let router = create_router(app_state);

            let res = router
                .oneshot(
                    Request::builder()
                        .uri("/swagger-ui/")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            // Swagger UI should return 200 OK
            assert_eq!(res.status(), StatusCode::OK);
        }
    }

    mod rate_limit_state_tests {
        use super::*;

        #[test]
        fn test_rate_limit_state_creation() {
            let config = RateLimitConfig::default();
            let _state = RateLimitState::new(config);
            // Should not panic
        }

        #[test]
        fn test_rate_limit_state_with_custom_config() {
            let config = RateLimitConfig {
                general_rps: 50,
                general_burst: 100,
                health_rps: 200,
                health_burst: 400,
            };
            let _state = RateLimitState::new(config);
            // Should not panic with various configurations
        }
    }
}
