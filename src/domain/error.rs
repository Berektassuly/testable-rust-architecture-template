//! Application error types with proper error chaining.
//!
//! This module provides a hierarchical error system that preserves
//! error context and enables proper error handling at each layer.

use thiserror::Error;

/// Database-specific errors.
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Query execution failed: {0}")]
    Query(String),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Duplicate record: {0}")]
    Duplicate(String),

    #[error("Pool exhausted: {0}")]
    PoolExhausted(String),

    #[error("Migration failed: {0}")]
    Migration(String),
}

/// Blockchain-specific errors.
#[derive(Error, Debug)]
pub enum BlockchainError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("RPC call failed: {0}")]
    RpcError(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Insufficient funds for transaction")]
    InsufficientFunds,

    #[error("Timeout waiting for confirmation: {0}")]
    Timeout(String),
}

/// Configuration-specific errors.
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Invalid value for '{key}': {message}")]
    InvalidValue { key: String, message: String },

    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Validation-specific errors.
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid field '{field}': {message}")]
    InvalidField { field: String, message: String },

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Validation failed: {0}")]
    Multiple(String),
}

/// External service errors.
#[derive(Error, Debug)]
pub enum ExternalServiceError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Service unavailable: {0}")]
    Unavailable(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),
}

/// Main application error type.
///
/// This enum aggregates all domain-specific errors and provides
/// a unified error handling interface for the application.
#[derive(Error, Debug)]
pub enum AppError {
    // Infrastructure errors
    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error(transparent)]
    Blockchain(#[from] BlockchainError),

    #[error(transparent)]
    ExternalService(#[from] ExternalServiceError),

    // Application errors
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Validation(#[from] ValidationError),

    // Authentication/Authorization
    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Authorization denied: {0}")]
    Authorization(String),

    // Serialization
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    // Generic
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

// Implement From traits for common error types

impl From<std::env::VarError> for AppError {
    fn from(err: std::env::VarError) -> Self {
        AppError::Config(ConfigError::MissingEnvVar(err.to_string()))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        if err.is_data() || err.is_syntax() || err.is_eof() {
            AppError::Deserialization(err.to_string())
        } else {
            AppError::Serialization(err.to_string())
        }
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(err: validator::ValidationErrors) -> Self {
        AppError::Validation(ValidationError::Multiple(err.to_string()))
    }
}

impl From<sqlx::Error> for DatabaseError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => DatabaseError::NotFound("Row not found".to_string()),
            sqlx::Error::PoolTimedOut => {
                DatabaseError::PoolExhausted("Connection pool timed out".to_string())
            }
            sqlx::Error::Database(db_err) => {
                if let Some(code) = db_err.code() {
                    if code == "23505" {
                        return DatabaseError::Duplicate(db_err.message().to_string());
                    }
                }
                DatabaseError::Query(db_err.message().to_string())
            }
            _ => DatabaseError::Query(err.to_string()),
        }
    }
}

/// Result type alias for convenience.
pub type AppResult<T> = Result<T, AppError>;

/// Extension trait for adding context to errors.
pub trait ResultExt<T> {
    /// Add context to an error.
    fn with_context<F, S>(self, f: F) -> Result<T, AppError>
    where
        F: FnOnce() -> S,
        S: Into<String>;
}

impl<T, E: Into<AppError>> ResultExt<T> for Result<T, E> {
    fn with_context<F, S>(self, f: F) -> Result<T, AppError>
    where
        F: FnOnce() -> S,
        S: Into<String>,
    {
        self.map_err(|e| {
            let app_error = e.into();
            // Log the context for debugging
            tracing::error!(context = %f().into(), error = ?app_error, "Error with context");
            app_error
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_display() {
        let error = DatabaseError::Connection("timeout".to_string());
        assert_eq!(error.to_string(), "Connection failed: timeout");
    }

    #[test]
    fn test_blockchain_error_display() {
        let error = BlockchainError::InsufficientFunds;
        assert_eq!(
            error.to_string(),
            "Insufficient funds for transaction"
        );
    }

    #[test]
    fn test_nested_error_conversion() {
        let db_error = DatabaseError::NotFound("user 123".to_string());
        let app_error: AppError = db_error.into();

        assert!(matches!(app_error, AppError::Database(DatabaseError::NotFound(_))));
    }

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError::InvalidField {
            field: "email".to_string(),
            message: "invalid format".to_string(),
        };
        assert_eq!(error.to_string(), "Invalid field 'email': invalid format");
    }

    #[test]
    fn test_config_error_display() {
        let error = ConfigError::InvalidValue {
            key: "port".to_string(),
            message: "must be positive".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Invalid value for 'port': must be positive"
        );
    }

    #[test]
    fn test_app_result_type_alias() {
        fn returns_ok() -> AppResult<i32> {
            Ok(42)
        }

        fn returns_err() -> AppResult<i32> {
            Err(AppError::Internal("test error".to_string()))
        }

        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }
}
