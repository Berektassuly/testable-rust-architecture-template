use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    // Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Invalid configuration value for '{key}': {message}")]
    InvalidConfigValue { key: String, message: String },

    // Infrastructure errors - Database
    #[error("Database error: {0}")]
    Database(String),

    #[error("Database connection failed: {0}")]
    DatabaseConnection(String),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Duplicate record: {0}")]
    DuplicateRecord(String),

    // Infrastructure errors - Blockchain
    #[error("Blockchain error: {0}")]
    Blockchain(String),

    #[error("Blockchain connection failed: {0}")]
    BlockchainConnection(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Invalid transaction signature: {0}")]
    InvalidSignature(String),

    #[error("Insufficient funds for transaction")]
    InsufficientFunds,

    // Infrastructure errors - External services
    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("HTTP request failed: {0}")]
    HttpRequest(String),

    #[error("Service timeout: {0}")]
    Timeout(String),

    // Validation errors
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Invalid input: {field} - {message}")]
    InvalidInput { field: String, message: String },

    #[error("Invalid hash format: {0}")]
    InvalidHash(String),

    // Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    // Authentication/Authorization errors
    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Authorization denied: {0}")]
    Authorization(String),

    // Generic errors
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

// Implement From traits for common error types

impl From<std::env::VarError> for AppError {
    fn from(err: std::env::VarError) -> Self {
        AppError::MissingEnvVar(err.to_string())
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

// Result type alias for convenience
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        let error = AppError::Config("invalid port number".to_string());
        assert_eq!(error.to_string(), "Configuration error: invalid port number");
    }

    #[test]
    fn test_missing_env_var_display() {
        let error = AppError::MissingEnvVar("DATABASE_URL".to_string());
        assert_eq!(error.to_string(), "Missing environment variable: DATABASE_URL");
    }

    #[test]
    fn test_invalid_config_value_display() {
        let error = AppError::InvalidConfigValue {
            key: "port".to_string(),
            message: "must be a positive integer".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Invalid configuration value for 'port': must be a positive integer"
        );
    }

    #[test]
    fn test_database_error_display() {
        let error = AppError::Database("connection pool exhausted".to_string());
        assert_eq!(error.to_string(), "Database error: connection pool exhausted");
    }

    #[test]
    fn test_blockchain_error_display() {
        let error = AppError::Blockchain("RPC node unavailable".to_string());
        assert_eq!(error.to_string(), "Blockchain error: RPC node unavailable");
    }

    #[test]
    fn test_not_found_error_display() {
        let error = AppError::NotFound("record with id 123".to_string());
        assert_eq!(error.to_string(), "Record not found: record with id 123");
    }

    #[test]
    fn test_validation_error_display() {
        let error = AppError::InvalidInput {
            field: "email".to_string(),
            message: "invalid format".to_string(),
        };
        assert_eq!(error.to_string(), "Invalid input: email - invalid format");
    }

    #[test]
    fn test_from_env_var_error() {
        let env_error = std::env::VarError::NotPresent;
        let app_error: AppError = env_error.into();
        assert!(matches!(app_error, AppError::MissingEnvVar(_)));
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