//! Integration tests for the application.
//!
//! These tests verify the full request/response cycle using mock implementations
//! of external services, enabling fast and reliable testing without real
//! infrastructure dependencies.

use std::collections::HashMap;
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
    CreateItemRequest, HealthResponse, HealthStatus, Item, ItemMetadataRequest,
};
use testable_rust_architecture_template::test_utils::{
    mocks::MockConfig, MockBlockchainClient, MockDatabaseClient,
};

/// Helper to create test application state.
fn create_test_state() -> Arc<AppState> {
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());
    Arc::new(AppState::new(mock_db, mock_blockchain))
}

/// Helper to create test state with failing database.
fn create_test_state_with_db_failure() -> Arc<AppState> {
    let mock_db = Arc::new(MockDatabaseClient::failing("Database connection failed"));
    let mock_blockchain = Arc::new(MockBlockchainClient::new());
    Arc::new(AppState::new(mock_db, mock_blockchain))
}

/// Helper to create test state with failing blockchain.
fn create_test_state_with_blockchain_failure() -> Arc<AppState> {
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::failing("RPC timeout"));
    Arc::new(AppState::new(mock_db, mock_blockchain))
}

// =============================================================================
// Happy Path Tests
// =============================================================================

#[tokio::test]
async fn test_create_item_success_e2e() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "Test Item".to_string(),
        description: Some("A test item description".to_string()),
        content: "Test content for the item".to_string(),
        metadata: None,
    };

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
    assert_eq!(
        item.description,
        Some("A test item description".to_string())
    );
    assert!(item.id.starts_with("item_"));
    assert!(item.hash.starts_with("hash_"));
}

#[tokio::test]
async fn test_create_item_with_metadata() {
    let state = create_test_state();
    let router = create_router(state);

    let mut custom_fields = HashMap::new();
    custom_fields.insert("key1".to_string(), "value1".to_string());

    let metadata = ItemMetadataRequest {
        author: Some("Test Author".to_string()),
        version: Some("1.0.0".to_string()),
        tags: vec!["test".to_string(), "integration".to_string()],
        custom_fields,
    };

    let payload = CreateItemRequest {
        name: "Item with Metadata".to_string(),
        description: None,
        content: "Content here".to_string(),
        metadata: Some(metadata),
    };

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

    assert_eq!(item.name, "Item with Metadata");
    assert!(item.metadata.is_some());

    let item_metadata = item.metadata.unwrap();
    assert_eq!(item_metadata.author, Some("Test Author".to_string()));
    assert_eq!(item_metadata.version, Some("1.0.0".to_string()));
    assert_eq!(item_metadata.tags.len(), 2);
}

// =============================================================================
// Health Check Tests
// =============================================================================

#[tokio::test]
async fn test_health_check_endpoint() {
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
    assert_eq!(health.database, HealthStatus::Healthy);
    assert_eq!(health.blockchain, HealthStatus::Healthy);
}

#[tokio::test]
async fn test_liveness_probe() {
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
async fn test_readiness_probe_healthy() {
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
async fn test_readiness_probe_unhealthy_db() {
    let mock_db = Arc::new(MockDatabaseClient::new());
    mock_db.set_healthy(false);
    let mock_blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(mock_db, mock_blockchain));
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
async fn test_readiness_probe_unhealthy_blockchain() {
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());
    mock_blockchain.set_healthy(false);
    let state = Arc::new(AppState::new(mock_db, mock_blockchain));
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/health/ready")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// =============================================================================
// Validation Error Tests
// =============================================================================

#[tokio::test]
async fn test_create_item_empty_name() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "".to_string(), // Empty name should fail
        description: None,
        content: "Some content".to_string(),
        metadata: None,
    };

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(error["error"]["type"].as_str().unwrap().contains("validation"));
}

#[tokio::test]
async fn test_create_item_empty_content() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "Valid Name".to_string(),
        description: None,
        content: "".to_string(), // Empty content should fail
        metadata: None,
    };

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
async fn test_create_item_name_too_long() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "a".repeat(256), // 256 chars, max is 255
        description: None,
        content: "Some content".to_string(),
        metadata: None,
    };

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
async fn test_invalid_json_returns_error() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from("{ invalid json }"))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert!(response.status().is_client_error());
}

// =============================================================================
// Infrastructure Failure Tests
// =============================================================================

#[tokio::test]
async fn test_create_item_database_failure() {
    let state = create_test_state_with_db_failure();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "Test Item".to_string(),
        description: None,
        content: "Test content".to_string(),
        metadata: None,
    };

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(error["error"]["type"]
        .as_str()
        .unwrap()
        .contains("database"));
}

#[tokio::test]
async fn test_create_item_blockchain_failure() {
    let state = create_test_state_with_blockchain_failure();
    let router = create_router(state);

    let payload = CreateItemRequest {
        name: "Test Item".to_string(),
        description: None,
        content: "Test content".to_string(),
        metadata: None,
    };

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(error["error"]["type"]
        .as_str()
        .unwrap()
        .contains("blockchain"));
}

// =============================================================================
// Error Response Format Tests
// =============================================================================

#[tokio::test]
async fn test_error_response_format() {
    let state = create_test_state();
    let router = create_router(state);

    // Send invalid request to trigger error
    let payload = CreateItemRequest {
        name: "".to_string(),
        description: None,
        content: "".to_string(),
        metadata: None,
    };

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify error response structure
    assert!(error["error"].is_object());
    assert!(error["error"]["type"].is_string());
    assert!(error["error"]["message"].is_string());
}

// =============================================================================
// Route Not Found Tests
// =============================================================================

#[tokio::test]
async fn test_not_found_route() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/nonexistent")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_method_not_allowed() {
    let state = create_test_state();
    let router = create_router(state);

    // GET on /items should not be allowed (only POST)
    let request = Request::builder()
        .method("GET")
        .uri("/items")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
