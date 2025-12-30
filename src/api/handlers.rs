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
