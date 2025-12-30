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
