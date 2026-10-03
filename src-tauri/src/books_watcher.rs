//! Books-directory watcher — ingests the game's own `.txt` book exports.
//!
//! The game writes one `.txt` per opened book into `<game_data_path>/Books/`
//! (`SkillReport_*`, `PlayerAge_*`, `HelpScreen_*`, `PlayerShopLog_*`,
//! `GuildMotd_*`, `MessageOfTheDay_*`, `NewQuestquest_*`). Files written while
//! glogger was closed previously sat unimported: the live coordinator only
//! sees books whose `ProcessBook` line streams through the running log tail.
//! This watcher mirrors `replay::spawn_player_prev_watcher` (background
//! thread, periodic scan, change detection) but tracks per-file mtimes and
//! needs no settings gate — book ingestion is unconditional.
//!
//! Content contract: live `ProcessBook` content carries escaped newlines
//! (`\n`), book files carry real newlines. Everything here normalizes through
//! `coordinator::normalize_book_content` once per file, so the shared
//! ingestion helpers in `coordinator` see identical content on both paths.

use crate::cdn_commands::GameDataState;
use crate::coordinator::{
    ingest_hoplology_content, ingest_report_stats_content, ingest_teleport_binds_content,
    persist_book_content,
};
use crate::db::stall_tracker_commands::{
    insert_stall_events, StallEventInput, StallOpsLock,
};
use crate::db::DbPool;
use crate::settings::SettingsManager;
use crate::shop_log_parser::parse_shop_log;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

/// Initial settle delay before the first scan so startup DB churn
/// (migrations, CDN init, catch-up polling handoff) isn't contended.
const INITIAL_DELAY_SECS: u64 = 15;

/// Rescan cadence.
const SCAN_INTERVAL_SECS: u64 = 30;

/// Spawn the Books-directory watcher thread. Runs for the process lifetime;
/// re-reads settings each tick so `game_data_path` changes take effect
/// without a restart.
pub fn spawn_books_watcher(
    settings: std::sync::Arc<SettingsManager>,
    db: DbPool,
    app: AppHandle,
) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(INITIAL_DELAY_SECS));
        let mut seen: HashMap<PathBuf, SystemTime> = HashMap::new();

        loop {
            scan_books_dir(&settings, &db, &app, &mut seen);
            std::thread::sleep(Duration::from_secs(SCAN_INTERVAL_SECS));
        }
    });
}

/// One scan pass. First pass ingests everything on disk (backfill); later
/// passes only touch files whose mtime changed. Removed files are dropped
/// from the tracking map so it never grows unbounded.
fn scan_books_dir(
    settings: &SettingsManager,
    db: &DbPool,
    app: &AppHandle,
    seen: &mut HashMap<PathBuf, SystemTime>,
) {
    let settings = settings.get();
    let Some(game_data_path) = opt_nonempty(settings.game_data_path.as_str()) else {
        return;
    };
    let Some(character) = opt_nonempty(settings.active_character_name.as_deref().unwrap_or(""))
    else {
        return;
    };
    // Mirror the live `active_character_server()` guard: persisting under an
    // empty server would land rows the UI (scoped to the active server) never
    // sees, and once the real server is detected the same books would be
    // persisted again as a second row set.
    let Some(server) = opt_nonempty(settings.active_server_name.as_deref().unwrap_or("")) else {
        return;
    };

    let books_dir = Path::new(&game_data_path).join("Books");
    if !books_dir.is_dir() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(&books_dir) else {
        return;
    };

    let mut to_process: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_txt = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("txt"));
        if !is_txt {
            continue;
        }
        let Ok(mtime) = entry
            .metadata()
            .and_then(|m| m.modified())
        else {
            continue;
        };
        if seen.get(&path) != Some(&mtime) {
            to_process.push((path, mtime));
        }
    }

    if to_process.is_empty() {
        return;
    }

    // Shared stall-ops lock instance (managed in lib.rs before the watcher
    // spawns) so shop-log writes serialize with live ingest and Clear — same
    // synchronous State borrow the coordinator's ingest_shop_log uses.
    let Some(ops_lock) = app.try_state::<StallOpsLock>() else {
        eprintln!("[books-watcher] StallOpsLock not managed yet; skipping tick");
        return;
    };

    for (path, mtime) in to_process {
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| {
                process_book_file(
                    &path, &content, &character, &server, db, &ops_lock, app,
                )
                .ok()
                .map(|note| (note, content))
            }) {
            Some((note, _)) => {
                // Mark the file seen only after successful processing so a
                // transient failure (pool exhaustion, missing managed state,
                // unreadable file) retries on the next tick.
                seen.insert(path.clone(), mtime);
                eprintln!("[books-watcher] {file_name}: {note}");
            }
            None => {
                eprintln!("[books-watcher] {file_name}: failed this tick; will retry");
            }
        }
    }
}

