//! HTTP request handlers.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use tracing::error;

use crate::app::AppState;
use crate::domain::{
    AppError, BlockchainError, ConfigError, CreateItemRequest, DatabaseError, ExternalServiceError,
    HealthResponse, HealthStatus, Item, ValidationError,
};

pub async fn create_item_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateItemRequest>,
) -> Result<Json<Item>, AppError> {
    let item = state.service.create_and_submit_item(&payload).await?;
    Ok(Json(item))
}

pub async fn health_check_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let health = state.service.health_check().await;
    Json(health)
}

pub async fn liveness_handler() -> StatusCode {
    StatusCode::OK
}

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
        };

        if status.is_server_error() {
            error!(error_type = %error_type, message = %message, "Server error");
        }

        let body = Json(serde_json::json!({
            "error": {
                "type": error_type,
                "message": message,
            }
        }));

        (status, body).into_response()
    }
}
