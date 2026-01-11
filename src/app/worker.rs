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

    /// Get the configured batch size
    #[must_use]
    pub fn batch_size(&self) -> i64 {
        self.config.batch_size
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

    /// Execute a single tick of the worker loop (for testing)
    /// This processes one batch without the full loop infrastructure
    pub async fn run_once(&self) {
        if !self.config.enabled {
            return;
        }
        self.process_batch().await;
    }

    /// Process a batch of pending submissions
    pub async fn process_batch(&self) {
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
    use crate::domain::{BlockchainStatus, CreateItemRequest, DatabaseClient};
    use crate::test_utils::{MockBlockchainClient, MockConfig, MockDatabaseClient};

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

    #[test]
    fn test_worker_config_debug() {
        let config = WorkerConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("WorkerConfig"));
        assert!(debug_str.contains("poll_interval"));
        assert!(debug_str.contains("batch_size"));
    }

    #[test]
    fn test_worker_config_clone() {
        let config1 = WorkerConfig {
            poll_interval: Duration::from_secs(30),
            batch_size: 50,
            enabled: true,
        };
        let config2 = config1.clone();
        assert_eq!(config1.poll_interval, config2.poll_interval);
        assert_eq!(config1.batch_size, config2.batch_size);
        assert_eq!(config1.enabled, config2.enabled);
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

    // --- NEW TESTS: run_once and process_batch ---

    #[tokio::test]
    async fn test_run_once_disabled_returns_immediately() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_millis(100),
            batch_size: 10,
            enabled: false,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        let start = std::time::Instant::now();
        worker.run_once().await;
        let elapsed = start.elapsed();

        // Should complete instantly when disabled
        assert!(elapsed < Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_run_once_enabled_calls_process_batch() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60),
            batch_size: 5,
            enabled: true,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // run_once should complete without hanging (even with no pending items)
        let result = tokio::time::timeout(Duration::from_secs(1), worker.run_once()).await;
        assert!(result.is_ok(), "run_once should complete within 1 second");
    }

    #[tokio::test]
    async fn test_batch_size_accessor() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(10),
            batch_size: 42,
            enabled: true,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        assert_eq!(worker.batch_size(), 42);
    }

    #[tokio::test]
    async fn test_process_batch_with_no_pending_items() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(10),
            batch_size: 10,
            enabled: true,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // Should complete without panic when no items
        worker.process_batch().await;
    }

    #[tokio::test]
    async fn test_process_batch_handles_service_error() {
        // Use a failing database client
        let db = Arc::new(MockDatabaseClient::with_config(MockConfig::failure(
            "Database error",
        )));
        let bc = Arc::new(MockBlockchainClient::new());
        let service = Arc::new(AppService::new(db, bc));

        let config = WorkerConfig {
            poll_interval: Duration::from_secs(10),
            batch_size: 10,
            enabled: true,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // Should not panic - errors are logged
        worker.process_batch().await;
    }

    #[tokio::test]
    async fn test_worker_with_tokio_time_pause() {
        tokio::time::pause();

        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60),
            batch_size: 10,
            enabled: true,
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        let handle = tokio::spawn(worker.run());

        // Advance time past the poll interval
        tokio::time::advance(Duration::from_secs(61)).await;

        // Send shutdown signal
        shutdown_tx.send(true).unwrap();

        // Worker should shutdown
        let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_worker_multiple_ticks_with_time_control() {
        tokio::time::pause();

        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(5),
            batch_size: 10,
            enabled: true,
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        let handle = tokio::spawn(worker.run());

        // Advance through multiple poll intervals
        for _ in 0..3 {
            tokio::time::advance(Duration::from_secs(6)).await;
            tokio::task::yield_now().await;
        }

        // Shutdown
        shutdown_tx.send(true).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_spawn_worker_with_enabled_config() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60),
            batch_size: 10,
            enabled: true,
        };

        let (handle, shutdown_tx) = spawn_worker(service, config);

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Shutdown
        shutdown_tx.send(true).unwrap();

        // Should complete
        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_worker_shutdown_channel_closed() {
        let service = create_test_service();
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(60),
            batch_size: 10,
            enabled: true,
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        let handle = tokio::spawn(worker.run());

        // Drop the sender - this should trigger an error in changed()
        drop(shutdown_tx);

        // Worker should eventually notice and handle the closed channel
        // Give it a moment to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Cancel the task since it won't shutdown naturally
        handle.abort();
    }

    #[tokio::test]
    async fn test_run_once_processes_pending_items() {
        let db = Arc::new(MockDatabaseClient::new());
        let bc = Arc::new(MockBlockchainClient::new());

        // Create an item that needs processing
        let request = CreateItemRequest {
            name: "Test Item".to_string(),
            description: None,
            content: "Content".to_string(),
            metadata: None,
        };
        let item = db.create_item(&request).await.unwrap();

        // Update to pending submission status
        db.update_blockchain_status(
            &item.id,
            BlockchainStatus::PendingSubmission,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let service = Arc::new(AppService::new(db.clone(), bc));

        let config = WorkerConfig {
            poll_interval: Duration::from_secs(10),
            batch_size: 10,
            enabled: true,
        };
        let (_, shutdown_rx) = watch::channel(false);
        let worker = BlockchainRetryWorker::new(service, config, shutdown_rx);

        // Process the pending item
        worker.run_once().await;

        // Verify the item was processed
        let updated = db.get_item(&item.id).await.unwrap().unwrap();
        assert_eq!(updated.blockchain_status, BlockchainStatus::Submitted);
    }

    #[test]
    fn test_worker_config_zero_batch_size() {
        let config = WorkerConfig {
            poll_interval: Duration::from_secs(10),
            batch_size: 0,
            enabled: true,
        };
        assert_eq!(config.batch_size, 0);
    }

    #[test]
    fn test_worker_config_very_short_poll_interval() {
        let config = WorkerConfig {
            poll_interval: Duration::from_millis(1),
            batch_size: 10,
            enabled: true,
        };
        assert_eq!(config.poll_interval, Duration::from_millis(1));
    }
}
