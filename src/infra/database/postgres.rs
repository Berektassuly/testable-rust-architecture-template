//! PostgreSQL database client implementation.
//! Maps sqlx errors to domain ItemError / HealthCheckError; does not leak SQL or driver details.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgPoolOptions, types::Json};
use std::time::Duration;
use thiserror::Error;
use tracing::{info, instrument};

use crate::domain::{
    BlockchainStatus, CreateItemRequest, HealthCheckError, Item, ItemError, ItemMetadata,
    ItemRepository, OutboxRepository, OutboxStatus, PaginatedResponse, SolanaOutboxEntry,
    SolanaOutboxPayload, build_solana_outbox_payload_from_request,
};

/// Error for Postgres client construction and migrations (used by main only).
#[derive(Error, Debug)]
pub enum PostgresInitError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Migration failed: {0}")]
    Migration(String),
}

fn map_sqlx_to_item_error(e: sqlx::Error) -> ItemError {
    match &e {
        sqlx::Error::RowNotFound => ItemError::NotFound("Row not found".to_string()),
        sqlx::Error::Database(db_err) => {
            if db_err.code().as_deref() == Some("23505") {
                return ItemError::InvalidState("Duplicate".to_string());
            }
            ItemError::RepositoryFailure
        }
        _ => ItemError::RepositoryFailure,
    }
}

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
    pub async fn new(
        database_url: &str,
        config: PostgresConfig,
    ) -> Result<Self, PostgresInitError> {
        info!("Connecting to PostgreSQL...");
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(database_url)
            .await
            .map_err(|e| PostgresInitError::Connection(e.to_string()))?;
        info!("Connected to PostgreSQL");
        Ok(Self { pool })
    }

    /// Create a new PostgreSQL client with default configuration
    pub async fn with_defaults(database_url: &str) -> Result<Self, PostgresInitError> {
        Self::new(database_url, PostgresConfig::default()).await
    }

    /// Run database migrations using sqlx migrate
    pub async fn run_migrations(&self) -> Result<(), PostgresInitError> {
        info!("Running database migrations...");
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| PostgresInitError::Migration(e.to_string()))?;
        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Get the underlying connection pool (for testing)
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Parse a database row into an Item
    fn row_to_item(row: &sqlx::postgres::PgRow) -> Result<Item, ItemError> {
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

    /// Parse a database row into a Solana outbox entry
    fn row_to_outbox(row: &sqlx::postgres::PgRow) -> Result<SolanaOutboxEntry, ItemError> {
        let payload: Json<SolanaOutboxPayload> = row
            .try_get("payload")
            .map_err(|_| ItemError::RepositoryFailure)?;
        let status_str: String = row.get("status");

        Ok(SolanaOutboxEntry {
            id: row.get::<uuid::Uuid, _>("id").to_string(),
            aggregate_id: row.get("aggregate_id"),
            payload: payload.0,
            status: status_str.parse().unwrap_or(OutboxStatus::Pending),
            retry_count: row.get("retry_count"),
            created_at: row.get("created_at"),
        })
    }
}

#[async_trait]
impl ItemRepository for PostgresClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), HealthCheckError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|_| HealthCheckError::DatabaseUnavailable)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_item(&self, id: &str) -> Result<Option<Item>, ItemError> {
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
        .map_err(map_sqlx_to_item_error)?;

