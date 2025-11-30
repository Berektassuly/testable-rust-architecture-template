use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::app::{AppService, AppState};
use crate::domain::{AppError, CreateItemRequest, Item};

/// Handler for creating a new item.
///
/// This handler receives a JSON payload, delegates to the application service,
/// and returns the created item or an error response.
pub async fn create_item_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateItemRequest>,
) -> Result<Json<Item>, AppError> {
    let app_service = AppService::new();
    let item = app_service.create_and_submit_item(&state, &payload).await?;
    Ok(Json(item))
}

/// Handler for health check endpoint.
///
/// Returns a 200 OK status to indicate the service is running.
pub async fn health_check_handler() -> StatusCode {
    StatusCode::OK
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            // Configuration errors
            AppError::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::MissingEnvVar(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::InvalidConfigValue { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }

            // Database errors
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::DatabaseConnection(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::DuplicateRecord(_) => (StatusCode::CONFLICT, self.to_string()),

            // Blockchain errors
            AppError::Blockchain(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::BlockchainConnection(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            AppError::TransactionFailed(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::InvalidSignature(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::InsufficientFunds => (StatusCode::PAYMENT_REQUIRED, self.to_string()),

            // External service errors
            AppError::ExternalService(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::HttpRequest(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),

            // Validation errors
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::InvalidInput { .. } => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::InvalidHash(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            // Serialization errors
            AppError::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::Deserialization(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            // Authentication/Authorization errors
            AppError::Authentication(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Authorization(_) => (StatusCode::FORBIDDEN, self.to_string()),

            // Generic errors
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::NotSupported(_) => (StatusCode::NOT_IMPLEMENTED, self.to_string()),
        };

        let body = Json(serde_json::json!({
            "error": message
        }));

        (status, body).into_response()
    }
}