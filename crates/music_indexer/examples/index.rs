//! CLI example for music_indexer
//!
//! Usage:
//! ```bash
//! cargo run -p music_indexer --example index /path/to/music
//! ```
//!
//! This example demonstrates how to use the MusicIndexer to scan a directory
//! and index all audio files into the music library.

use music_indexer::MusicIndexer;
use music_library::MusicLibrary;
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: cargo run -p music_indexer --example index <directory>");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  <directory>  Path to the directory to scan for audio files");
        eprintln!();
        eprintln!("Example:");
        eprintln!("  cargo run -p music_indexer --example index /Music");
        std::process::exit(1);
    }

    let dir_path = PathBuf::from(&args[1]);

    // Validate directory exists
    if !dir_path.exists() {
        eprintln!("Error: Directory does not exist: {}", dir_path.display());
        std::process::exit(1);
    }

    if !dir_path.is_dir() {
        eprintln!("Error: Path is not a directory: {}", dir_path.display());
        std::process::exit(1);
    }

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║           Music Indexer - CLI Example                     ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();
    println!("📁 Directory: {}", dir_path.display());
    println!();

    // Create library (using in-memory for this example)
    println!("📦 Initializing music library...");
    let library = MusicLibrary::open_in_memory().await?;
    println!("   ✓ Library initialized");
    println!();

    // Create indexer
    let indexer = MusicIndexer::new(library);

    // Run indexing
    println!("🔍 Scanning directory for audio files...");
    let start_time = std::time::Instant::now();
    
    let report = indexer.scan_directory(&dir_path).await?;
    
    let elapsed = start_time.elapsed();

    // Print results
    println!();
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║                  Indexing Results                         ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();
    println!("⏱️  Time elapsed: {:.2?}", elapsed);
    println!();
    println!("📊 Summary:");
    println!("   ✅ Added:   {} tracks", report.added);
    println!("   🔄 Updated: {} tracks", report.updated);
    println!("   ❌ Removed: {} tracks", report.removed);
    println!();

    if !report.errors.is_empty() {
        println!("⚠️  Errors ({}):", report.errors.len());
        for (path, error) in &report.errors {
            println!("   - {}: {}", path.display(), error);
        }
        println!();
    }

    // Print library summary
    let library = indexer.library();
    
    let total_tracks = library.get_tracks_count().await.unwrap_or(0);
    let artists = library.get_all_artists().await.unwrap_or_default();
    let albums = library.get_all_albums().await.unwrap_or_default();
    
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║                  Library Summary                          ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();
    println!("   🎵 Total tracks:  {}", total_tracks);
    println!("   🎤 Total artists: {}", artists.len());
    println!("   💿 Total albums:  {}", albums.len());
    println!();

    // Show some artists
    if !artists.is_empty() {
        println!("📋 Artists (first 10):");
        for artist in artists.iter().take(10) {
            println!("   • {}", artist.name);
        }
        if artists.len() > 10 {
            println!("   ... and {} more", artists.len() - 10);
        }
        println!();
    }

    // Show some albums
    if !albums.is_empty() {
        println!("💿 Albums (first 10):");
        for album in albums.iter().take(10) {
            let year = album.year
                .map(|y| format!(" ({})", y))
                .unwrap_or_default();
            println!("   • {}{}", album.title, year);
        }
        if albums.len() > 10 {
            println!("   ... and {} more", albums.len() - 10);
        }
        println!();
    }

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║                    ✓ Complete!                            ║");
    println!("╚═══════════════════════════════════════════════════════════╝");

    Ok(())
}