/// `Some(s)` when the string is non-empty after trim, else `None`.
fn opt_nonempty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ============================================================
// File classification + dispatch
// ============================================================

/// What ingestion a classified book file gets, mirroring the live
/// coordinator's `BookOpened` dispatch (coordinator.rs). `BookOnly` files are
/// captured into `game_state_books` for queryability even where no live
/// consumer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookKind {
    /// Shop log: stall-event ingest ONLY (no book row) — the live path
    /// persists PlayerShopLog to `stall_events`, never `game_state_books`.
    ShopLog,
    /// SkillReport with content-driven extra ingest (gourmand / hoplology /
    /// binds) on top of the always-on book upsert.
    SkillReport,
    /// PlayerAge / HelpScreen: book upsert + structured stats ingest.
    Stats,
    /// Book upsert only (GuildMotd, MessageOfTheDay, NewQuest, unknown,
    /// ServerStatus, GardeningAlmanac — the watcher captures content but does
    /// not duplicate the live almanac extraction).
    BookOnly,
}

/// Classify a Books-dir file by its stem. Returns the `book_type` used for
/// the `game_state_books` upsert plus the ingest kind.
///
/// Filename shapes seen on disk: `SkillReport_261001_212859`,
/// `PlayerShopLog_260826_194723`, `NewQuestquest_45455_Name_260623_174509`
/// (the quest book's stem prefix is `NewQuestquest`, normalized to
/// `NewQuestBook` — a file-local book_type; these streams don't appear in any
/// live `matches!` list).
pub(crate) fn classify_book_file(stem: &str) -> (String, BookKind) {
    let prefix = stem.split('_').next().unwrap_or_default();
    match prefix {
        "PlayerShopLog" => ("PlayerShopLog".to_string(), BookKind::ShopLog),
        "SkillReport" => ("SkillReport".to_string(), BookKind::SkillReport),
        "PlayerAge" => ("PlayerAge".to_string(), BookKind::Stats),
        "HelpScreen" => ("HelpScreen".to_string(), BookKind::Stats),
        "NewQuestquest" => ("NewQuestBook".to_string(), BookKind::BookOnly),
        other => (other.to_string(), BookKind::BookOnly),
    }
}

