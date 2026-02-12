//! Additional integration tests for specific request flows.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use testable_rust_architecture_template::api::create_router;
use testable_rust_architecture_template::app::AppState;
use testable_rust_architecture_template::domain::{CreateItemRequest, Item, PaginatedResponse};
use testable_rust_architecture_template::test_utils::{
    MockBlockchainClient, MockProvider, mock_repos,
};

fn create_test_state() -> Arc<AppState> {
    let mock = Arc::new(MockProvider::new());
    let (item_repo, outbox_repo) = mock_repos(&mock);
    let blockchain = Arc::new(MockBlockchainClient::new());
    Arc::new(AppState::new(item_repo, outbox_repo, blockchain))
}

#[tokio::test]
async fn test_full_item_lifecycle_flow() {
    let state = create_test_state();
    let router = create_router(state);

    // 1. POST - Create Item
    let create_payload = CreateItemRequest::new(
        "Lifecycle Test Item".to_string(),
        "This is a test of the full item lifecycle.".to_string(),
    );

    let create_request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&create_payload).unwrap()))
        .unwrap();

    let create_response = router.clone().oneshot(create_request).await.unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);

    let body_bytes = create_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let created_item: Item = serde_json::from_slice(&body_bytes).unwrap();
    let item_id = created_item.id;
    assert_eq!(created_item.name, "Lifecycle Test Item");

    // 2. GET - Retrieve the created item by ID
    let get_request = Request::builder()
        .method("GET")
        .uri(format!("/items/{}", item_id))
        .body(Body::empty())
        .unwrap();

    let get_response = router.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let body_bytes = get_response.into_body().collect().await.unwrap().to_bytes();
    let retrieved_item: Item = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(retrieved_item.id, item_id);
    assert_eq!(retrieved_item.name, "Lifecycle Test Item");

    // 3. GET - List items and verify the new item is present
    let list_request = Request::builder()
        .method("GET")
        .uri("/items?limit=10")
        .body(Body::empty())
        .unwrap();

    let list_response = router.clone().oneshot(list_request).await.unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);

    let body_bytes = list_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let list_result: PaginatedResponse<Item> = serde_json::from_slice(&body_bytes).unwrap();
    assert!(list_result.items.iter().any(|i| i.id == item_id));
}

#[tokio::test]
async fn test_post_bad_request_validation() {
    let state = create_test_state();
    let router = create_router(state);

    // Missing required field "name" (by sending empty string which fails validation)
    let bad_payload = CreateItemRequest::new("".to_string(), "Some content".to_string());

    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&bad_payload).unwrap()))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
