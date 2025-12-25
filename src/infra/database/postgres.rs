//! PostgreSQL database client implementation.
//!
//! This module provides a production-ready implementation of the
//! `DatabaseClient` trait using SQLx for async database operations.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::time::Duration;
use tracing::{debug, info, instrument};

use crate::domain::{
    AppError, CreateItemRequest, DatabaseClient, DatabaseError, Item, ItemMetadata,
};

/// Configuration for the PostgreSQL connection pool.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
    /// Minimum number of connections to maintain.
    pub min_connections: u32,
    /// Maximum time to wait for a connection.
    pub acquire_timeout: Duration,
    /// Maximum idle time for a connection.
    pub idle_timeout: Duration,
    /// Maximum lifetime of a connection.
    pub max_lifetime: Duration,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 2,
            acquire_timeout: Duration::from_secs(3),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

impl PostgresConfig {
    /// Creates a new configuration for development.
    #[must_use]
    pub fn development() -> Self {
        Self {
            max_connections: 5,
            min_connections: 1,
            acquire_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(900),
        }
    }

    /// Creates a new configuration for production.
    #[must_use]
    pub fn production() -> Self {
        Self {
            max_connections: 20,
            min_connections: 5,
            acquire_timeout: Duration::from_secs(3),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

/// PostgreSQL database client implementation.
///
/// This struct provides a production-ready implementation of the `DatabaseClient`
/// trait using SQLx for async database operations with connection pooling.
///
/// # Example
///
/// ```ignore
/// let config = PostgresConfig::default();
/// let client = PostgresClient::new("postgres://localhost/mydb", config).await?;
///
/// let item = client.get_item("item-123").await?;
/// ```
pub struct PostgresClient {
    pool: PgPool,
}

impl PostgresClient {
    /// Creates a new `PostgresClient` instance with a connection pool.
    ///
    /// # Arguments
    ///
    /// * `database_url` - The PostgreSQL connection string.
    /// * `config` - Pool configuration options.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection pool cannot be established.
    pub async fn new(database_url: &str, config: PostgresConfig) -> Result<Self, AppError> {
        info!("Connecting to PostgreSQL database...");

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(database_url)
            .await
            .map_err(|e| {
                AppError::Database(DatabaseError::Connection(format!(
                    "Failed to connect to database: {}",
                    e
                )))
            })?;

        info!("Successfully connected to PostgreSQL");

        Ok(Self { pool })
    }

    /// Creates a new client with default configuration.
    pub async fn with_defaults(database_url: &str) -> Result<Self, AppError> {
        Self::new(database_url, PostgresConfig::default()).await
    }

    /// Returns a reference to the connection pool.
    ///
    /// This can be useful for running migrations or custom queries.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Runs database migrations.
    ///
    /// This should be called during application startup.
    pub async fn run_migrations(&self) -> Result<(), AppError> {
        info!("Running database migrations...");

        // Create the items table if it doesn't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS items (
                id VARCHAR(255) PRIMARY KEY,
                hash VARCHAR(255) NOT NULL,
                name VARCHAR(255) NOT NULL,
                description TEXT,
                metadata JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Migration(e.to_string())))?;

        // Create index on hash for faster lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_items_hash ON items(hash)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Migration(e.to_string())))?;

        info!("Database migrations completed successfully");

        Ok(())
    }
}

#[async_trait]
impl DatabaseClient for PostgresClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), AppError> {
        debug!("Performing database health check");

        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Connection(e.to_string())))?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        debug!(item_id = %id, "Fetching item from database");

        let row = sqlx::query(
            r#"
            SELECT id, hash, name, description, metadata, created_at, updated_at
            FROM items
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        match row {
            Some(row) => {
                let metadata: Option<serde_json::Value> = row.get("metadata");
                let item = Item {
                    id: row.get("id"),
                    hash: row.get("hash"),
                    name: row.get("name"),
                    description: row.get("description"),
                    metadata: metadata.and_then(|v| serde_json::from_value(v).ok()),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                };
                Ok(Some(item))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, data), fields(item_name = %data.name))]
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError> {
        debug!("Creating new item in database");

        let id = format!("item_{}", uuid::Uuid::new_v4());
        let hash = format!("hash_{}", uuid::Uuid::new_v4());
        let now = Utc::now();

        let metadata_json = data
            .metadata
            .as_ref()
            .map(|m| serde_json::to_value(m))
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO items (id, hash, name, description, metadata, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&id)
        .bind(&hash)
        .bind(&data.name)
        .bind(&data.description)
        .bind(&metadata_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::from(e)))?;

        let metadata: Option<ItemMetadata> = data.metadata.as_ref().map(|m| ItemMetadata {
            author: m.author.clone(),
            version: m.version.clone(),
            tags: m.tags.clone(),
            custom_fields: m.custom_fields.clone(),
        });

        let item = Item {
            id,
            hash,
            name: data.name.clone(),
            description: data.description.clone(),
            metadata,
            created_at: now,
            updated_at: now,
        };

        info!(item_id = %item.id, "Item created successfully");

        Ok(item)
    }

    #[instrument(skip(self, data), fields(item_id = %id))]
    async fn update_item(&self, id: &str, data: &CreateItemRequest) -> Result<Item, AppError> {
        debug!("Updating item in database");

        let now = Utc::now();

        let metadata_json = data
            .metadata
            .as_ref()
            .map(|m| serde_json::to_value(m))
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        let result = sqlx::query(
            r#"
            UPDATE items
            SET name = $2, description = $3, metadata = $4, updated_at = $5
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(&data.name)
        .bind(&data.description)
        .bind(&metadata_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        if result.rows_affected() == 0 {
            return Err(AppError::Database(DatabaseError::NotFound(format!(
                "Item {} not found",
                id
            ))));
        }

        // Fetch the updated item
        self.get_item(id)
            .await?
            .ok_or_else(|| AppError::Database(DatabaseError::NotFound(format!("Item {}", id))))
    }

    #[instrument(skip(self))]
    async fn delete_item(&self, id: &str) -> Result<bool, AppError> {
        debug!(item_id = %id, "Deleting item from database");

        let result = sqlx::query("DELETE FROM items WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        let deleted = result.rows_affected() > 0;

        if deleted {
            info!(item_id = %id, "Item deleted successfully");
        } else {
            debug!(item_id = %id, "Item not found for deletion");
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_config_default() {
        let config = PostgresConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
    }

    #[test]
    fn test_postgres_config_development() {
        let config = PostgresConfig::development();
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.min_connections, 1);
    }

    #[test]
    fn test_postgres_config_production() {
        let config = PostgresConfig::production();
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 5);
    }
}