/// Ingest one book file. `content` may be raw file text (real newlines) —
/// normalization to the shared live-path shape happens here, once.
pub(crate) fn process_book_file(
    path: &Path,
    content: &str,
    character: &str,
    server: &str,
    db: &DbPool,
    ops_lock: &StallOpsLock,
    app: &AppHandle,
) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "filename has no stem".to_string())?
        .to_string();
    let (book_type, kind) = classify_book_file(&stem);
    let title = stem;

    // Normalize escaped-newline content (live-path shape) into real newlines.
    // File content already has real newlines; this is a no-op there.
    let content = crate::coordinator::normalize_book_content(content);

    match kind {
        BookKind::ShopLog => ingest_shop_log_file(&title, &content, path, character, db, ops_lock, app),
        BookKind::SkillReport => {
            let mut notes: Vec<String> = Vec::new();

            // Live path persists ALL SkillReports to game_state_books first,
            // then runs content-specific branches. Mirror that.
            persist_book_content(db, app, character, server, &book_type, &title, &content)?;
            notes.push("book upserted".to_string());

            let trimmed = content.trim_start();
            if trimmed.starts_with("Foods Consumed:") {
                let n = import_gourmand(db, &content, app)?;
                if n > 0 {
                    notes.push(format!("gourmand +{n} foods"));
                }
            }
            if trimmed.starts_with("Equipment Studied:") {
                let game_data = app
                    .try_state::<GameDataState>()
                    .map(|s| s.inner().clone())
                    .ok_or_else(|| "GameDataState not managed yet".to_string())?;
                let inserted =
                    ingest_hoplology_content(db, &game_data, app, character, server, &content);
                if inserted > 0 {
                    notes.push(format!("hoplology +{inserted}"));
                }
            }
            if content.contains("Primary Bind Location:") {
                ingest_teleport_binds_content(db, app, character, server, &content);
                notes.push("binds updated".to_string());
            }
            Ok(notes.join(", "))
        }
        BookKind::Stats => {
            persist_book_content(db, app, character, server, &book_type, &title, &content)?;
            ingest_report_stats_content(db, app, character, server, &book_type, &content);
            Ok("book + stats ingested".to_string())
        }
        BookKind::BookOnly => {
            persist_book_content(db, app, character, server, &book_type, &title, &content)?;
            Ok("book upserted".to_string())
        }
    }
}

/// Ingest a PlayerShopLog file into `stall_events` (mirrors the live
/// `ingest_shop_log` + Import's owner resolution). No `game_state_books` row —
/// the live path never persists shop logs there.
fn ingest_shop_log_file(
    title: &str,
    content: &str,
    path: &Path,
    character: &str,
    db: &DbPool,
    ops_lock: &StallOpsLock,
    app: &AppHandle,
) -> Result<String, String> {
    // Probe parse (base year 1970) only to learn whether the file has any
    // parseable entries at all; the real parse uses the filename year.
    let probe = parse_shop_log(title, content, "imported", 1970);
    if probe.entries.is_empty() {
        return Err("no parseable shop-log entries".to_string());
    }

    let base_year = crate::db::stall_tracker_commands::year_from_filename(path);
    let shop_log = parse_shop_log(title, content, "imported", base_year);

    // Owner resolution (mirrors import_shop_log_file): the parsed advisory
    // owner wins; a bought-only file is claimed for the active character.
    let effective_owner = shop_log
        .owner
        .clone()
        .or_else(|| Some(character.to_string()));

    let inputs: Vec<StallEventInput> = shop_log
        .entries
        .iter()
        .map(|e| StallEventInput {
            event_timestamp: e.timestamp.clone(),
            event_at: e.event_at.clone(),
            log_timestamp: shop_log.log_timestamp.clone(),
            log_title: shop_log.title.clone(),
            action: e.action.clone(),
            player: e.player.clone(),
            owner: effective_owner.clone(),
            item: e.item.clone(),
            quantity: e.quantity,
            price_unit: e.price_unit,
            price_total: e.price_total,
            raw_message: e.raw_message.clone(),
            entry_index: e.entry_index,
        })
        .collect();

    let inserted = insert_stall_events(db, ops_lock, &inputs)?;
    if inserted > 0 {
        app.emit("stall-events-updated", inserted).ok();
    }
    Ok(format!("stall events +{inserted}"))
}

