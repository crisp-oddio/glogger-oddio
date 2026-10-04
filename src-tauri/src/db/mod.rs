use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, Result};
use std::path::PathBuf;

pub mod admin_commands;
pub mod aggregate_commands;
pub mod arena_commands;
pub mod brewing_commands;
pub mod build_planner_commands;
pub mod cdn_persistence;
pub mod character_commands;
pub mod chat_commands;
pub mod combat_wisdom_commands;
pub mod crafting_commands;
pub mod death_commands;
pub mod farming_commands;
pub mod kill_tracking_commands;
pub mod resuscitate_commands;
pub mod roulette_commands;
pub mod game_state_commands;
pub mod gourmand_commands;
pub mod inventory_commands;
pub mod market_commands;
pub mod message_commands;
pub mod migrations;
pub mod player_commands;
pub mod poem_commands;
#[allow(dead_code)]
pub mod price_helper_commands;
pub mod queries;
pub mod stall_tracker_commands;
// Reads survey_types (CDN-populated reference table) — useful raw material
// for the future Analytics rebuild. No active consumer right now since the
// legacy survey screen was removed; allow dead_code until that work lands.
#[allow(dead_code)]
pub mod survey_commands;
pub mod timer_commands;
pub mod hoplology_commands;
pub mod word_of_power_catalog;
pub mod words_of_power_commands;

pub type DbConnection = r2d2::PooledConnection<SqliteConnectionManager>;

/// A pool pair for a WAL-mode SQLite database: a read pool (multiple
/// connections — WAL readers never block the writer) and a single-connection
/// write pool. All writers queue in the app on `get_write()` instead of
/// fighting over SQLite's write lock via `busy_timeout`, which cannot queue
/// fairly and starves under back-to-back holders.
///
/// `get()` returns a read connection; `get_write()` returns the dedicated
/// write connection. Signatures keep taking `&DbPool`, so routing a call site
/// is a one-word change (`get()` → `get_write()`).
#[derive(Clone)]
pub struct DbPool {
    reads: r2d2::Pool<SqliteConnectionManager>,
    writes: r2d2::Pool<SqliteConnectionManager>,
}

impl DbPool {
    /// Test/dev constructor: build a pool pair around existing pools.
    #[cfg(test)]
    pub fn from_pools(
        reads: r2d2::Pool<SqliteConnectionManager>,
        writes: r2d2::Pool<SqliteConnectionManager>,
    ) -> Self {
        Self { reads, writes }
    }

    /// Get a read connection (concurrent with the writer and other readers).
    pub fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, r2d2::Error> {
        self.reads.get()
    }

    /// Get the dedicated write connection. Max one exists, so concurrent
    /// writers queue here in the app (fair FIFO via r2d2) rather than racing
    /// SQLite's busy handler.
    pub fn get_write(
        &self,
    ) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, r2d2::Error> {
        self.writes.get()
    }

    /// Wait-for-writer helper: blocks until the write connection is
    /// free and returns nothing. Used by startup steps that want to
    /// ensure prior writes are drained without doing any themselves.
    pub fn drain_writes(&self) -> Result<(), r2d2::Error> {
        let _guard = self.writes.get()?;
        Ok(())
    }
}

/// Initialize the database pool pair with the given path.
/// `tz_offset_seconds` is needed for one-time migration to fix historical timestamps.
pub fn init_pool(db_path: PathBuf, tz_offset_seconds: Option<i32>) -> Result<DbPool, Box<dyn std::error::Error>> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
                 PRAGMA busy_timeout=5000;
                 PRAGMA synchronous=NORMAL;
                 PRAGMA foreign_keys=ON;",
        )
    });

    // Writes: exactly one connection — app-side serialization of every writer.
    let writes = r2d2::Pool::builder()
        .max_size(1)
        .min_idle(Some(1))
        .build(manager)?;

    // Reads: concurrent in WAL mode, never block the writer.
    let reads = r2d2::Pool::builder()
        .max_size(12)
        .build(SqliteConnectionManager::file(&db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                     PRAGMA busy_timeout=5000;
                     PRAGMA synchronous=NORMAL;
                     PRAGMA foreign_keys=ON;",
            )
        }))?;

    // Run migrations on the write connection (schema changes are writes).
    {
        let conn = writes.get()?;
        migrations::run_migrations(&conn, tz_offset_seconds)?;
    } // Released back to the write pool here.

    Ok(DbPool { reads, writes })
}

/// Get current schema version
pub fn get_schema_version(conn: &Connection) -> Result<i32> {
    let version: i32 = conn
        .query_row(
            "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(version)
}

/// Record that a migration was applied
fn record_migration(conn: &Connection, version: i32) -> Result<()> {
    conn.execute(
        "INSERT INTO schema_migrations (version) VALUES (?1)",
        [version],
    )?;
    Ok(())
}
