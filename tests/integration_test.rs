//! Integration tests.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use testable_rust_architecture_template::api::create_router;
use testable_rust_architecture_template::app::AppState;
use testable_rust_architecture_template::domain::{
    CreateItemRequest, HealthResponse, HealthStatus, Item,
};
use testable_rust_architecture_template::test_utils::{MockBlockchainClient, MockDatabaseClient};

fn create_test_state() -> Arc<AppState> {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    Arc::new(AppState::new(db, blockchain))
}

#[tokio::test]
async fn test_create_item_success() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let item: Item = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item.name, "Test Item");
}

#[tokio::test]
async fn test_create_item_validation_error() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest::new("".to_string(), "Content".to_string());

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_health_check() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let health: HealthResponse = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(health.status, HealthStatus::Healthy);
}

#[tokio::test]
async fn test_liveness() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/health/live")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_readiness_healthy() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_readiness_unhealthy() {
    let db = Arc::new(MockDatabaseClient::new());
    db.set_healthy(false);
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(db, blockchain));
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_database_failure() {
    let db = Arc::new(MockDatabaseClient::failing("DB error"));
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(db, blockchain));
    let router = create_router(state);

    let payload = CreateItemRequest::new("Test".to_string(), "Content".to_string());

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_blockchain_failure() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::failing("RPC error"));
    let state = Arc::new(AppState::new(db, blockchain));
    let router = create_router(state);

    let payload = CreateItemRequest::new("Test".to_string(), "Content".to_string());

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
