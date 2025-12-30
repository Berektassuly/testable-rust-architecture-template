//! PostgreSQL database client implementation.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;
use tracing::{info, instrument};

use crate::domain::{
    AppError, BlockchainStatus, CreateItemRequest, DatabaseClient, DatabaseError, Item,
    ItemMetadata, PaginatedResponse,
};

/// PostgreSQL connection pool configuration
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
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

/// PostgreSQL database client with connection pooling
pub struct PostgresClient {
    pool: PgPool,
}

impl PostgresClient {
    /// Create a new PostgreSQL client with custom configuration
    pub async fn new(database_url: &str, config: PostgresConfig) -> Result<Self, AppError> {
        info!("Connecting to PostgreSQL...");
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(database_url)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Connection(e.to_string())))?;
        info!("Connected to PostgreSQL");
        Ok(Self { pool })
    }

    /// Create a new PostgreSQL client with default configuration
    pub async fn with_defaults(database_url: &str) -> Result<Self, AppError> {
        Self::new(database_url, PostgresConfig::default()).await
    }

    /// Run database migrations using sqlx migrate
    pub async fn run_migrations(&self) -> Result<(), AppError> {
        info!("Running database migrations...");
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Migration(e.to_string())))?;
        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Get the underlying connection pool (for testing)
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Parse a database row into an Item
    fn row_to_item(row: &sqlx::postgres::PgRow) -> Result<Item, AppError> {
        let metadata: Option<serde_json::Value> = row.try_get("metadata").ok();
        let status_str: String = row.get("blockchain_status");

        Ok(Item {
            id: row.get("id"),
            hash: row.get("hash"),
            name: row.get("name"),
            description: row.get("description"),
            content: row.get("content"),
            metadata: metadata.and_then(|v| serde_json::from_value(v).ok()),
            blockchain_status: status_str.parse().unwrap_or(BlockchainStatus::Pending),
            blockchain_signature: row.get("blockchain_signature"),
            blockchain_retry_count: row.get("blockchain_retry_count"),
            blockchain_last_error: row.get("blockchain_last_error"),
            blockchain_next_retry_at: row.get("blockchain_next_retry_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}

#[async_trait]
impl DatabaseClient for PostgresClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), AppError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Connection(e.to_string())))?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        let row = sqlx::query(
            r#"
            SELECT id, hash, name, description, content, metadata, 
                   blockchain_status, blockchain_signature, blockchain_retry_count,
                   blockchain_last_error, blockchain_next_retry_at,
                   created_at, updated_at 
            FROM items 
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_item(&row)?)),
            None => Ok(None),
        }
    }

    #[instrument(skip(self, data), fields(item_name = %data.name))]
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, AppError> {
        let id = format!("item_{}", uuid::Uuid::new_v4());
        let hash = format!("hash_{}", uuid::Uuid::new_v4());
        let now = Utc::now();

        let metadata_json = data
            .metadata
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO items (id, hash, name, description, content, metadata, 
                               blockchain_status, blockchain_retry_count,
                               created_at, updated_at) 
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(&id)
        .bind(&hash)
        .bind(&data.name)
        .bind(&data.description)
        .bind(&data.content)
        .bind(&metadata_json)
        .bind(BlockchainStatus::Pending.as_str())
        .bind(0i32)
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

        Ok(Item {
            id,
            hash,
            name: data.name.clone(),
            description: data.description.clone(),
            content: data.content.clone(),
            metadata,
            blockchain_status: BlockchainStatus::Pending,
            blockchain_signature: None,
            blockchain_retry_count: 0,
            blockchain_last_error: None,
            blockchain_next_retry_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[instrument(skip(self))]
    async fn list_items(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<PaginatedResponse<Item>, AppError> {
        // Clamp limit to valid range
        let limit = limit.clamp(1, 100);
        // Fetch one extra to determine if there are more items
        let fetch_limit = limit + 1;

        let rows = match cursor {
            Some(cursor_id) => {
                // Get the created_at of the cursor item for proper pagination
                let cursor_row = sqlx::query("SELECT created_at FROM items WHERE id = $1")
                    .bind(cursor_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

                let cursor_created_at: DateTime<Utc> = match cursor_row {
                    Some(row) => row.get("created_at"),
                    None => {
                        return Err(AppError::Validation(
                            crate::domain::ValidationError::InvalidField {
                                field: "cursor".to_string(),
                                message: "Invalid cursor".to_string(),
                            },
                        ));
                    }
                };

                sqlx::query(
                    r#"
                    SELECT id, hash, name, description, content, metadata,
                           blockchain_status, blockchain_signature, blockchain_retry_count,
                           blockchain_last_error, blockchain_next_retry_at,
                           created_at, updated_at
                    FROM items
                    WHERE (created_at, id) < ($1, $2)
                    ORDER BY created_at DESC, id DESC
                    LIMIT $3
                    "#,
                )
                .bind(cursor_created_at)
                .bind(cursor_id)
                .bind(fetch_limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?
            }
            None => sqlx::query(
                r#"
                    SELECT id, hash, name, description, content, metadata,
                           blockchain_status, blockchain_signature, blockchain_retry_count,
                           blockchain_last_error, blockchain_next_retry_at,
                           created_at, updated_at
                    FROM items
                    ORDER BY created_at DESC, id DESC
                    LIMIT $1
                    "#,
            )
            .bind(fetch_limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?,
        };

        let has_more = rows.len() > limit as usize;
        let items: Vec<Item> = rows
            .iter()
            .take(limit as usize)
            .map(Self::row_to_item)
            .collect::<Result<Vec<_>, _>>()?;

        let next_cursor = if has_more {
            items.last().map(|item| item.id.clone())
        } else {
            None
        };

        Ok(PaginatedResponse::new(items, next_cursor, has_more))
    }

    #[instrument(skip(self))]
    async fn update_blockchain_status(
        &self,
        id: &str,
        status: BlockchainStatus,
        signature: Option<&str>,
        error: Option<&str>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), AppError> {
        let now = Utc::now();

        sqlx::query(
            r#"
            UPDATE items 
            SET blockchain_status = $1,
                blockchain_signature = COALESCE($2, blockchain_signature),
                blockchain_last_error = $3,
                blockchain_next_retry_at = $4,
                updated_at = $5
            WHERE id = $6
            "#,
        )
        .bind(status.as_str())
        .bind(signature)
        .bind(error)
        .bind(next_retry_at)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_pending_blockchain_items(&self, limit: i64) -> Result<Vec<Item>, AppError> {
        let now = Utc::now();
        let rows = sqlx::query(
            r#"
            SELECT id, hash, name, description, content, metadata,
                   blockchain_status, blockchain_signature, blockchain_retry_count,
                   blockchain_last_error, blockchain_next_retry_at,
                   created_at, updated_at
            FROM items
            WHERE blockchain_status = 'pending_submission'
              AND (blockchain_next_retry_at IS NULL OR blockchain_next_retry_at <= $1)
              AND blockchain_retry_count < 10
            ORDER BY blockchain_next_retry_at ASC NULLS FIRST, created_at ASC
            LIMIT $2
            "#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        rows.iter().map(Self::row_to_item).collect()
    }

    #[instrument(skip(self))]
    async fn increment_retry_count(&self, id: &str) -> Result<i32, AppError> {
        let row = sqlx::query(
            r#"
            UPDATE items 
            SET blockchain_retry_count = blockchain_retry_count + 1,
                updated_at = NOW()
            WHERE id = $1
            RETURNING blockchain_retry_count
            "#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        Ok(row.get("blockchain_retry_count"))
    }
}