        match row {
            Some(row) => Ok(Some(Self::row_to_item(&row)?)),
            None => Ok(None),
        }
    }

    #[instrument(skip(self, data), fields(item_name = %data.name))]
    async fn create_item(&self, data: &CreateItemRequest) -> Result<Item, ItemError> {
        let id = format!("item_{}", uuid::Uuid::now_v7());
        let hash = format!("hash_{}", uuid::Uuid::now_v7());
        let now = Utc::now();
        let outbox_id = uuid::Uuid::now_v7();
        let outbox_payload = build_solana_outbox_payload_from_request(&id, data);

        let metadata_json = data
            .metadata
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| ItemError::RepositoryFailure)?;

        let mut tx = self.pool.begin().await.map_err(map_sqlx_to_item_error)?;

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
        .bind(BlockchainStatus::PendingSubmission.as_str())
        .bind(0i32)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            INSERT INTO solana_outbox (id, aggregate_id, payload, status, created_at, retry_count)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(outbox_id)
        .bind(&id)
        .bind(Json(outbox_payload))
        .bind(OutboxStatus::Pending.as_str())
        .bind(now)
        .bind(0i32)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        tx.commit().await.map_err(map_sqlx_to_item_error)?;

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
            blockchain_status: BlockchainStatus::PendingSubmission,
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
    ) -> Result<PaginatedResponse<Item>, ItemError> {
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
                    .map_err(map_sqlx_to_item_error)?;

                let cursor_created_at: DateTime<Utc> = match cursor_row {
                    Some(row) => row.get("created_at"),
                    None => {
                        return Err(ItemError::InvalidState("Invalid cursor".to_string()));
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
                .map_err(map_sqlx_to_item_error)?
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
            .map_err(map_sqlx_to_item_error)?,
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
    ) -> Result<(), ItemError> {
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
        .map_err(map_sqlx_to_item_error)?;

        Ok(())
    }

    #[instrument(skip(self, payload))]
    async fn enqueue_solana_outbox_for_item(
        &self,
        item_id: &str,
        payload: &SolanaOutboxPayload,
    ) -> Result<Item, ItemError> {
        let now = Utc::now();
        let outbox_id = uuid::Uuid::now_v7();
        let mut tx = self.pool.begin().await.map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            INSERT INTO solana_outbox (id, aggregate_id, payload, status, created_at, retry_count)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(outbox_id)
        .bind(item_id)
        .bind(Json(payload.clone()))
        .bind(OutboxStatus::Pending.as_str())
        .bind(now)
        .bind(0i32)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        let row = sqlx::query(
            r#"
            UPDATE items
            SET blockchain_status = $1,
                blockchain_last_error = NULL,
                blockchain_next_retry_at = NULL,
                blockchain_retry_count = 0,
                updated_at = $2
            WHERE id = $3
            RETURNING id, hash, name, description, content, metadata,
                      blockchain_status, blockchain_signature, blockchain_retry_count,
                      blockchain_last_error, blockchain_next_retry_at,
                      created_at, updated_at
            "#,
        )
        .bind(BlockchainStatus::PendingSubmission.as_str())
        .bind(now)
        .bind(item_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        tx.commit().await.map_err(map_sqlx_to_item_error)?;

        Self::row_to_item(&row)
    }

    #[instrument(skip(self))]
    async fn get_pending_blockchain_items(&self, limit: i64) -> Result<Vec<Item>, ItemError> {
        let now = Utc::now();
        let rows = sqlx::query(
            r#"
            WITH candidate AS (
                SELECT id
                FROM items
                WHERE blockchain_status = 'pending_submission'
                  AND (blockchain_next_retry_at IS NULL OR blockchain_next_retry_at <= $1)
                  AND blockchain_retry_count < 10
                ORDER BY blockchain_next_retry_at ASC NULLS FIRST, created_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            UPDATE items
            SET updated_at = $1
            FROM candidate
            WHERE items.id = candidate.id
            RETURNING items.id, items.hash, items.name, items.description, items.content, items.metadata,
                      items.blockchain_status, items.blockchain_signature, items.blockchain_retry_count,
                      items.blockchain_last_error, items.blockchain_next_retry_at,
                      items.created_at, items.updated_at
            "#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_to_item_error)?;

        rows.iter().map(Self::row_to_item).collect()
    }

    #[instrument(skip(self))]
    async fn increment_retry_count(&self, id: &str) -> Result<i32, ItemError> {
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
        .map_err(map_sqlx_to_item_error)?;

        Ok(row.get("blockchain_retry_count"))
    }
}

#[async_trait]
impl OutboxRepository for PostgresClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), HealthCheckError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|_| HealthCheckError::DatabaseUnavailable)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn claim_pending_solana_outbox(
        &self,
        limit: i64,
    ) -> Result<Vec<SolanaOutboxEntry>, ItemError> {
        let now = Utc::now();
        let rows = sqlx::query(
            r#"
            WITH candidate AS (
                SELECT o.id
                FROM solana_outbox o
                JOIN items i ON i.id = o.aggregate_id
                WHERE o.status = 'pending'
                  AND (i.blockchain_next_retry_at IS NULL OR i.blockchain_next_retry_at <= $1)
                ORDER BY o.created_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            UPDATE solana_outbox o
            SET status = 'processing'
            FROM candidate
            WHERE o.id = candidate.id
            RETURNING o.id, o.aggregate_id, o.payload, o.status, o.retry_count, o.created_at
            "#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_to_item_error)?;

        rows.iter().map(Self::row_to_outbox).collect()
    }

    #[instrument(skip(self))]
    async fn complete_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        signature: &str,
    ) -> Result<(), ItemError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await.map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            UPDATE solana_outbox
            SET status = $1
            WHERE id = $2
            "#,
        )
        .bind(OutboxStatus::Completed.as_str())
        .bind(outbox_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            UPDATE items
            SET blockchain_status = $1,
                blockchain_signature = $2,
                blockchain_last_error = NULL,
                blockchain_next_retry_at = NULL,
                updated_at = $3
            WHERE id = $4
            "#,
        )
        .bind(BlockchainStatus::Submitted.as_str())
        .bind(signature)
        .bind(now)
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        tx.commit().await.map_err(map_sqlx_to_item_error)?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn fail_solana_outbox(
        &self,
        outbox_id: &str,
        item_id: &str,
        retry_count: i32,
        outbox_status: OutboxStatus,
        item_status: BlockchainStatus,
        error: &str,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), ItemError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await.map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            UPDATE solana_outbox
            SET status = $1,
                retry_count = $2
            WHERE id = $3
            "#,
        )
        .bind(outbox_status.as_str())
        .bind(retry_count)
        .bind(outbox_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        sqlx::query(
            r#"
            UPDATE items
            SET blockchain_status = $1,
                blockchain_last_error = $2,
                blockchain_next_retry_at = $3,
                blockchain_retry_count = $4,
                updated_at = $5
            WHERE id = $6
            "#,
        )
        .bind(item_status.as_str())
        .bind(error)
        .bind(next_retry_at)
        .bind(retry_count)
        .bind(now)
        .bind(item_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_to_item_error)?;

        tx.commit().await.map_err(map_sqlx_to_item_error)?;

        Ok(())
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
        assert_eq!(config.acquire_timeout, Duration::from_secs(3));
        assert_eq!(config.idle_timeout, Duration::from_secs(600));
        assert_eq!(config.max_lifetime, Duration::from_secs(1800));
    }

    #[test]
    fn test_postgres_config_custom() {
        let config = PostgresConfig {
            max_connections: 20,
            min_connections: 5,
            acquire_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(3600),
        };
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.acquire_timeout, Duration::from_secs(10));
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
        assert_eq!(config.max_lifetime, Duration::from_secs(3600));
    }
}
