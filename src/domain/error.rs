//! Application error types with proper error chaining.

use thiserror::Error;

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

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    #[error("Invalid value for '{key}': {message}")]
    InvalidValue { key: String, message: String },
    #[error("Parse error: {0}")]
    ParseError(String),
}

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

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Blockchain(#[from] BlockchainError),
    #[error(transparent)]
    ExternalService(#[from] ExternalServiceError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("Authentication failed: {0}")]
    Authentication(String),
    #[error("Authorization denied: {0}")]
    Authorization(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Operation not supported: {0}")]
    NotSupported(String),
    #[error("Rate limit exceeded")]
    RateLimited,
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
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
            sqlx::Error::PoolTimedOut => DatabaseError::PoolExhausted("Pool timed out".to_string()),
            sqlx::Error::Database(db_err) => {
                if db_err.code().is_some_and(|code| code == "23505") {
                    return DatabaseError::Duplicate(db_err.message().to_string());
                }
                DatabaseError::Query(db_err.message().to_string())
            }
            _ => DatabaseError::Query(err.to_string()),
        }
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        AppError::Database(DatabaseError::Migration(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_conversions() {
        let not_found = DatabaseError::from(sqlx::Error::RowNotFound);
        assert!(matches!(not_found, DatabaseError::NotFound(_)));

        // Simulate duplicate key error check (requires constructing a specific sqlx error which is hard,
        // so we check the fallback)
        let generic = DatabaseError::from(sqlx::Error::WorkerCrashed);
        assert!(matches!(generic, DatabaseError::Query(_)));
    }

    #[test]
    fn test_validation_conversion() {
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
        let app_err = AppError::from(err);

        assert!(matches!(
            app_err,
            AppError::Validation(ValidationError::Multiple(_))
        ));
    }
}
