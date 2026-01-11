//! Application error types with proper error chaining.

use thiserror::Error;

#[derive(Error, Debug, Clone)]
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

#[derive(Error, Debug, Clone)]
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

impl From<&str> for ConfigError {
    fn from(s: &str) -> Self {
        ConfigError::ParseError(s.to_string())
    }
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

impl From<&str> for ValidationError {
    fn from(s: &str) -> Self {
        ValidationError::InvalidFormat(s.to_string())
    }
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

        // Test pool timeout
        let pool_timeout = DatabaseError::from(sqlx::Error::PoolTimedOut);
        assert!(matches!(pool_timeout, DatabaseError::PoolExhausted(_)));

        // Simulate fallback for unknown errors
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
    fn test_serde_json_error_conversion() {
        let json_err = serde_json::from_str::<String>("invalid json").unwrap_err();
        let app_err = AppError::from(json_err);
        assert!(matches!(app_err, AppError::Serialization(_)));
    }

    #[test]
    fn test_database_error_display() {
        let err = DatabaseError::Connection("timeout".to_string());
        assert_eq!(err.to_string(), "Connection failed: timeout");

        let err = DatabaseError::Query("syntax error".to_string());
        assert_eq!(err.to_string(), "Query execution failed: syntax error");

        let err = DatabaseError::NotFound("item_123".to_string());
        assert_eq!(err.to_string(), "Record not found: item_123");

        let err = DatabaseError::Duplicate("unique violation".to_string());
        assert_eq!(err.to_string(), "Duplicate record: unique violation");

        let err = DatabaseError::PoolExhausted("no connections".to_string());
        assert_eq!(err.to_string(), "Pool exhausted: no connections");

        let err = DatabaseError::Migration("failed".to_string());
        assert_eq!(err.to_string(), "Migration failed: failed");
    }

    #[test]
    fn test_blockchain_error_display() {
        let err = BlockchainError::Connection("refused".to_string());
        assert_eq!(err.to_string(), "Connection failed: refused");

        let err = BlockchainError::RpcError("invalid method".to_string());
        assert_eq!(err.to_string(), "RPC call failed: invalid method");

        let err = BlockchainError::TransactionFailed("nonce".to_string());
        assert_eq!(err.to_string(), "Transaction failed: nonce");

        let err = BlockchainError::InvalidSignature("corrupt".to_string());
        assert_eq!(err.to_string(), "Invalid signature: corrupt");

        let err = BlockchainError::InsufficientFunds;
        assert_eq!(err.to_string(), "Insufficient funds for transaction");

        let err = BlockchainError::Timeout("30s".to_string());
        assert_eq!(err.to_string(), "Timeout waiting for confirmation: 30s");
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::MissingEnvVar("DATABASE_URL".to_string());
        assert_eq!(
            err.to_string(),
            "Missing environment variable: DATABASE_URL"
        );

        let err = ConfigError::InvalidValue {
            key: "PORT".to_string(),
            message: "not a number".to_string(),
        };
        assert_eq!(err.to_string(), "Invalid value for 'PORT': not a number");

        let err = ConfigError::ParseError("json parse".to_string());
        assert_eq!(err.to_string(), "Parse error: json parse");
    }

    #[test]
    fn test_validation_error_display() {
        let err = ValidationError::InvalidField {
            field: "email".to_string(),
            message: "invalid format".to_string(),
        };
        assert_eq!(err.to_string(), "Invalid field 'email': invalid format");

        let err = ValidationError::MissingField("name".to_string());
        assert_eq!(err.to_string(), "Missing required field: name");

        let err = ValidationError::InvalidFormat("date".to_string());
        assert_eq!(err.to_string(), "Invalid format: date");

        let err = ValidationError::Multiple("many errors".to_string());
        assert_eq!(err.to_string(), "Validation failed: many errors");
    }

    #[test]
    fn test_external_service_error_display() {
        let err = ExternalServiceError::HttpError("404".to_string());
        assert_eq!(err.to_string(), "HTTP request failed: 404");

        let err = ExternalServiceError::Unavailable("payment gateway".to_string());
        assert_eq!(err.to_string(), "Service unavailable: payment gateway");

        let err = ExternalServiceError::Timeout("30s".to_string());
        assert_eq!(err.to_string(), "Timeout: 30s");

        let err = ExternalServiceError::RateLimited("api".to_string());
        assert_eq!(err.to_string(), "Rate limited: api");
    }

    #[test]
    fn test_app_error_display() {
        let err = AppError::Authentication("bad token".to_string());
        assert_eq!(err.to_string(), "Authentication failed: bad token");

        let err = AppError::Authorization("no access".to_string());
        assert_eq!(err.to_string(), "Authorization denied: no access");

        let err = AppError::Serialization("json".to_string());
        assert_eq!(err.to_string(), "Serialization error: json");

        let err = AppError::Deserialization("yaml".to_string());
        assert_eq!(err.to_string(), "Deserialization error: yaml");

        let err = AppError::Internal("panic".to_string());
        assert_eq!(err.to_string(), "Internal error: panic");

        let err = AppError::NotSupported("feature".to_string());
        assert_eq!(err.to_string(), "Operation not supported: feature");

        let err = AppError::RateLimited;
        assert_eq!(err.to_string(), "Rate limit exceeded");
    }

    #[test]
    fn test_app_error_from_database_error() {
        let db_err = DatabaseError::NotFound("id".to_string());
        let app_err: AppError = db_err.into();
        assert!(matches!(
            app_err,
            AppError::Database(DatabaseError::NotFound(_))
        ));
    }

    #[test]
    fn test_app_error_from_blockchain_error() {
        let bc_err = BlockchainError::Timeout("10s".to_string());
        let app_err: AppError = bc_err.into();
        assert!(matches!(
            app_err,
            AppError::Blockchain(BlockchainError::Timeout(_))
        ));
    }

    #[test]
    fn test_app_error_from_external_service_error() {
        let ext_err = ExternalServiceError::Unavailable("api".to_string());
        let app_err: AppError = ext_err.into();
        assert!(matches!(
            app_err,
            AppError::ExternalService(ExternalServiceError::Unavailable(_))
        ));
    }

    #[test]
    fn test_app_error_from_config_error() {
        let cfg_err = ConfigError::MissingEnvVar("KEY".to_string());
        let app_err: AppError = cfg_err.into();
        assert!(matches!(
            app_err,
            AppError::Config(ConfigError::MissingEnvVar(_))
        ));
    }

    #[test]
    fn test_app_error_from_validation_error() {
        let val_err = ValidationError::MissingField("email".to_string());
        let app_err: AppError = val_err.into();
        assert!(matches!(
            app_err,
            AppError::Validation(ValidationError::MissingField(_))
        ));
    }

    #[test]
    fn test_app_error_from_migrate_error() {
        // Construct a simple MigrateError.
        // MigrateError::VersionMissing(1) is easy to construct.
        let mig_err = sqlx::migrate::MigrateError::VersionMissing(1);
        let app_err: AppError = mig_err.into();

        match app_err {
            AppError::Database(DatabaseError::Migration(msg)) => {
                assert!(msg.contains("migration 1 was previously applied"));
            }
            _ => panic!("Expected DatabaseError::Migration, got {:?}", app_err),
        }
    }
}
