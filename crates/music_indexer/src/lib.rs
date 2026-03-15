//! Music Indexer - Scans directories and indexes audio files into the music library
//!
//! This crate provides functionality to scan directories for audio files,
//! extract metadata using symphonia, and populate the music library database.
//!
//! # Example
//!
//! ```rust,no_run
//! use music_library::MusicLibrary;
//! use music_indexer::MusicIndexer;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Open the library
//!     let library = MusicLibrary::open("music_library.db").await?;
//!
//!     // Create indexer
//!     let indexer = MusicIndexer::new(library);
//!
//!     // Scan and index directories
//!     let report = indexer.scan_directory(&PathBuf::from("/Music")).await?;
//!
//!     println!("Added: {}, Updated: {}, Removed: {}", report.added, report.updated, report.removed);
//!
//!     Ok(())
//! }
//! ```

pub mod error;
pub mod indexer;
pub mod metadata;
pub mod scanner;

pub use error::{IndexerError, Result};
pub use indexer::{IndexReport, MusicIndexer};
pub use metadata::{extract_metadata, is_audio_file};
