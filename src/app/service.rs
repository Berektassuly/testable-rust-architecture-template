//! Application service layer.

use std::sync::Arc;
use tracing::{info, instrument, warn};
use validator::Validate;

use crate::domain::{
    AppError, BlockchainClient, CreateItemRequest, DatabaseClient, HealthResponse, HealthStatus,
    Item, ValidationError,
};

pub struct AppService {
    db_client: Arc<dyn DatabaseClient>,
    blockchain_client: Arc<dyn BlockchainClient>,
}

impl AppService {
    #[must_use]
    pub fn new(
        db_client: Arc<dyn DatabaseClient>,
        blockchain_client: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            db_client,
            blockchain_client,
        }
    }

    #[instrument(skip(self, request), fields(item_name = %request.name))]
    pub async fn create_and_submit_item(
        &self,
        request: &CreateItemRequest,
    ) -> Result<Item, AppError> {
        request.validate().map_err(|e| {
            warn!(error = %e, "Validation failed");
            AppError::Validation(ValidationError::Multiple(e.to_string()))
        })?;

        info!("Creating new item: {}", request.name);
        let item = self.db_client.create_item(request).await?;
        info!(item_id = %item.id, "Item created in database");

        let hash = self.generate_hash(&item);
        match self.blockchain_client.submit_transaction(&hash).await {
            Ok(signature) => {
                info!(item_id = %item.id, signature = %signature, "Submitted to blockchain");
            }
            Err(e) => {
                warn!(item_id = %item.id, error = ?e, "Blockchain submission failed");
                return Err(e);
            }
        }

        Ok(item)
    }

    #[instrument(skip(self))]
    pub async fn get_item(&self, id: &str) -> Result<Option<Item>, AppError> {
        self.db_client.get_item(id).await
    }

    #[instrument(skip(self))]
    pub async fn health_check(&self) -> HealthResponse {
        let db_health = match self.db_client.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        };
        let blockchain_health = match self.blockchain_client.health_check().await {
            Ok(()) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Unhealthy,
        };
        HealthResponse::new(db_health, blockchain_health)
    }

    fn generate_hash(&self, item: &Item) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(item.id.as_bytes());
        hasher.update(item.name.as_bytes());
        if let Some(ref desc) = item.description {
            hasher.update(desc.as_bytes());
        }
        let result = hasher.finalize();
        result.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
