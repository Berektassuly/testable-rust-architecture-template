//! Concrete database client implementations.
//!
//! This module contains production-ready database adapters that implement
//! the `DatabaseClient` trait defined in the domain layer.

pub mod postgres;

pub use postgres::PostgresDatabase;