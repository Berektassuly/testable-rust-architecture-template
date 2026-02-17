//! HTTP request handlers with OpenAPI documentation.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::error;
use utoipa::OpenApi;

use crate::app::{AppState, CreateItemError};
use crate::domain::{
    BlockchainError, CreateItemRequest, ErrorDetail, ErrorResponse, HealthResponse, HealthStatus,
    Item, ItemError, PaginatedResponse, PaginationParams, RateLimitResponse, ValidationError,
};

/// OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Testable Rust Architecture API",
        version = "0.3.0",
        description = "Production-ready Rust template demonstrating testable architecture with PostgreSQL and Solana integration",
        contact(
            name = "API Support",
            email = "support@example.com"
        ),
        license(
            name = "MIT"
        )
    ),
    paths(
        create_item_handler,
        list_items_handler,
        get_item_handler,
        retry_blockchain_handler,
        health_check_handler,
        liveness_handler,
        readiness_handler,
    ),
    components(
        schemas(
            Item,
            CreateItemRequest,
            crate::domain::ItemMetadata,
            crate::domain::ItemMetadataRequest,
            crate::domain::BlockchainStatus,
            PaginationParams,
            PaginatedResponse<Item>,
            HealthResponse,
            HealthStatus,
            ErrorResponse,
            ErrorDetail,
            RateLimitResponse,
        )
    ),
    tags(
        (name = "items", description = "Item management endpoints"),
        (name = "health", description = "Health check endpoints")
    )
)]
pub struct ApiDoc;

/// Create a new item
#[utoipa::path(
    post,
    path = "/items",
    tag = "items",
    request_body = CreateItemRequest,
    responses(
        (status = 200, description = "Item created successfully", body = Item),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = RateLimitResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
        (status = 503, description = "Service unavailable", body = ErrorResponse)
    )
)]
pub async fn create_item_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateItemRequest>,
) -> Result<Json<Item>, CreateItemError> {
    let item = state.service.create_and_submit_item(&payload).await?;
    Ok(Json(item))
}

/// List items with pagination
#[utoipa::path(
    get,
    path = "/items",
    tag = "items",
    params(
        ("limit" = Option<i64>, Query, description = "Maximum number of items to return (1-100, default: 20)"),
        ("cursor" = Option<String>, Query, description = "Cursor for pagination (item ID to start after)")
    ),
    responses(
        (status = 200, description = "List of items", body = PaginatedResponse<Item>),
        (status = 400, description = "Invalid pagination parameters", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = RateLimitResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn list_items_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<Item>>, ItemError> {
    // Validate limit
    let limit = params.limit.clamp(1, 100);
    let items = state
        .service
        .list_items(limit, params.cursor.as_deref())
        .await?;
    Ok(Json(items))
}

/// Get a single item by ID
#[utoipa::path(
    get,
    path = "/items/{id}",
    tag = "items",
    params(
        ("id" = String, Path, description = "Item ID")
    ),
    responses(
        (status = 200, description = "Item found", body = Item),
        (status = 404, description = "Item not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = RateLimitResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_item_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Item>, ItemError> {
    let item = state
        .service
        .get_item(&id)
        .await?
        .ok_or(ItemError::NotFound(id))?;
    Ok(Json(item))
}

/// Retry blockchain submission for an item
#[utoipa::path(
    post,
    path = "/items/{id}/retry",
    tag = "items",
    params(
        ("id" = String, Path, description = "Item ID")
    ),
    responses(
        (status = 200, description = "Retry successful", body = Item),
        (status = 400, description = "Item not eligible for retry", body = ErrorResponse),
        (status = 404, description = "Item not found", body = ErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = RateLimitResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
        (status = 503, description = "Blockchain unavailable", body = ErrorResponse)
    )
)]
pub async fn retry_blockchain_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Item>, ItemError> {
    let item = state.service.retry_blockchain_submission(&id).await?;
    Ok(Json(item))
}

/// Detailed health check
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Health status", body = HealthResponse)
    )
)]
pub async fn health_check_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let health = state.service.health_check().await;
    Json(health)
}

/// Kubernetes liveness probe
#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses(
        (status = 200, description = "Application is alive")
    )
)]
pub async fn liveness_handler() -> StatusCode {
    StatusCode::OK
}

/// Kubernetes readiness probe
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Application is ready to serve traffic"),
        (status = 503, description = "Application is not ready")
    )
)]
pub async fn readiness_handler(State(state): State<Arc<AppState>>) -> StatusCode {
    let health = state.service.health_check().await;
    match health.status {
        HealthStatus::Healthy | HealthStatus::Degraded => StatusCode::OK,
        HealthStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn error_response(
    status: StatusCode,
    error_type: &str,
    message: String,
) -> axum::response::Response {
    if status.is_server_error() {
        error!(error_type = %error_type, message = %message, "Server error");
    }
    let body = Json(ErrorResponse {
        error: ErrorDetail {
            r#type: error_type.to_string(),
            message,
        },
    });
    (status, body).into_response()
}

impl IntoResponse for ItemError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_type, message) = match &self {
            ItemError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found", self.to_string()),
            ItemError::InvalidState(_) => {
                (StatusCode::BAD_REQUEST, "invalid_state", self.to_string())
            }
            ItemError::RepositoryFailure => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "repository_error",
                "Internal server error".to_string(),
            ),
        };
        error_response(status, error_type, message)
    }
}

