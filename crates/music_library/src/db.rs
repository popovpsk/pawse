//! Database connection management.

use rusqlite::{Connection, Result as SqliteResult};
use std::path::Path;

use crate::migrations::Result as MigrationResult;

/// Create a new SQLite database connection.
pub fn create_connection<P: AsRef<Path>>(db_path: P) -> SqliteResult<Connection> {
    let conn = Connection::open(db_path)?;
    Ok(conn)
}

/// Create an in-memory database connection (for testing).
pub fn create_in_memory_connection() -> SqliteResult<Connection> {
    let conn = Connection::open_in_memory()?;
    Ok(conn)
}

/// Initialize database with optimal PRAGMA settings and run migrations.
pub fn init_db(conn: &mut Connection) -> MigrationResult<()> {
    crate::migrations::init_pragmas(conn)?;
    crate::migrations::run_migrations(conn)?;
    Ok(())
}
