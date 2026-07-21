//! Casino arena fight tracking.
//!
//! Persists resolved arena matches (who fought whom, and who won) parsed from
//! Kuzavek's `[NPC Chatter]` narration by [`crate::arena_parser`]. The dashboard
//! Arena widget reads the aggregate to rank fighters and predict the favorite in
//! any matchup — driven entirely from the chat log, with no manual tip entry.
//!
//! Only outcomes are stored; the player's own wager (a `Player.log` talk screen)
//! is not recorded here. Rows dedup on `(fought_at, fighter_a, fighter_b)` via a
//! unique index, so re-ingesting the same match from chat backfill is a no-op.

use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use tauri::State;

use super::DbPool;
use crate::arena_parser::ArenaTracker;
use crate::chat_parser::parse_chat_line;
use crate::settings::SettingsManager;

/// Per-fighter overall record.
#[derive(Debug, Serialize, Clone)]
pub struct ArenaFighter {
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    /// Win percentage 0..=100, one decimal of precision retained by the frontend.
    pub win_pct: f64,
}

/// One directed head-to-head cell: `fighter` vs `opponent`.
#[derive(Debug, Serialize, Clone)]
pub struct ArenaHeadToHead {
    pub fighter: String,
    pub opponent: String,
    pub wins: u32,
    pub losses: u32,
}

/// A recently-observed resolved match, newest first.
#[derive(Debug, Serialize, Clone)]
pub struct ArenaRecentMatch {
    pub fought_at: String,
    pub fighter_a: String,
    pub fighter_b: String,
    pub winner: String,
}

/// Aggregate arena history for the dashboard widget.
#[derive(Debug, Serialize, Clone, Default)]
pub struct ArenaStats {
    pub total_matches: u32,
    /// Fighters sorted by win percentage descending (ties by wins).
    pub fighters: Vec<ArenaFighter>,
    /// Every directed matchup with at least one observed result.
    pub head_to_head: Vec<ArenaHeadToHead>,
    /// Up to the last 15 matches, newest first.
    pub recent: Vec<ArenaRecentMatch>,
}

/// Persist a single resolved match. Idempotent via `idx_arena_dedup`.
pub fn record_arena_match(
    conn: &rusqlite::Connection,
    fought_at: &str,
    fighter_a: &str,
    fighter_b: &str,
    winner: &str,
) -> Result<usize, String> {
    conn.execute(
        "INSERT OR IGNORE INTO arena_matches (fought_at, fighter_a, fighter_b, winner)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![fought_at, fighter_a, fighter_b, winner],
    )
    .map_err(|e| format!("Failed to record arena match: {e}"))
}

/// Aggregate all persisted matches into fighter records, a head-to-head matrix,
/// and a recent-results list.
pub fn aggregate_stats(conn: &rusqlite::Connection) -> Result<ArenaStats, String> {
    // Pull every match once; the dataset is tiny (hundreds of rows at most).
    let mut stmt = conn
        .prepare(
            "SELECT fighter_a, fighter_b, winner FROM arena_matches",
        )
        .map_err(|e| format!("Query prepare error: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Query error: {e}"))?;

    use std::collections::HashMap;
    // name → (wins, losses)
    let mut overall: HashMap<String, (u32, u32)> = HashMap::new();
    // (winner, loser) → count
    let mut h2h: HashMap<(String, String), u32> = HashMap::new();
    let mut total = 0u32;

    for r in rows {
        let (a, b, winner) = r.map_err(|e| format!("Row error: {e}"))?;
        total += 1;
        let loser = if winner == a { b.clone() } else { a.clone() };
        overall.entry(winner.clone()).or_default().0 += 1;
        overall.entry(loser.clone()).or_default().1 += 1;
        *h2h.entry((winner, loser)).or_default() += 1;
    }

    let mut fighters: Vec<ArenaFighter> = overall
        .into_iter()
        .map(|(name, (wins, losses))| {
            let played = wins + losses;
            let win_pct = if played == 0 {
                0.0
            } else {
                (wins as f64 / played as f64) * 100.0
            };
            ArenaFighter {
                name,
                wins,
                losses,
                win_pct,
            }
        })
        .collect();
    fighters.sort_by(|x, y| {
        y.win_pct
            .partial_cmp(&x.win_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(y.wins.cmp(&x.wins))
            .then(x.name.cmp(&y.name))
    });

    // Build directed head-to-head cells from the win-count map (both directions).
    let mut cells: HashMap<(String, String), ArenaHeadToHead> = HashMap::new();
    for ((winner, loser), count) in &h2h {
        cells
            .entry((winner.clone(), loser.clone()))
            .or_insert_with(|| ArenaHeadToHead {
                fighter: winner.clone(),
                opponent: loser.clone(),
                wins: 0,
                losses: 0,
            })
            .wins += count;
        cells
            .entry((loser.clone(), winner.clone()))
            .or_insert_with(|| ArenaHeadToHead {
                fighter: loser.clone(),
                opponent: winner.clone(),
                wins: 0,
                losses: 0,
            })
            .losses += count;
    }
    let head_to_head: Vec<ArenaHeadToHead> = cells.into_values().collect();

    // Recent matches, newest first.
    let mut recent_stmt = conn
        .prepare(
            "SELECT fought_at, fighter_a, fighter_b, winner FROM arena_matches
             ORDER BY fought_at DESC, id DESC LIMIT 15",
        )
        .map_err(|e| format!("Query prepare error: {e}"))?;
    let recent = recent_stmt
        .query_map([], |row| {
            Ok(ArenaRecentMatch {
                fought_at: row.get(0)?,
                fighter_a: row.get(1)?,
                fighter_b: row.get(2)?,
                winner: row.get(3)?,
            })
        })
        .map_err(|e| format!("Query error: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Row error: {e}"))?;

    Ok(ArenaStats {
        total_matches: total,
        fighters,
        head_to_head,
        recent,
    })
}

/// Aggregate persisted arena outcomes for the dashboard widget.
#[tauri::command]
pub fn get_arena_stats(db: State<'_, DbPool>) -> Result<ArenaStats, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;
    aggregate_stats(&conn)
}

/// Read every `Chat-*.log` in the ChatLogs directory and persist any arena
/// matches found. Idempotent (unique index). Returns rows inserted.
///
/// A fresh [`ArenaTracker`] is driven line-by-line per file. Chat logs are read
/// oldest-to-newest within each file; intros and results interleave with all
/// other chatter, which the tracker filters to Kuzavek's `NPC Chatter` lines.
pub fn backfill_from_chat_logs(
    settings: &SettingsManager,
    db: &DbPool,
) -> Result<usize, String> {
    let Some(dir) = settings.get_chat_logs_dir() else {
        return Ok(0);
    };
    if !dir.is_dir() {
        return Ok(0);
    }

    let mut conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    // Read files in name order so timestamps advance monotonically across the
    // backfill (Chat-YY-MM-DD.log sorts chronologically).
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read ChatLogs dir: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("Chat-") && n.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();

    let mut inserted = 0usize;
    // BEGIN IMMEDIATE, not the default DEFERRED: this runs on startup alongside
    // other backfills, and a deferred transaction that upgrades read→write on
    // its first INSERT gets an instant SQLITE_BUSY (deadlock avoidance) that the
    // connection's busy_timeout can't retry. Acquiring the write lock up front
    // makes busy_timeout apply, so we wait for contention instead of erroring.
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("Failed to begin transaction: {e}"))?;

    for path in paths {
        let Ok(file) = File::open(&path) else { continue };
        // Each daily log is its own arena session; a match never straddles two
        // files, so a per-file tracker avoids stale pairings across a midnight
        // gap.
        let mut tracker = ArenaTracker::new();
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let Some(msg) = parse_chat_line(&line) else {
                continue;
            };
            let ts = msg.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
            if let Some(m) = tracker.observe(
                msg.channel.as_deref(),
                msg.sender.as_deref(),
                &msg.message,
                &ts,
            ) {
                inserted += tx
                    .execute(
                        "INSERT OR IGNORE INTO arena_matches
                            (fought_at, fighter_a, fighter_b, winner)
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![m.fought_at, m.fighter_a, m.fighter_b, m.winner],
                    )
                    .map_err(|e| format!("Insert error: {e}"))?;
            }
        }
    }

    tx.commit().map_err(|e| format!("Commit error: {e}"))?;
    Ok(inserted)
}