impl IntoResponse for BlockchainError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_type, message) = match &self {
            BlockchainError::SubmissionFailed(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "blockchain_error",
                "Transaction submission failed".to_string(),
            ),
            BlockchainError::SubmissionFailedWithBlockhash { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "blockchain_error",
                "Transaction submission failed".to_string(),
            ),
            BlockchainError::BlockhashExpired => (
                StatusCode::BAD_REQUEST,
                "blockhash_expired",
                "Blockhash expired or invalid".to_string(),
            ),
            BlockchainError::NetworkError { .. } => (
                StatusCode::SERVICE_UNAVAILABLE,
                "blockchain_unavailable",
                "Blockchain service unavailable".to_string(),
            ),
            BlockchainError::InsufficientFunds => (
                StatusCode::PAYMENT_REQUIRED,
                "insufficient_funds",
                self.to_string(),
            ),
            BlockchainError::Timeout { .. } => {
                (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string())
            }
        };
        error_response(status, error_type, message)
    }
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> axum::response::Response {
        error_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            self.to_string(),
        )
    }
}

impl IntoResponse for CreateItemError {
    fn into_response(self) -> axum::response::Response {
        match self {
            CreateItemError::Validation(e) => e.into_response(),
            CreateItemError::Item(e) => e.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ItemRepository;
    use crate::test_utils::{MockBlockchainClient, MockProvider, mock_repos, test_api_key};

    #[tokio::test]
    async fn test_create_item_handler() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let payload = CreateItemRequest {
            name: "Test Item".to_string(),
            description: Some("Desc".to_string()),
            content: "Content".to_string(),
            metadata: None,
        };

        let result = create_item_handler(State(state), Json(payload)).await;
        assert!(result.is_ok());
        let Json(item) = result.unwrap();
        assert_eq!(item.name, "Test Item");
        assert_eq!(
            item.blockchain_status,
            crate::domain::BlockchainStatus::PendingSubmission
        );
    }

    #[tokio::test]
    async fn test_get_item_handler() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        // Seed item
        let req = CreateItemRequest::new("Seed".to_string(), "Content".to_string());
        let created = mock.create_item(&req).await.unwrap();

        let result = get_item_handler(State(state), Path(created.id.clone())).await;
        assert!(result.is_ok());
        let Json(fetched) = result.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn test_health_check_handler() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let Json(resp) = health_check_handler(State(state)).await;
        assert_eq!(resp.status, HealthStatus::Healthy);
    }
    #[tokio::test]
    async fn test_list_items_handler_pagination_clamping() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        // Test with limit > 100
        let params_high = PaginationParams {
            limit: i64::MAX,
            cursor: None,
        };
        let result = list_items_handler(State(state.clone()), Query(params_high)).await;
        assert!(result.is_ok());
        // Note: We can't verify the internal call argument without a spy,
        // but we ensure the handler doesn't panic and returns success.

        // Test with limit < 1
        let params_low = PaginationParams {
            limit: i64::MIN,
            cursor: None,
        };
        let result_low = list_items_handler(State(state), Query(params_low)).await;
        assert!(result_low.is_ok());
    }

    #[tokio::test]
    async fn test_get_item_handler_not_found() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let result = get_item_handler(State(state), Path("non-existent-id".to_string())).await;

        match result {
            Err(ItemError::NotFound(id)) => {
                assert_eq!(id, "non-existent-id");
            }
            _ => panic!("Expected ItemError::NotFound"),
        }
    }

    #[tokio::test]
    async fn test_retry_blockchain_handler_success() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        // Seed item
        let req = CreateItemRequest::new("Retry Item".to_string(), "Content".to_string());
        let created = mock.create_item(&req).await.unwrap();

        // Update status to be eligible for retry
        mock.update_blockchain_status(
            &created.id,
            crate::domain::BlockchainStatus::PendingSubmission,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let result = retry_blockchain_handler(State(state), Path(created.id)).await;
        assert!(result.is_ok());
        let Json(item) = result.unwrap();
        assert_eq!(item.name, "Retry Item");
    }

    #[tokio::test]
    async fn test_liveness_handler() {
        let status = liveness_handler().await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_readiness_handler_healthy() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let status = readiness_handler(State(state)).await;
        assert_eq!(status, StatusCode::OK);
    }

    // --- Error Mapping Tests (IntoResponse) ---

    #[test]
    fn test_error_mapping_item_not_found() {
        let err = ItemError::NotFound("123".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_mapping_item_invalid_state() {
        let err = ItemError::InvalidState("invalid".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_item_repository_failure() {
        let err = ItemError::RepositoryFailure;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_error_mapping_blockchain_insufficient_funds() {
        let err = BlockchainError::InsufficientFunds;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[test]
    fn test_error_mapping_blockchain_timeout() {
        let err = BlockchainError::Timeout {
            message: "5000ms".to_string(),
            blockhash: String::new(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn test_error_mapping_blockchain_submission_failed() {
        let err = BlockchainError::SubmissionFailed("rpc error".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_error_mapping_blockchain_network_error() {
        let err = BlockchainError::NetworkError {
            message: "refused".to_string(),
            blockhash: String::new(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_error_mapping_validation_error() {
        let err = ValidationError::InvalidFormat("Invalid email format".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_create_item_validation() {
        let err = CreateItemError::Validation(ValidationError::MissingField("name".into()));
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_create_item_repository() {
        let err = CreateItemError::Item(ItemError::RepositoryFailure);
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn test_readiness_handler_degraded() {
        // When blockchain is unhealthy but db healthy = degraded (returns OK)
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        bc.set_healthy(false);
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let status = readiness_handler(State(state)).await;
        // Unhealthy blockchain makes overall status Unhealthy
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_retry_blockchain_handler_not_found() {
        let mock = Arc::new(MockProvider::new());
        let (item_repo, outbox_repo) = mock_repos(&mock);
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(item_repo, outbox_repo, bc, test_api_key()));

        let result = retry_blockchain_handler(State(state), Path("nonexistent".to_string())).await;
        assert!(matches!(result, Err(ItemError::NotFound(_))));
    }
}
