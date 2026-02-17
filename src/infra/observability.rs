//! Prometheus metrics infrastructure for Grafana visibility.
//!
//! Provides wallet balance, error rates, queue depth, and HTTP traffic metrics.

use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::Arc;

/// Prometheus handle for on-demand scrape output (e.g. GET /metrics).
pub type PrometheusHandle = metrics_exporter_prometheus::PrometheusHandle;

/// Install the global metrics recorder and return a handle for rendering.
///
/// Uses `PrometheusBuilder` without an HTTP listener; the application
/// exposes metrics via GET /metrics using `handle.render()`.
///
/// # Errors
/// Returns an error if a recorder is already installed or building fails.
pub fn init_metrics() -> Result<PrometheusHandle, metrics_exporter_prometheus::BuildError> {
    let handle = PrometheusBuilder::new()
        .with_recommended_naming(true)
        .install_recorder()?;
    Ok(handle)
}

/// Convenience to wrap the handle in Arc for shared use in app state.
#[must_use]
pub fn init_metrics_handle() -> Option<Arc<PrometheusHandle>> {
    init_metrics().ok().map(Arc::new)
}
