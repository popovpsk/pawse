//! Music library management with SQLite storage.
//!
//! This crate provides:
//! - Domain models for artists, albums, and tracks
//! - Many-to-many relationships (multiple artists per track/album)
//! - SQLite storage with migrations
//! - Repository pattern for data access

pub mod db;
pub mod migrations;
pub mod models;
pub mod repositories;

pub use migrations::{run_migrations, init_pragmas};
pub use models::*;
pub use repositories::*;
