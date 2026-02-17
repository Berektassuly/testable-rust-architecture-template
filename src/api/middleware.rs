//! HTTP middleware for API layer.

use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::warn;

use crate::app::AppState;

/// Constant-time comparison of two byte slices to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// API key authentication middleware.
/// Protects POST endpoints by requiring a valid `x-api-key` header.
/// GET requests pass through without authentication.
/// Uses constant-time comparison (via SHA-256 digest) to prevent timing attacks.
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Only protect POST requests
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }

    let api_key_header = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());

    let Some(provided) = api_key_header else {
        warn!("API auth failed: missing x-api-key header");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };

    let expected = state.api_auth_key.expose_secret().as_bytes();
    let provided_bytes = provided.as_bytes();

    // Compare via SHA-256 digests for constant-time comparison (prevents timing attacks)
    let expected_hash = Sha256::digest(expected);
    let provided_hash = Sha256::digest(provided_bytes);

    if !constant_time_eq(expected_hash.as_slice(), provided_hash.as_slice()) {
        warn!("API auth failed: invalid x-api-key");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    next.run(request).await
}