/// Tauri command wrapper around [`backfill_from_chat_logs`].
#[tauri::command]
pub fn backfill_arena_from_chat_logs(
    settings: State<'_, Arc<SettingsManager>>,
    db: State<'_, DbPool>,
) -> Result<usize, String> {
    backfill_from_chat_logs(&settings, &db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE arena_matches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fought_at TEXT NOT NULL,
                fighter_a TEXT NOT NULL,
                fighter_b TEXT NOT NULL,
                winner TEXT NOT NULL
            );
            CREATE UNIQUE INDEX idx_arena_dedup
                ON arena_matches(fought_at, fighter_a, fighter_b);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn record_is_idempotent() {
        let conn = setup();
        for _ in 0..3 {
            record_arena_match(&conn, "2026-07-20 20:52:59", "Otis", "Leo", "Leo").unwrap();
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM arena_matches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "same match re-recorded must dedup via unique index");
    }

    #[test]
    fn aggregates_records_and_h2h() {
        let conn = setup();
        // Leo beats Otis twice; Otis beats Leo once.
        record_arena_match(&conn, "2026-07-20 10:00:00", "Otis", "Leo", "Leo").unwrap();
        record_arena_match(&conn, "2026-07-20 11:00:00", "Leo", "Otis", "Leo").unwrap();
        record_arena_match(&conn, "2026-07-20 12:00:00", "Otis", "Leo", "Otis").unwrap();

        let stats = aggregate_stats(&conn).unwrap();
        assert_eq!(stats.total_matches, 3);

        let leo = stats.fighters.iter().find(|f| f.name == "Leo").unwrap();
        assert_eq!((leo.wins, leo.losses), (2, 1));
        let otis = stats.fighters.iter().find(|f| f.name == "Otis").unwrap();
        assert_eq!((otis.wins, otis.losses), (1, 2));

        // Higher win pct sorts first.
        assert_eq!(stats.fighters.first().unwrap().name, "Leo");

        // Directed H2H: Leo vs Otis = 2-1; Otis vs Leo = 1-2.
        let lo = stats
            .head_to_head
            .iter()
            .find(|c| c.fighter == "Leo" && c.opponent == "Otis")
            .unwrap();
        assert_eq!((lo.wins, lo.losses), (2, 1));
        let ol = stats
            .head_to_head
            .iter()
            .find(|c| c.fighter == "Otis" && c.opponent == "Leo")
            .unwrap();
        assert_eq!((ol.wins, ol.losses), (1, 2));
    }

    #[test]
    fn empty_aggregate() {
        let conn = setup();
        let stats = aggregate_stats(&conn).unwrap();
        assert_eq!(stats.total_matches, 0);
        assert!(stats.fighters.is_empty());
        assert!(stats.head_to_head.is_empty());
    }
}
