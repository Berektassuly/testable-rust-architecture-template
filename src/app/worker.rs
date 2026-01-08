//! Background worker for processing pending blockchain submissions.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info};

use super::service::AppService;

/// Configuration for the background worker
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Interval between processing batches
    pub poll_interval: Duration,
    /// Number of items to process per batch
    pub batch_size: i64,
    /// Whether the worker is enabled
    pub enabled: bool,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(10),
            batch_size: 10,
            enabled: true,
        }
    }
}

/// Background worker for processing pending blockchain submissions
pub struct BlockchainRetryWorker {
    service: Arc<AppService>,
    config: WorkerConfig,
    shutdown_rx: watch::Receiver<bool>,
}

impl BlockchainRetryWorker {
    /// Create a new worker instance
    pub fn new(
        service: Arc<AppService>,
        config: WorkerConfig,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            service,
            config,
            shutdown_rx,
        }
    }

    /// Run the worker loop
    pub async fn run(mut self) {
        if !self.config.enabled {
            info!("Blockchain retry worker is disabled");
            return;
        }

        info!(
            poll_interval = ?self.config.poll_interval,
            batch_size = self.config.batch_size,
            "Starting blockchain retry worker"
        );

        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.config.poll_interval) => {
                    self.process_batch().await;
                }
                result = self.shutdown_rx.changed() => {
                    if result.is_ok() && *self.shutdown_rx.borrow() {
                        info!("Blockchain retry worker shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Process a batch of pending submissions
    async fn process_batch(&self) {
        match self
            .service
            .process_pending_submissions(self.config.batch_size)
            .await
        {
            Ok(0) => {
                // No pending items, nothing to log
            }
            Ok(count) => {
                info!(count = count, "Processed pending blockchain submissions");
            }
            Err(e) => {
                error!(error = ?e, "Error processing pending submissions");
            }
        }
    }
}

/// Spawn the background worker as a tokio task
pub fn spawn_worker(
    service: Arc<AppService>,
    config: WorkerConfig,
) -> (tokio::task::JoinHandle<()>, watch::Sender<bool>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);
    let handle = tokio::spawn(worker.run());
    (handle, shutdown_tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{MockBlockchainClient, MockDatabaseClient};

    fn create_test_service() -> Arc<AppService> {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());
        Arc::new(AppService::new(db, bc))
    }

    #[test]
    fn test_worker_config_default() {
        let config = WorkerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(10));
        assert_eq!(config.batch_size, 10);
        assert!(config.enabled);
    }

    #[test]
    fn test_worker_config_custom() {
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(5),
            batch_size: 20,
            enabled: false,
        };
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.batch_size, 20);
        assert!(!config.enabled);
    }

    #[tokio::test]
    async fn test_worker_disabled_returns_immediately() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_millis(100),
            batch_size: 10,
            enabled: false, // Disabled
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // Should return immediately without blocking
        let start = std::time::Instant::now();
        worker.run().await;
        let elapsed = start.elapsed();

        // Should complete almost instantly (less than 50ms)
        assert!(elapsed < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_worker_shutdown_via_channel() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60), // Long poll so it doesn't trigger
            batch_size: 10,
            enabled: true,
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // Spawn worker in background
        let handle = tokio::spawn(worker.run());

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send shutdown signal
        shutdown_tx.send(true).unwrap();

        // Worker should complete within reasonable time
        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(result.is_ok(), "Worker should shutdown within 2 seconds");
    }

    #[tokio::test]
    async fn test_spawn_worker_returns_handles() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60),
            batch_size: 10,
            enabled: false, // Disabled so it returns immediately
        };

        let (handle, shutdown_tx) = spawn_worker(service, config);

        // Wait for disabled worker to finish (it returns immediately when disabled)
        let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
        assert!(
            result.is_ok(),
            "Worker should complete within 1 second when disabled"
        );

        // Shutdown sender should still be usable (no panic on send)
        let _ = shutdown_tx.send(true);
    }

    #[tokio::test]
    async fn test_worker_new_construction() {
        let service = create_test_service();
        let config = WorkerConfig::default();
        let (_, shutdown_rx) = watch::channel(false);

        let worker = BlockchainRetryWorker::new(Arc::clone(&service), config.clone(), shutdown_rx);

        // Worker should be constructed without panicking
        // Since fields are private, we verify by running it (which tests all the fields were set)
        drop(worker); // Just ensure it was created successfully
    }
}
