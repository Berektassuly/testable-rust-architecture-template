//! Integration tests for the API.

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
    BlockchainStatus, CreateItemRequest, HealthResponse, HealthStatus, Item, PaginatedResponse,
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
    assert_eq!(item.blockchain_status, BlockchainStatus::Submitted);
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
async fn test_list_items_empty() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/items")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: PaginatedResponse<Item> = serde_json::from_slice(&body_bytes).unwrap();
    assert!(result.items.is_empty());
    assert!(!result.has_more);
    assert!(result.next_cursor.is_none());
}

#[tokio::test]
async fn test_list_items_with_pagination() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(
        Arc::clone(&db) as _,
        Arc::clone(&blockchain) as _,
    ));

    // Create some items
    for i in 0..5 {
        let payload = CreateItemRequest::new(format!("Item {}", i), "Content".to_string());
        state
            .service
            .create_and_submit_item(&payload)
            .await
            .unwrap();
    }

    let router = create_router(state);

    // Get first page
    let request = Request::builder()
        .method("GET")
        .uri("/items?limit=2")
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: PaginatedResponse<Item> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
    assert!(result.next_cursor.is_some());

    // Get second page
    let cursor = result.next_cursor.unwrap();
    let request = Request::builder()
        .method("GET")
        .uri(format!("/items?limit=2&cursor={}", cursor))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: PaginatedResponse<Item> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
}

#[tokio::test]
async fn test_get_item_success() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(
        Arc::clone(&db) as _,
        Arc::clone(&blockchain) as _,
    ));

    // Create an item
    let payload = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
    let created_item = state
        .service
        .create_and_submit_item(&payload)
        .await
        .unwrap();

    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/items/{}", created_item.id))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let item: Item = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(item.id, created_item.id);
}

#[tokio::test]
async fn test_get_item_not_found() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/items/nonexistent_id")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_graceful_degradation_blockchain_failure() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::failing("RPC error"));
    let state = Arc::new(AppState::new(Arc::clone(&db) as _, blockchain));
    let router = create_router(state);

    let payload = CreateItemRequest::new("Test".to_string(), "Content".to_string());

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

    // Item should be created but with pending_submission status
    assert_eq!(item.blockchain_status, BlockchainStatus::PendingSubmission);
    assert!(item.blockchain_last_error.is_some());
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
async fn test_swagger_ui_available() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/swagger-ui/")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    // Swagger UI redirects or returns 200
    assert!(response.status().is_success() || response.status().is_redirection());
}

#[tokio::test]
async fn test_openapi_spec_available() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("GET")
        .uri("/api-docs/openapi.json")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let spec: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(spec.get("openapi").is_some());
    assert!(spec.get("paths").is_some());
}

#[tokio::test]
async fn test_retry_handler_item_not_found() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("POST")
        .uri("/items/nonexistent_id/retry")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_retry_handler_not_eligible() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(
        Arc::clone(&db) as _,
        Arc::clone(&blockchain) as _,
    ));

    // Create an item with Submitted status (not eligible for retry)
    let payload = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
    let created_item = state
        .service
        .create_and_submit_item(&payload)
        .await
        .unwrap();

    let router = create_router(state);

    let request = Request::builder()
        .method("POST")
        .uri(format!("/items/{}/retry", created_item.id))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    // Item is already submitted, not eligible for retry
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_item_malformed_json() {
    let state = create_test_state();
    let router = create_router(state);

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from("{ invalid json }"))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_list_items_invalid_limit() {
    let state = create_test_state();
    let router = create_router(state);

    // Limit is clamped, so this should still work
    let request = Request::builder()
        .method("GET")
        .uri("/items?limit=999999")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_health_check_degraded() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    blockchain.set_healthy(false);
    let state = Arc::new(AppState::new(db, blockchain));
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
    assert_eq!(health.status, HealthStatus::Unhealthy);
    assert_eq!(health.database, HealthStatus::Healthy);
    assert_eq!(health.blockchain, HealthStatus::Unhealthy);
}

#[tokio::test]
async fn test_create_item_with_metadata() {
    let state = create_test_state();
    let router = create_router(state);

    let payload = serde_json::json!({
        "name": "Item with Metadata",
        "content": "Content here",
        "description": "A description",
        "metadata": {
            "author": "Test Author",
            "version": "1.0.0",
            "tags": ["tag1", "tag2"],
            "custom_fields": {"key": "value"}
        }
    });

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
}
