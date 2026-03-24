//! Migration runner for SQLite database schema updates.
//!
//! Migrations are stored in the `migrations/` directory and applied in order
//! based on their filename prefix (e.g., `001_*.sql`, `002_*.sql`).

use rusqlite::{Connection, Result as SqliteResult};
use std::fs;
use std::path::{Path, PathBuf};

/// Current schema version. Increment when adding new migrations.
pub const CURRENT_SCHEMA_VERSION: i32 = 1;

/// Error type for migration operations.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Migration file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Invalid migration version in file: expected {expected}, found {found}")]
    InvalidVersion { expected: i32, found: i32 },

    #[error("Migration already applied: version {0}")]
    AlreadyApplied(i32),
}

pub type Result<T> = std::result::Result<T, MigrationError>;

/// Get the path to the migrations directory.
pub fn migrations_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

/// Get all migration files sorted by version.
fn get_migration_files(migrations_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(migrations_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();

    files.sort();
    Ok(files)
}

/// Extract version number from migration filename (e.g., "001_*.sql" -> 1).
fn extract_version(filename: &Path) -> Option<i32> {
    filename
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|name| name.split('_').next())
        .and_then(|prefix| prefix.parse::<i32>().ok())
}

/// Get the current schema version from the database.
fn get_current_version(conn: &Connection) -> SqliteResult<i32> {
    // Check if schema_version table exists
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM sqlite_master 
            WHERE type='table' AND name='schema_version'
        )",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        return Ok(0);
    }

    conn.query_row("SELECT MAX(version) FROM schema_version", [], |row| {
        row.get(0)
    })
    .or(Ok(0))
}

/// Apply a single migration file.
fn apply_migration(conn: &mut Connection, path: &Path, version: i32) -> Result<()> {
    let sql = fs::read_to_string(path)?;

    // Execute the migration in a transaction
    let tx = conn.transaction()?;
    tx.execute_batch(&sql)?;

    // Update schema version
    tx.execute(
        "INSERT OR REPLACE INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        [version],
    )?;

    tx.commit()?;

    log::info!("Applied migration version {}: {:?}", version, path);
    Ok(())
}

/// Run all pending migrations on the database.
pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    run_migrations_with_dir(conn, &migrations_dir())
}

/// Run all pending migrations from a specific directory.
pub fn run_migrations_with_dir(conn: &mut Connection, migrations_dir: &Path) -> Result<()> {
    if !migrations_dir.exists() {
        return Err(MigrationError::FileNotFound(migrations_dir.to_path_buf()));
    }

    let current_version = get_current_version(conn).map_err(MigrationError::from)?;
    log::info!("Current schema version: {}", current_version);

    let migration_files = get_migration_files(migrations_dir)?;

    for path in migration_files {
        let version = extract_version(&path)
            .ok_or_else(|| MigrationError::FileNotFound(path.clone()))?;

        // Skip already applied migrations
        if version <= current_version {
            continue;
        }

        apply_migration(conn, &path, version)?;
    }

    let final_version = get_current_version(conn).map_err(MigrationError::from)?;
    log::info!("Schema updated to version: {}", final_version);

    Ok(())
}

/// Initialize database with PRAGMA settings for optimal performance.
pub fn init_pragmas(conn: &mut Connection) -> SqliteResult<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = -64000;
        PRAGMA temp_store = MEMORY;
        PRAGMA foreign_keys = ON;
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version(Path::new("001_initial.sql")), Some(1));
        assert_eq!(extract_version(Path::new("002_add_column.sql")), Some(2));
        assert_eq!(extract_version(Path::new("10_update.sql")), Some(10));
        assert_eq!(extract_version(Path::new("invalid.sql")), None);
    }

    #[test]
    fn test_run_migrations() -> Result<()> {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut conn = Connection::open(&db_path)?;

        // Create temp migrations dir
        let migrations_dir = dir.path().join("migrations");
        fs::create_dir(&migrations_dir).unwrap();

        // Create a test migration that includes schema_version table
        let migration_path = migrations_dir.join("001_test.sql");
        let mut file = File::create(&migration_path).unwrap();
        writeln!(
            file,
            r#"
            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY CHECK (version = 1),
                applied_at DATETIME NOT NULL DEFAULT (datetime('now'))
            );
            
            CREATE TABLE test_table (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL
            );
            
            INSERT INTO schema_version (version) VALUES (1);
        "#
        )
        .unwrap();

        run_migrations_with_dir(&mut conn, &migrations_dir)?;

        // Verify table was created
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='test_table')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists);

        Ok(())
    }
}
