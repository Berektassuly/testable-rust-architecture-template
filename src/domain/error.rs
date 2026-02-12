//! Domain error types: context-specific, idiomatic error handling.
//! Infrastructure details (SQLx, Reqwest) are mapped to semantic variants and do not leak.

use thiserror::Error;

/// Item-related business logic and repository errors.
#[derive(Error, Debug, Clone)]
pub enum ItemError {
    #[error("Item not found: {0}")]
    NotFound(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Repository operation failed")]
    RepositoryFailure,
}

/// Blockchain / chain interaction errors.
#[derive(Error, Debug, Clone)]
pub enum BlockchainError {
    #[error("Transaction submission failed: {0}")]
    SubmissionFailed(String),
    /// Submission failed but the blockhash used is provided for sticky retry
    #[error("Transaction submission failed: {message} (blockhash_used: {blockhash_used})")]
    SubmissionFailedWithBlockhash {
        message: String,
        blockhash_used: String,
    },
    #[error("Blockhash expired or invalid")]
    BlockhashExpired,
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Insufficient funds for transaction")]
    InsufficientFunds,
    #[error("Timeout: {0}")]
    Timeout(String),
}

/// System health check errors.
#[derive(Error, Debug, Clone)]
pub enum HealthCheckError {
    #[error("Database unavailable")]
    DatabaseUnavailable,
    #[error("Blockchain unavailable")]
    BlockchainUnavailable,
}

#[derive(Error, Debug, Clone)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    #[error("Invalid value for '{key}': {message}")]
    InvalidValue { key: String, message: String },
    #[error("Parse error: {0}")]
    ParseError(String),
}

impl From<&str> for ConfigError {
    fn from(s: &str) -> Self {
        ConfigError::ParseError(s.to_string())
    }
}

#[derive(Error, Debug, Clone)]
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

impl From<&str> for ValidationError {
    fn from(s: &str) -> Self {
        ValidationError::InvalidFormat(s.to_string())
    }
}

impl From<validator::ValidationErrors> for ValidationError {
    fn from(err: validator::ValidationErrors) -> Self {
        ValidationError::Multiple(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_error_display() {
        let err = ItemError::NotFound("id".to_string());
        assert_eq!(err.to_string(), "Item not found: id");
        let err = ItemError::InvalidState("not eligible".to_string());
        assert_eq!(err.to_string(), "Invalid state: not eligible");
        let err = ItemError::RepositoryFailure;
        assert_eq!(err.to_string(), "Repository operation failed");
    }

    #[test]
    fn test_blockchain_error_display() {
        let err = BlockchainError::SubmissionFailed("rpc error".to_string());
        assert!(err.to_string().contains("Transaction submission failed"));
        let err = BlockchainError::NetworkError("timeout".to_string());
        assert!(err.to_string().contains("Network error"));
        let err = BlockchainError::InsufficientFunds;
        assert_eq!(err.to_string(), "Insufficient funds for transaction");
        let err = BlockchainError::Timeout("30s".to_string());
        assert!(err.to_string().contains("Timeout"));
    }

    #[test]
    fn test_health_check_error_display() {
        let err = HealthCheckError::DatabaseUnavailable;
        assert_eq!(err.to_string(), "Database unavailable");
        let err = HealthCheckError::BlockchainUnavailable;
        assert_eq!(err.to_string(), "Blockchain unavailable");
    }

    #[test]
    fn test_config_error_from_str() {
        let err: ConfigError = "parse failure".into();
        assert!(matches!(err, ConfigError::ParseError(msg) if msg == "parse failure"));
    }

    #[test]
    fn test_validation_error_from_str() {
        let err: ValidationError = "invalid format".into();
        assert!(matches!(err, ValidationError::InvalidFormat(msg) if msg == "invalid format"));
    }

    #[test]
    fn test_validation_error_from_validator() {
        use validator::Validate;

        #[derive(Validate)]
        struct TestStruct {
            #[validate(length(min = 1))]
            val: String,
        }

        let s = TestStruct {
            val: "".to_string(),
        };
        let err = s.validate().unwrap_err();
        let val_err = ValidationError::from(err);
        assert!(matches!(val_err, ValidationError::Multiple(_)));
    }
}