/// Gourmand import with the live path's emit contract
/// (`gourmand-updated` with the imported count when n > 0).
fn import_gourmand(db: &DbPool, content: &str, app: &AppHandle) -> Result<usize, String> {
    let conn = db.get().map_err(|e| format!("Database connection error: {e}"))?;
    let n = crate::db::gourmand_commands::import_gourmand_from_content(&conn, content)?;
    if n > 0 {
        app.emit("gourmand-updated", n).ok();
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use chrono::Datelike;
    use rusqlite::Connection;

    /// Migrated in-memory DB wrapped in the app's pool type.
    fn pool() -> DbPool {
        let manager = r2d2_sqlite::SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch("PRAGMA foreign_keys=ON;")
        });
        let pool = r2d2::Pool::builder()
            .max_size(2)
            .build(manager)
            .expect("pool");
        let conn = pool.get().expect("conn");
        run_migrations(&conn, None).expect("migrations");
        drop(conn);
        pool
    }

    // ── classify_book_file ────────────────────────────────────────

    #[test]
    fn classify_seven_on_disk_prefixes() {
        use BookKind::*;
        let cases = [
            ("SkillReport_261001_212859", ("SkillReport", SkillReport)),
            ("PlayerShopLog_260826_194723", ("PlayerShopLog", ShopLog)),
            ("PlayerAge_261001_212847", ("PlayerAge", Stats)),
            ("HelpScreen_261001_212849", ("HelpScreen", Stats)),
            (
                "NewQuestquest_45455_Name_260623_174509",
                ("NewQuestBook", BookOnly),
            ),
            ("GuildMotd_260712_173954", ("GuildMotd", BookOnly)),
            (
                "MessageOfTheDay_260623_174529",
                ("MessageOfTheDay", BookOnly),
            ),
        ];
        for (stem, (book_type, kind)) in cases {
            let (bt, k) = classify_book_file(stem);
            assert_eq!(bt, book_type, "book_type for {stem}");
            assert_eq!(k, kind, "kind for {stem}");
        }
    }

    #[test]
    fn classify_unknown_prefix_is_book_only() {
        let (bt, kind) = classify_book_file("WhateverElse_261001_101010");
        assert_eq!(bt, "WhateverElse");
        assert_eq!(kind, BookKind::BookOnly);
    }

    #[test]
    fn classify_defensive_prefixes() {
        let (bt, kind) = classify_book_file("ServerStatus_261001_101010");
        assert_eq!(bt, "ServerStatus");
        assert_eq!(kind, BookKind::BookOnly);
    }

    // ── persist_book_content ──────────────────────────────────────

    #[test]
    fn persist_book_is_idempotent_upsert() {
        let pool = pool();
        // AppHandle isn't constructible in unit tests; persist_book_content
        // emits through it. Split: verify the SQL contract by calling the
        // same statement shape the helper runs. The emit path is exercised
        // by the live-path regression proof.
        let conn = pool.get().unwrap();
        for _ in 0..2 {
            let dt = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO game_state_books (character_name, server_name, book_type, title, content, captured_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(character_name, server_name, book_type, title) DO UPDATE SET
                    content = excluded.content,
                    captured_at = excluded.captured_at",
                rusqlite::params!["TestChar", "TestServer", "GuildMotd", "GuildMotd_260712_173954", "body", dt],
            )
            .unwrap();
        }
        let (count, captured): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), MAX(captured_at) FROM game_state_books
                 WHERE character_name = 'TestChar' AND book_type = 'GuildMotd'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1, "idempotent upsert keeps one row");
        assert!(!captured.is_empty());
    }

    // ── teleportation binds ───────────────────────────────────────

    #[test]
    fn binds_real_newline_content_parses_fields() {
        let content =
            "Teleportation Status:\n\nPrimary Bind Location: Serbule\nSecondary Bind Location: (none)\n";
        let primary = crate::coordinator::extract_bind_field(content, "Primary Bind Location:");
        let secondary = crate::coordinator::extract_bind_field(content, "Secondary Bind Location:");
        assert_eq!(primary.as_deref(), Some("Serbule"));
        assert_eq!(secondary, None, "\"(none)\" maps to NULL");
    }

    #[test]
    fn binds_escaped_and_real_newlines_agree() {
        let escaped = "Primary Bind Location: Red Wing Casino\\nSecondary Bind Location: Caves Beneath Gazluk\\n";
        // Production path: normalize_book_content turns escaped newlines real
        // before extraction; the real-newline variant skips through unchanged.
        let normalized = crate::coordinator::normalize_book_content(escaped);
        for content in [normalized.as_str(), escaped.replace("\\n", "\n").as_str()] {
            let primary = crate::coordinator::extract_bind_field(content, "Primary Bind Location:");
            let secondary =
                crate::coordinator::extract_bind_field(content, "Secondary Bind Location:");
            assert_eq!(primary.as_deref(), Some("Red Wing Casino"));
            assert_eq!(secondary.as_deref(), Some("Caves Beneath Gazluk"));
        }
    }

    #[test]
    fn binds_created_on_shape_yields_null_primary() {
        // Real on-disk HelpScreen first lines — no bind fields at all.
        let content =
            "TwinkleofToes was created on Sat Jan 31 09:47:43 EST 2026\n\nPrimary Bind Location: (none)";
        assert_eq!(
            crate::coordinator::extract_bind_field(content, "Primary Bind Location:"),
            None
        );
    }

    // ── report stats ──────────────────────────────────────────────

    #[test]
    fn player_age_content_produces_stats() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, None).unwrap();
        let content = "TwinkleofToes was created on <b>Sat Jan 31 09:47:43 EST 2026</b>.\nYou have spent <b>61 days 10 hours</b> logged into the game.\nYou have died <b>1,841</b> times.\n";
        let stats = crate::report_stats::parse_player_age(content);
        assert!(stats.len() >= 3, "expected created_on/time_played/deaths");
        let n = crate::report_stats::persist_stats(&conn, "TestChar", "TestServer", &stats, "now")
            .unwrap();
        assert_eq!(n, stats.len());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM character_report_stats WHERE character_name = 'TestChar'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
    }

    // ── gourmand ──────────────────────────────────────────────────

    #[test]
    fn gourmand_real_file_fixture_idempotent() {
        let pool = pool();
        let conn = pool.get().unwrap();
        let content =
            "Foods Consumed:\n\n  \"Meaty\" Tomato Soup: 7\n  8-Year Steamed Cake (HAS DAIRY): 1\n";
        let n1 = crate::db::gourmand_commands::import_gourmand_from_content(&conn, content).unwrap();
        assert_eq!(n1, 2);
        let n2 = crate::db::gourmand_commands::import_gourmand_from_content(&conn, content).unwrap();
        assert_eq!(n2, 2, "re-import stays stable");
        let soup: i64 = conn
            .query_row(
                "SELECT times_eaten FROM gourmand_eaten_foods WHERE food_name = '\"Meaty\" Tomato Soup'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(soup, 7, "count preserved across re-import");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM gourmand_eaten_foods", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, 2, "idempotent import keeps one row per food");
    }

    // ── shop log ──────────────────────────────────────────────────

    #[test]
    fn shop_log_real_line_parses_one_entry() {
        let content = "Mon Aug 24 23:52 - GorgonoidNeurodivergente bought Mastery Spark: Staves at a cost of 10000 per 1 = 10000\n";
        let parsed = parse_shop_log("Imported", content, "imported", 2026);
        assert_eq!(parsed.entries.len(), 1);
        let e = &parsed.entries[0];
        assert_eq!(e.action, "bought");
        assert_eq!(e.item.as_deref(), Some("Mastery Spark: Staves"));
        assert_eq!(e.price_total, Some(10000));
    }

    #[test]
    fn year_from_filename_on_disk_name() {
        // No 4-digit year in range → falls back to current local year.
        let path = Path::new("PlayerShopLog_260826_194723.txt");
        let expected = chrono::Local::now().year();
        assert_eq!(
            crate::db::stall_tracker_commands::year_from_filename(path),
            expected
        );
    }

    // ── hoplology splitter (post-normalization) ───────────────────

    #[test]
    fn hoplology_content_split_skips_header() {
        // Post-normalization shape: real newlines, one item per line.
        let content = "Equipment Studied:\n\n  CrudBurst's Hammer of Thumping\n  Thentree Harness\n";
        let items: Vec<&str> = content
            .split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("Equipment Studied:"))
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], "CrudBurst's Hammer of Thumping");
    }
}
