//! PostgreSQL database client implementation.

use async_trait::async_trait;
use chrono::Utc;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;
use tracing::{info, instrument};

use crate::domain::{
    AppError, CreateItemRequest, DatabaseClient, DatabaseError, Item, ItemMetadata,
};

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

pub struct PostgresClient {
    pool: PgPool,
}

impl PostgresClient {
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

    pub async fn with_defaults(database_url: &str) -> Result<Self, AppError> {
        Self::new(database_url, PostgresConfig::default()).await
    }

    pub async fn run_migrations(&self) -> Result<(), AppError> {
        info!("Running migrations...");
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
        info!("Migrations completed");
        Ok(())
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
            "SELECT id, hash, name, description, metadata, created_at, updated_at FROM items WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(DatabaseError::Query(e.to_string())))?;

        match row {
            Some(row) => {
                let metadata: Option<serde_json::Value> = row.get("metadata");
                Ok(Some(Item {
                    id: row.get("id"),
                    hash: row.get("hash"),
                    name: row.get("name"),
                    description: row.get("description"),
                    metadata: metadata.and_then(|v| serde_json::from_value(v).ok()),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }))
            }
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
            "INSERT INTO items (id, hash, name, description, metadata, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
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

        Ok(Item {
            id,
            hash,
            name: data.name.clone(),
            description: data.description.clone(),
            metadata,
            created_at: now,
            updated_at: now,
        })
    }
}
