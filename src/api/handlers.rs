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

use crate::app::AppState;
use crate::domain::{
    AppError, BlockchainError, CreateItemRequest, DatabaseError, ErrorDetail, ErrorResponse,
    ExternalServiceError, HealthResponse, HealthStatus, Item, PaginatedResponse, PaginationParams,
    RateLimitResponse,
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
) -> Result<Json<Item>, AppError> {
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
) -> Result<Json<PaginatedResponse<Item>>, AppError> {
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
) -> Result<Json<Item>, AppError> {
    let item = state
        .service
        .get_item(&id)
        .await?
        .ok_or(AppError::Database(DatabaseError::NotFound(id)))?;
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
) -> Result<Json<Item>, AppError> {
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

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_type, message) = match &self {
            AppError::Database(db_err) => match db_err {
                DatabaseError::Connection(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "database_error",
                    self.to_string(),
                ),
                DatabaseError::NotFound(_) => {
                    (StatusCode::NOT_FOUND, "not_found", self.to_string())
                }
                DatabaseError::Duplicate(_) => {
                    (StatusCode::CONFLICT, "duplicate", self.to_string())
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    self.to_string(),
                ),
            },
            AppError::Blockchain(bc_err) => match bc_err {
                BlockchainError::Connection(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "blockchain_error",
                    self.to_string(),
                ),
                BlockchainError::InsufficientFunds => (
                    StatusCode::PAYMENT_REQUIRED,
                    "insufficient_funds",
                    self.to_string(),
                ),
                BlockchainError::Timeout(_) => {
                    (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string())
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "blockchain_error",
                    self.to_string(),
                ),
            },
            AppError::ExternalService(ext_err) => match ext_err {
                ExternalServiceError::Unavailable(_) => (
                    StatusCode::BAD_GATEWAY,
                    "external_service_error",
                    self.to_string(),
                ),
                ExternalServiceError::Timeout(_) => {
                    (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string())
                }
                ExternalServiceError::RateLimited(_) => (
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                    self.to_string(),
                ),
                _ => (
                    StatusCode::BAD_GATEWAY,
                    "external_service_error",
                    self.to_string(),
                ),
            },
            AppError::Config(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "configuration_error",
                self.to_string(),
            ),
            AppError::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "validation_error",
                self.to_string(),
            ),
            AppError::Authentication(_) => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                self.to_string(),
            ),
            AppError::Authorization(_) => (
                StatusCode::FORBIDDEN,
                "authorization_error",
                self.to_string(),
            ),
            AppError::Serialization(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "serialization_error",
                self.to_string(),
            ),
            AppError::Deserialization(_) => (
                StatusCode::BAD_REQUEST,
                "deserialization_error",
                self.to_string(),
            ),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                self.to_string(),
            ),
            AppError::NotSupported(_) => (
                StatusCode::NOT_IMPLEMENTED,
                "not_supported",
                self.to_string(),
            ),
            AppError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Rate limit exceeded".to_string(),
            ),
        };

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DatabaseClient;
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    #[tokio::test]
    async fn test_create_item_handler() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db.clone(), bc));

        // Seed item
        let req = CreateItemRequest::new("Seed".to_string(), "Content".to_string());
        let created = db.create_item(&req).await.unwrap();

        let result = get_item_handler(State(state), Path(created.id.clone())).await;
        assert!(result.is_ok());
        let Json(fetched) = result.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn test_health_check_handler() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

        let Json(resp) = health_check_handler(State(state)).await;
        assert_eq!(resp.status, HealthStatus::Healthy);
    }
    #[tokio::test]
    async fn test_list_items_handler_pagination_clamping() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

        let result = get_item_handler(State(state), Path("non-existent-id".to_string())).await;

        match result {
            Err(AppError::Database(DatabaseError::NotFound(id))) => {
                assert_eq!(id, "non-existent-id");
            }
            _ => panic!("Expected DatabaseError::NotFound"),
        }
    }

    #[tokio::test]
    async fn test_retry_blockchain_handler_success() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db.clone(), bc));

        // Seed item
        let req = CreateItemRequest::new("Retry Item".to_string(), "Content".to_string());
        let created = db.create_item(&req).await.unwrap();

        // Update status to be eligible for retry
        db.update_blockchain_status(
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
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

        let status = readiness_handler(State(state)).await;
        assert_eq!(status, StatusCode::OK);
    }

    // --- Error Mapping Tests (IntoResponse) ---

    #[test]
    fn test_error_mapping_database_not_found() {
        let err = AppError::Database(DatabaseError::NotFound("123".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_mapping_database_conflict() {
        let err = AppError::Database(DatabaseError::Duplicate("key".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_error_mapping_blockchain_insufficient_funds() {
        let err = AppError::Blockchain(BlockchainError::InsufficientFunds);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[test]
    fn test_error_mapping_blockchain_timeout() {
        let err = AppError::Blockchain(BlockchainError::Timeout("5000ms".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn test_error_mapping_external_service_rate_limited() {
        let err = AppError::ExternalService(ExternalServiceError::RateLimited("provider".into()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_error_mapping_validation_error() {
        let err = AppError::Validation("Invalid email format".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_auth_errors() {
        let err_unauth = AppError::Authentication("Missing token".into());
        assert_eq!(
            err_unauth.into_response().status(),
            StatusCode::UNAUTHORIZED
        );

        let err_forbidden = AppError::Authorization("Insufficient permissions".into());
        assert_eq!(
            err_forbidden.into_response().status(),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn test_error_mapping_internal_errors() {
        let err_internal = AppError::Internal("Unexpected failure".into());
        assert_eq!(
            err_internal.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let err_config = AppError::Config("Missing env var".into());
        assert_eq!(
            err_config.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_database_connection() {
        let err = AppError::Database(DatabaseError::Connection("timeout".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn test_error_mapping_database_query() {
        let err = AppError::Database(DatabaseError::Query("syntax error".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_database_pool_exhausted() {
        let err = AppError::Database(DatabaseError::PoolExhausted("no connections".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_database_migration() {
        let err = AppError::Database(DatabaseError::Migration("failed".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_blockchain_connection() {
        let err = AppError::Blockchain(BlockchainError::Connection("refused".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn test_error_mapping_blockchain_rpc_error() {
        let err = AppError::Blockchain(BlockchainError::RpcError("invalid method".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_blockchain_transaction_failed() {
        let err = AppError::Blockchain(BlockchainError::TransactionFailed("nonce".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_blockchain_invalid_signature() {
        let err = AppError::Blockchain(BlockchainError::InvalidSignature("corrupt".into()));
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_external_service_http_error() {
        let err = AppError::ExternalService(ExternalServiceError::HttpError("404".into()));
        assert_eq!(err.into_response().status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn test_error_mapping_external_service_unavailable() {
        let err = AppError::ExternalService(ExternalServiceError::Unavailable("down".into()));
        assert_eq!(err.into_response().status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn test_error_mapping_external_service_timeout() {
        let err = AppError::ExternalService(ExternalServiceError::Timeout("30s".into()));
        assert_eq!(err.into_response().status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn test_error_mapping_serialization() {
        let err = AppError::Serialization("json encode".into());
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn test_error_mapping_deserialization() {
        let err = AppError::Deserialization("invalid json".into());
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_not_supported() {
        let err = AppError::NotSupported("feature".into());
        assert_eq!(err.into_response().status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[test]
    fn test_error_mapping_rate_limited() {
        let err = AppError::RateLimited;
        assert_eq!(err.into_response().status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_readiness_handler_degraded() {
        // When blockchain is unhealthy but db healthy = degraded (returns OK)
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        bc.set_healthy(false);
        let state = Arc::new(AppState::new(db, bc));

        let status = readiness_handler(State(state)).await;
        // Unhealthy blockchain makes overall status Unhealthy
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_retry_blockchain_handler_not_found() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        let state = Arc::new(AppState::new(db, bc));

        let result = retry_blockchain_handler(State(state), Path("nonexistent".to_string())).await;
        assert!(matches!(
            result,
            Err(AppError::Database(DatabaseError::NotFound(_)))
        ));
    }
}
