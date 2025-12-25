//! HTTP request handlers.
//!
//! This module contains all Axum request handlers that process
//! incoming HTTP requests and return responses.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use tracing::error;

use crate::app::AppState;
use crate::domain::{
    AppError, BlockchainError, ConfigError, CreateItemRequest, DatabaseError,
    ExternalServiceError, HealthResponse, Item, ValidationError,
};

/// Handler for creating a new item.
///
/// This handler receives a JSON payload, delegates to the application service,
/// and returns the created item or an error response.
///
/// # Endpoint
///
/// `POST /items`
///
/// # Request Body
///
/// ```json
/// {
///     "name": "Item Name",
///     "content": "Item content",
///     "description": "Optional description",
///     "metadata": {
///         "author": "Optional author",
///         "version": "Optional version",
///         "tags": ["tag1", "tag2"],
///         "custom_fields": {}
///     }
/// }
/// ```
///
/// # Response
///
/// Returns the created item with generated ID, hash, and timestamps.
pub async fn create_item_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateItemRequest>,
) -> Result<Json<Item>, AppError> {
    let item = state.service.create_and_submit_item(&payload).await?;
    Ok(Json(item))
}

/// Handler for health check endpoint.
///
/// Returns detailed health status of all application dependencies.
///
/// # Endpoint
///
/// `GET /health`
///
/// # Response
///
/// Returns health status for database and blockchain connections.
pub async fn health_check_handler(
    State(state): State<Arc<AppState>>,
) -> Json<HealthResponse> {
    let health = state.service.health_check().await;
    Json(health)
}

/// Handler for liveness probe.
///
/// Simple endpoint that returns 200 OK if the service is running.
/// Used by Kubernetes or other orchestrators for liveness checks.
///
/// # Endpoint
///
/// `GET /health/live`
pub async fn liveness_handler() -> StatusCode {
    StatusCode::OK
}

/// Handler for readiness probe.
///
/// Checks if the service is ready to accept traffic by verifying
/// all dependencies are healthy.
///
/// # Endpoint
///
/// `GET /health/ready`
pub async fn readiness_handler(
    State(state): State<Arc<AppState>>,
) -> StatusCode {
    let health = state.service.health_check().await;
    
    match health.status {
        crate::domain::HealthStatus::Healthy => StatusCode::OK,
        crate::domain::HealthStatus::Degraded => StatusCode::OK, // Still accepting traffic
        crate::domain::HealthStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Converts `AppError` to HTTP response.
///
/// Maps application errors to appropriate HTTP status codes and
/// returns a JSON error response body.
impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_type, message) = match &self {
            // Database errors
            AppError::Database(db_err) => match db_err {
                DatabaseError::Connection(_) => {
                    (StatusCode::SERVICE_UNAVAILABLE, "database_error", self.to_string())
                }
                DatabaseError::NotFound(_) => {
                    (StatusCode::NOT_FOUND, "not_found", self.to_string())
                }
                DatabaseError::Duplicate(_) => {
                    (StatusCode::CONFLICT, "duplicate", self.to_string())
                }
                DatabaseError::PoolExhausted(_) => {
                    (StatusCode::SERVICE_UNAVAILABLE, "database_error", self.to_string())
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "database_error", self.to_string()),
            },

            // Blockchain errors
            AppError::Blockchain(bc_err) => match bc_err {
                BlockchainError::Connection(_) => {
                    (StatusCode::SERVICE_UNAVAILABLE, "blockchain_error", self.to_string())
                }
                BlockchainError::InsufficientFunds => {
                    (StatusCode::PAYMENT_REQUIRED, "insufficient_funds", self.to_string())
                }
                BlockchainError::InvalidSignature(_) => {
                    (StatusCode::BAD_REQUEST, "invalid_signature", self.to_string())
                }
                BlockchainError::Timeout(_) => {
                    (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string())
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "blockchain_error", self.to_string()),
            },

            // External service errors
            AppError::ExternalService(ext_err) => match ext_err {
                ExternalServiceError::Unavailable(_) => {
                    (StatusCode::BAD_GATEWAY, "external_service_error", self.to_string())
                }
                ExternalServiceError::Timeout(_) => {
                    (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string())
                }
                ExternalServiceError::RateLimited(_) => {
                    (StatusCode::TOO_MANY_REQUESTS, "rate_limited", self.to_string())
                }
                _ => (StatusCode::BAD_GATEWAY, "external_service_error", self.to_string()),
            },

            // Configuration errors
            AppError::Config(cfg_err) => match cfg_err {
                ConfigError::MissingEnvVar(_) | ConfigError::InvalidValue { .. } => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "configuration_error", self.to_string())
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "configuration_error", self.to_string()),
            },

            // Validation errors
            AppError::Validation(val_err) => match val_err {
                ValidationError::InvalidField { .. } | ValidationError::MissingField(_) => {
                    (StatusCode::BAD_REQUEST, "validation_error", self.to_string())
                }
                _ => (StatusCode::BAD_REQUEST, "validation_error", self.to_string()),
            },

            // Auth errors
            AppError::Authentication(_) => {
                (StatusCode::UNAUTHORIZED, "authentication_error", self.to_string())
            }
            AppError::Authorization(_) => {
                (StatusCode::FORBIDDEN, "authorization_error", self.to_string())
            }

            // Serialization errors
            AppError::Serialization(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "serialization_error", self.to_string())
            }
            AppError::Deserialization(_) => {
                (StatusCode::BAD_REQUEST, "deserialization_error", self.to_string())
            }

            // Generic errors
            AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", self.to_string())
            }
            AppError::NotSupported(_) => {
                (StatusCode::NOT_IMPLEMENTED, "not_supported", self.to_string())
            }
        };

        // Log server errors
        if status.is_server_error() {
            error!(
                error_type = %error_type,
                status = %status,
                message = %message,
                "Server error occurred"
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::api::create_router;
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    fn create_test_state() -> Arc<AppState> {
        let db = Arc::new(MockDatabaseClient::new());
        let blockchain = Arc::new(MockBlockchainClient::new());
        Arc::new(AppState::new(db, blockchain))
    }

    #[tokio::test]
    async fn test_create_item_handler_success() {
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
    async fn test_create_item_handler_validation_error() {
        let state = create_test_state();
        let router = create_router(state);

        // Empty name should fail validation
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
    async fn test_health_check_handler() {
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
        assert_eq!(health.status, crate::domain::HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_liveness_handler() {
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
    async fn test_readiness_handler_healthy() {
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
    async fn test_readiness_handler_unhealthy() {
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
    async fn test_error_response_format() {
        let error = AppError::Validation(ValidationError::InvalidField {
            field: "name".to_string(),
            message: "too short".to_string(),
        });

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert!(body["error"]["type"].is_string());
        assert!(body["error"]["message"].is_string());
    }
}
