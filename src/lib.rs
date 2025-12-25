//! Testable Rust Architecture Template

pub mod api;
pub mod app;
pub mod domain;
pub mod infra;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
