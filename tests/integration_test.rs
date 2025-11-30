use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use http_body_util::BodyExt;
use tower::ServiceExt;

use testable_rust_architecture_template::api::create_router;
use testable_rust_architecture_template::app::AppState;
use testable_rust_architecture_template::domain::{
    AppError, BlockchainClient, CreateItemRequest, DatabaseClient, Item,
};

/// Mock database client for integration testing.
struct MockDatabaseClient {
    data: Arc<Mutex<HashMap<String, Item>>>,
}

impl MockDatabaseClient {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl DatabaseClient for MockDatabaseClient {
    async fn health_check(&self) -> Result<(), AppError> {
        Ok(())
    }

    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        let data = self.data.lock().unwrap();
        Ok(data.get(id).cloned())
    }

    async fn create_item(&self, request: &CreateItemRequest) -> Result<Item, AppError> {
        let id = format!("item_{}", uuid::Uuid::new_v4());
        let now = Utc::now();

        let item = Item {
            id: id.clone(),
            hash: format!("hash_{}", id),
            name: request.name.clone(),
            description: request.description.clone(),
            metadata: request.metadata.clone(),
            created_at: now,
            updated_at: now,
        };

        let mut data = self.data.lock().unwrap();
        data.insert(id, item.clone());

        Ok(item)
    }
}

/// Mock blockchain client for integration testing.
struct MockBlockchainClient;

impl MockBlockchainClient {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BlockchainClient for MockBlockchainClient {
    async fn health_check(&self) -> Result<(), AppError> {
        Ok(())
    }

    async fn submit_transaction(&self, _hash: &str) -> Result<String, AppError> {
        Ok("mock_signature".to_string())
    }
}

#[tokio::test]
async fn test_create_item_success_e2e() {
    // Arrange: Setup mock clients and application state
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());

    let app_state = AppState::new(mock_db, mock_blockchain);
    let router = create_router(Arc::new(app_state));

    // Act: Create and send the request
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

    // Assert: Verify the response
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let item: Item = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(item.name, "Test Item");
    assert_eq!(item.description, Some("A test item description".to_string()));
    assert!(item.id.starts_with("item_"));
    assert!(item.hash.starts_with("hash_"));
}

#[tokio::test]
async fn test_health_check_endpoint() {
    // Arrange: Setup mock clients and application state
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());

    let app_state = AppState::new(mock_db, mock_blockchain);
    let router = create_router(Arc::new(app_state));

    // Act: Send GET request to /health
    let request = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    // Assert: Verify the response
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_create_item_with_metadata() {
    // Arrange: Setup mock clients and application state
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());

    let app_state = AppState::new(mock_db, mock_blockchain);
    let router = create_router(Arc::new(app_state));

    // Act: Create request with metadata
    let mut custom_fields = HashMap::new();
    custom_fields.insert("key1".to_string(), "value1".to_string());

    let metadata = testable_rust_architecture_template::domain::types::ItemMetadata {
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

    // Assert: Verify the response
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

#[tokio::test]
async fn test_invalid_json_returns_error() {
    // Arrange: Setup mock clients and application state
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());

    let app_state = AppState::new(mock_db, mock_blockchain);
    let router = create_router(Arc::new(app_state));

    // Act: Send invalid JSON
    let request = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/json")
        .body(Body::from("{ invalid json }"))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    // Assert: Should return an error status
    assert!(response.status().is_client_error());
}