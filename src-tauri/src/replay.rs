/// Dual-log replay — simulates live tailing by interleaving Player.log and Chat.log
/// events by timestamp, processing them through the same coordinator pipelines.
///
/// This enables cross-referencing between the two log streams (e.g., correcting
/// motherlode loot quantities from Chat.log [Status] messages) using archived logs.
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::cdn_commands::GameDataState;
use crate::chat_parser::{
    is_timestamped_line, parse_chat_line, parse_chat_login_line, ChatMessage,
};
use crate::chat_status_parser::parse_status_message;
use crate::db::DbPool;
use crate::game_state::GameStateManager;
use crate::parsers::{chat_local_to_utc, parse_skill_update, parse_timestamp};
use crate::player_event_parser::{PlayerEvent, PlayerEventParser};
use crate::survey::aggregator::SurveySessionAggregator;

/// A timestamped event from either log source, used for interleaving.
#[derive(Debug)]
enum TimedEvent {
    /// Events from Player.log (processed first within a second)
    PlayerLine {
        /// UTC second (for ordering)
        utc_second: i64,
        /// The raw log line
        line: String,
    },
    /// A chat message from Chat.log
    ChatMessage {
        /// UTC second (for ordering)
        utc_second: i64,
        msg: ChatMessage,
    },
    /// Login detected from Chat.log — carries timezone offset
    ChatLogin {
        /// UTC second (for ordering)
        utc_second: i64,
        server_name: String,
        character_name: String,
        timezone_offset_seconds: Option<i32>,
    },
}

impl TimedEvent {
    fn utc_second(&self) -> i64 {
        match self {
            TimedEvent::PlayerLine { utc_second, .. } => *utc_second,
            TimedEvent::ChatMessage { utc_second, .. } => *utc_second,
            TimedEvent::ChatLogin { utc_second, .. } => *utc_second,
        }
    }

    /// Sort key: (utc_second, source_order)
    /// source_order: 0 = ChatLogin (timezone must come first), 1 = PlayerLine, 2 = ChatMessage
    fn sort_key(&self) -> (i64, u8) {
        match self {
            TimedEvent::ChatLogin { utc_second, .. } => (*utc_second, 0),
            TimedEvent::PlayerLine { utc_second, .. } => (*utc_second, 1),
            TimedEvent::ChatMessage { utc_second, .. } => (*utc_second, 2),
        }
    }
}

/// Progress event emitted to the frontend during replay.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReplayProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub detail: String,
}

/// Replay result summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReplayResult {
    pub player_lines_processed: usize,
    pub chat_messages_processed: usize,
    pub player_events_emitted: usize,
    pub chat_status_events_emitted: usize,
}

/// Parse Player.log into timestamped lines.
///
/// Player.log timestamps are local time `[HH:MM:SS]` with no date.
/// Player.log timestamps are already UTC with no date. We derive the date from
/// the chat log filename or fall back to today's UTC date.
fn parse_player_log_lines(
    path: &PathBuf,
    base_date: chrono::NaiveDate,
) -> Result<Vec<TimedEvent>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open Player.log: {}", e))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {}", e))?;
        let line = line.trim_end().to_string();
        if line.is_empty() {
            continue;
        }

        // Extract [HH:MM:SS] timestamp — already UTC
        if let Some(ts_str) = parse_timestamp(&line) {
            if let Ok(utc_time) = chrono::NaiveTime::parse_from_str(&ts_str, "%H:%M:%S") {
                let utc_dt = base_date.and_time(utc_time);
                let utc_second = utc_dt.and_utc().timestamp();

                events.push(TimedEvent::PlayerLine { utc_second, line });
            }
        }
        // Lines without timestamps (login announcements, etc.) get appended
        // with the same second as the previous event
        else if !events.is_empty() {
            let prev_second = events.last().unwrap().utc_second();
            events.push(TimedEvent::PlayerLine {
                utc_second: prev_second,
                line,
            });
        }
    }

    Ok(events)
}

/// Parse Chat.log into timestamped events.
/// Also extracts login lines for timezone/server detection.
fn parse_chat_log_events(path: &PathBuf) -> Result<Vec<TimedEvent>, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open Chat.log: {}", e))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read Chat.log: {}", e))?;

    let mut events = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Check for login line first (carries timezone offset)
        if let Some(info) = parse_chat_login_line(line) {
            // Login lines have a timestamp too — extract it
            let utc_second = if is_timestamped_line(line) {
                if let Some(msg) = parse_chat_line(line) {
                    msg.timestamp.and_utc().timestamp()
                } else {
                    0
                }
            } else {
                0
            };

            events.push(TimedEvent::ChatLogin {
                utc_second,
                server_name: info.server_name,
                character_name: info.character_name,
                timezone_offset_seconds: info.timezone_offset_seconds,
            });
            continue;
        }

        // Regular chat message
        if is_timestamped_line(line) {
            if let Some(msg) = parse_chat_line(line) {
                let utc_second = msg.timestamp.and_utc().timestamp();
                events.push(TimedEvent::ChatMessage { utc_second, msg });
            }
        }
    }

    Ok(events)
}

/// Extract a date from a chat log filename like "Chat-26-03-27.log"
fn date_from_chat_filename(path: &PathBuf) -> Option<chrono::NaiveDate> {
    let stem = path.file_stem()?.to_str()?;
    // "Chat-YY-MM-DD"
    let date_part = stem.strip_prefix("Chat-")?;
    chrono::NaiveDate::parse_from_str(date_part, "%y-%m-%d").ok()
}

/// Extract the date from the first chat message timestamp in the file.
/// Fallback when the filename doesn't follow the Chat-YY-MM-DD pattern.
fn date_from_chat_content(events: &[TimedEvent]) -> Option<chrono::NaiveDate> {
    for event in events {
        if let TimedEvent::ChatMessage { msg, .. } = event {
            return Some(msg.timestamp.date());
        }
        if let TimedEvent::ChatLogin { utc_second, .. } = event {
            if *utc_second > 0 {
                let dt = chrono::DateTime::from_timestamp(*utc_second, 0)?;
                return Some(dt.date_naive());
            }
        }
    }
    None
}

/// Core replay logic — processes both logs through the full coordinator pipeline.
fn run_replay(
    player_log_path: PathBuf,
    chat_log_path: PathBuf,
    app: &AppHandle,
    db: &DbPool,
    game_data: GameDataState,
) -> Result<ReplayResult, String> {
    // --- Phase 1: Pre-scan chat log for timezone offset ---
    app.emit(
        "replay-progress",
        ReplayProgress {
            phase: "scanning".into(),
            current: 0,
            total: 2,
            detail: "Scanning chat log for timezone info...".into(),
        },
    )
    .ok();

    let chat_events = parse_chat_log_events(&chat_log_path)?;

    // Find the first timezone offset from login lines
    let mut tz_offset: i32 = 0;
    for event in &chat_events {
        if let TimedEvent::ChatLogin {
            timezone_offset_seconds: Some(offset),
            ..
        } = event
        {
            tz_offset = *offset;
            break;
        }
    }

    // Derive base date from chat log filename, chat content, or today
    let base_date = date_from_chat_filename(&chat_log_path)
        .or_else(|| date_from_chat_content(&chat_events))
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    eprintln!(
        "[replay] Base date: {}, timezone offset: {}s",
        base_date, tz_offset
    );

    // --- Phase 2: Parse Player.log with correct timezone ---
    app.emit(
        "replay-progress",
        ReplayProgress {
            phase: "scanning".into(),
            current: 1,
            total: 2,
            detail: "Parsing Player.log...".into(),
        },
    )
    .ok();

    let player_events = parse_player_log_lines(&player_log_path, base_date)?;

    // --- Phase 3: Apply timezone offset to chat events and merge ---
    // Chat.log timestamps are local time; convert to UTC using the detected offset.
    let chat_events: Vec<TimedEvent> = chat_events
        .into_iter()
        .map(|event| match event {
            TimedEvent::ChatMessage { msg, .. } => {
                let mut msg = msg;
                msg.timestamp = chat_local_to_utc(msg.timestamp, tz_offset);
                let utc_second = msg.timestamp.and_utc().timestamp();
                TimedEvent::ChatMessage { utc_second, msg }
            }
            TimedEvent::ChatLogin {
                server_name,
                character_name,
                timezone_offset_seconds,
                ..
            } => {
                // Recalculate utc_second with offset applied
                TimedEvent::ChatLogin {
                    utc_second: 0, // Login lines sort first regardless
                    server_name,
                    character_name,
                    timezone_offset_seconds,
                }
            }
            other => other,
        })
        .collect();

    let total_events = player_events.len() + chat_events.len();
    let mut all_events: Vec<TimedEvent> = Vec::with_capacity(total_events);
    all_events.extend(player_events);
    all_events.extend(chat_events);

    // Stable sort: ChatLogin first (timezone), then PlayerLine, then ChatMessage
    all_events.sort_by_key(|e| e.sort_key());

    // Diagnostic: show first/last timestamps from each source
    if let Some(first_player) = all_events
        .iter()
        .find(|e| matches!(e, TimedEvent::PlayerLine { .. }))
    {
        if let Some(last_player) = all_events
            .iter()
            .rev()
            .find(|e| matches!(e, TimedEvent::PlayerLine { .. }))
        {
            eprintln!(
                "[replay] Player.log UTC range: {} .. {}",
                first_player.utc_second(),
                last_player.utc_second()
            );
        }
    }
    if let Some(first_chat) = all_events
        .iter()
        .find(|e| matches!(e, TimedEvent::ChatMessage { .. }))
    {
        if let Some(last_chat) = all_events
            .iter()
            .rev()
            .find(|e| matches!(e, TimedEvent::ChatMessage { .. }))
        {
            eprintln!(
                "[replay] Chat.log UTC range: {} .. {}",
                first_chat.utc_second(),
                last_chat.utc_second()
            );
        }
    }

    // --- Phase 4: Process through coordinator pipeline ---
    let mut player_parser = PlayerEventParser::new();
    let mut game_state = GameStateManager::new(game_data.clone());
    game_state.set_base_date(base_date);
    let mut survey_aggregator = SurveySessionAggregator::new(game_data);
    survey_aggregator.set_base_date(base_date);

    let mut result = ReplayResult {
        player_lines_processed: 0,
        chat_messages_processed: 0,
        player_events_emitted: 0,
        chat_status_events_emitted: 0,
    };

    let progress_interval = (total_events / 100).max(50); // emit ~100 progress events

    // Use the same batching strategy as the live coordinator to avoid
    // flooding the Windows message queue. Accumulate player events and
    // domain updates, then flush in consolidated emissions. This
    // dramatically reduces the number of PostMessage calls compared to
    // per-event emission.
    const BATCH_MAX_SIZE: usize = 50;
    const BATCH_MAX_AGE: Duration = Duration::from_millis(100);

    let mut player_event_batch: Vec<PlayerEvent> = Vec::new();
    let mut domains_batch: Vec<&'static str> = Vec::new();
    let mut batch_start = Instant::now();
    let mut emits_since_yield: u32 = 0;
    let mut last_yield = Instant::now();

    /// Flush accumulated player events and domain updates to the frontend.
    /// Yields briefly after each flush so the message queue can drain.
    macro_rules! flush_batches {
        ($app:expr, $gs:expr, $pe:expr, $dom:expr, $start:expr, $emits:expr, $last_yield:expr) => {
            if !$pe.is_empty() {
                let batch_result = $gs.process_events_batch(&$pe, &db);
                $dom.extend(batch_result.domains_updated);

                $app.emit("player-events-batch", &$pe).ok();
                $emits += 1;
                $pe.clear();
            }
            if !$dom.is_empty() {
                $dom.sort_unstable();
                $dom.dedup();
                $app.emit("game-state-updated", &$dom).ok();
                $emits += 1;
                $dom.clear();
            }
            $start = Instant::now();

            // Yield so the webview JS event loop can process the batch
            if $emits >= 4 {
                std::thread::sleep(Duration::from_millis(15));
                $last_yield = Instant::now();
                $emits = 0;
            }
        };
    }

    for (i, event) in all_events.iter().enumerate() {
        // Progress updates
        if i % progress_interval == 0 {
            app.emit(
                "replay-progress",
                ReplayProgress {
                    phase: "processing".into(),
                    current: i,
                    total: total_events,
                    detail: format!("Processing event {}/{}", i, total_events),
                },
            )
            .ok();
        }

        match event {
            TimedEvent::ChatLogin {
                server_name,
                character_name,
                ..
            } => {
                // Flush pending batches before identity change
                flush_batches!(app, game_state, player_event_batch, domains_batch, batch_start, emits_since_yield, last_yield);

                game_state.set_active_character_name(character_name);
                game_state.set_active_server_name(server_name);

                app.emit("character-login", character_name).ok();
                app.emit("server-detected", server_name).ok();
                emits_since_yield += 2;
            }

            TimedEvent::PlayerLine { line, .. } => {
                result.player_lines_processed += 1;

                // Skill updates (legacy)
                if let Some(update) = parse_skill_update(line) {
                    app.emit("skill-update", &update).ok();
                    emits_since_yield += 1;
                }

                // Player events
                let mut p_events = player_parser.process_line(line);

                // Survey aggregator runs first so any survey_use_id it
                // injects into provenance reaches item_transactions via
                // game_state. Matches the live coordinator ordering.
                let active_char = game_state.get_active_character().map(String::from);
                let active_server = game_state.get_active_server().map(String::from);
                for pe in p_events.iter_mut() {
                    if let (Some(character), Some(server), Ok(conn)) =
                        (&active_char, &active_server, db.get())
                    {
                        let _ = survey_aggregator.process_event(pe, &conn, character, server, None);
                    }
                }

                result.player_events_emitted += p_events.len();
                player_event_batch.extend(p_events);
            }

            TimedEvent::ChatMessage { msg, .. } => {
                result.chat_messages_processed += 1;

                // Status channel → ChatStatusParser
                if let Some(status_event) = parse_status_message(msg) {
                    app.emit("chat-status-event", &status_event).ok();
                    emits_since_yield += 1;
                    result.chat_status_events_emitted += 1;
                }
            }
        }

        // Flush when batch is full or old enough
        if player_event_batch.len() >= BATCH_MAX_SIZE
            || (!player_event_batch.is_empty() && batch_start.elapsed() >= BATCH_MAX_AGE)
        {
            flush_batches!(app, game_state, player_event_batch, domains_batch, batch_start, emits_since_yield, last_yield);
        }

        // Extra yield if we've been emitting a lot of non-batched events
        // (skill-update, chat-status-event, character-login, etc.)
        if emits_since_yield >= 20 && last_yield.elapsed() < Duration::from_millis(50) {
            std::thread::sleep(Duration::from_millis(15));
            last_yield = Instant::now();
            emits_since_yield = 0;
        } else if last_yield.elapsed() >= Duration::from_millis(50) {
            last_yield = Instant::now();
            emits_since_yield = 0;
        }
    }

    // Flush any remaining batched events
    flush_batches!(app, game_state, player_event_batch, domains_batch, batch_start, emits_since_yield, last_yield);
    // Suppress unused-assignment warnings from the final macro expansion
    let _ = (batch_start, emits_since_yield, last_yield);

    // Flush pending player events from the parser itself
    let flush_events = player_parser.flush_all_pending();
    if !flush_events.is_empty() {
        let batch_result = game_state.process_events_batch(&flush_events, db);
        app.emit("player-events-batch", &flush_events).ok();
        result.player_events_emitted += flush_events.len();

        if !batch_result.domains_updated.is_empty() {
            let mut domains = batch_result.domains_updated;
            domains.sort_unstable();
            domains.dedup();
            app.emit("game-state-updated", &domains).ok();
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    // Final progress
    app.emit(
        "replay-progress",
        ReplayProgress {
            phase: "complete".into(),
            current: total_events,
            total: total_events,
            detail: format!(
                "Done: {} player events, {} chat messages",
                result.player_events_emitted, result.chat_messages_processed,
            ),
        },
    )
    .ok();

    Ok(result)
}


// ============================================================
// Tauri Command
// ============================================================

/// Replay both a Player.log and Chat.log file through the full coordinator pipeline,
/// interleaved by timestamp. This simulates live tailing with cross-referencing.
#[tauri::command]
pub async fn replay_dual_logs(
    player_log_path: String,
    chat_log_path: String,
    app: AppHandle,
) -> Result<ReplayResult, String> {
    let player_path = PathBuf::from(&player_log_path);
    let chat_path = PathBuf::from(&chat_log_path);

    if !player_path.exists() {
        return Err(format!("Player.log not found: {}", player_log_path));
    }
    if !chat_path.exists() {
        return Err(format!("Chat.log not found: {}", chat_log_path));
    }

    let db = app.state::<DbPool>().inner().clone();
    let game_data = app.state::<GameDataState>().inner().clone();

    // Run on a blocking thread since file I/O is synchronous
    let result = tokio::task::spawn_blocking(move || {
        run_replay(player_path, chat_path, &app, &db, game_data)
    })
    .await
    .map_err(|e| format!("Replay task failed: {}", e))??;

    Ok(result)
}

// ============================================================
// Historical kill/loot backfill (silent — no frontend events)
// ============================================================

/// Result of a historical ingest of a Player.log file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IngestResult {
    /// Lootable searched corpses recorded as kills (the drop-rate denominator).
    pub kills_added: usize,
    /// Loot rows attributed to those kills.
    pub loot_added: usize,
    /// True when this exact file content was already ingested before (no-op).
    pub already_ingested: bool,
}

/// FNV-1a hash of file contents — cheap, dependency-free idempotency key.
fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Parse a Player.log login line, returning `(character_name, base_date)`.
///
/// Format: `[HH:MM:SS] Logged in as character <Name>. Time UTC=MM/DD/YYYY HH:MM:SS. Timezone Offset ±HH:MM:SS`
/// Player.log is self-sufficient — no Chat.log pairing is needed: the character
/// and the UTC base date both come from this line.
fn parse_player_login_line(line: &str) -> Option<(String, chrono::NaiveDate)> {
    let marker = "Logged in as character ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    // Name runs up to the first '.' (character names contain no periods).
    let dot = rest.find('.')?;
    let character_name = rest[..dot].trim().trim_matches(['[', ']']).to_string();
    if character_name.is_empty() {
        return None;
    }

    let utc_marker = "Time UTC=";
    let utc_start = line.find(utc_marker)? + utc_marker.len();
    let utc_rest = &line[utc_start..];
    // Date is the first whitespace-delimited token: "MM/DD/YYYY".
    let date_token = utc_rest.split_whitespace().next()?;
    let base_date = chrono::NaiveDate::parse_from_str(date_token, "%m/%d/%Y").ok()?;
    Some((character_name, base_date))
}

/// Silently ingest lootable kills + loot from a single Player.log into the
/// lifetime database, deduped against live data via the enemy_kills UNIQUE
/// index. Emits no frontend events and does not touch live game state.
///
/// The drop-rate model is corpse-search based: each lootable `Search Corpse of
/// X` (permission granted) is one kill row keyed by the corpse entity_id (the
/// FIRST search's timestamp), and loot is attributed by matching the same corpse
/// entity_id. Loot dedup is by the item's `instance_id` so two separate
/// single-stacks of the same item off one corpse stay as two rows. Player.log is
/// self-sufficient (character + UTC date from the login line) — no Chat.log.
pub fn ingest_kill_loot_from_logs(
    player_log_path: PathBuf,
    db: &DbPool,
) -> Result<IngestResult, String> {
    let player_bytes =
        std::fs::read(&player_log_path).map_err(|e| format!("Failed to read Player.log: {e}"))?;
    let hash = content_hash(&player_bytes);

    let conn = db.get().map_err(|e| format!("DB connection error: {e}"))?;

    // Idempotency: skip if this exact Player.log content was already ingested.
    let already: bool = conn
        .query_row(
            "SELECT 1 FROM player_prev_ingests WHERE content_hash = ?1",
            [&hash],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if already {
        return Ok(IngestResult {
            kills_added: 0,
            loot_added: 0,
            already_ingested: true,
        });
    }

    // First login line gives the character + UTC base date used to timestamp
    // every event. Without it we can't produce killed_at parity with the live
    // path, so bail (nothing to ingest).
    let content = String::from_utf8_lossy(&player_bytes);
    let (character_name, base_date) = match content
        .lines()
        .find_map(parse_player_login_line)
    {
        Some(v) => v,
        None => {
            return Ok(IngestResult {
                kills_added: 0,
                loot_added: 0,
                already_ingested: false,
            });
        }
    };
    // Server isn't recorded in Player.log; mirror the live path's fallback so the
    // dedup key matches when a server-less session is involved.
    let server_name = "Unknown".to_string();

    let player_events = parse_player_log_lines(&player_log_path, base_date)?;

    // --- Pass A: collect searched corpses (with permission) + their loot ---
    let mut parser = PlayerEventParser::new();
    // corpse_entity_id -> (corpse_name, first-search killed_at, zone, combat_skills)
    let mut corpses: std::collections::HashMap<u32, (String, String, Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    // ordered loot: (corpse_entity_id, item_name, quantity, instance_id)
    let mut loot: Vec<(u32, String, u32, i64)> = Vec::new();
    // Current zone (internal area key) tracked from `LOADING LEVEL <area>` lines,
    // mirroring the live tailer, so each corpse is tagged with where it was searched.
    let mut current_zone: Option<String> = None;
    // Current equipped combat-skill loadout, tracked the same way as the live path
    // (ProcessSetActiveSkills / ProcessLoadAbilities), so each corpse is tagged with it.
    let mut current_combat_skills: Option<String> = None;

    for event in &player_events {
        let line = match event {
            TimedEvent::PlayerLine { line, .. } => line,
            _ => continue,
        };
        if let Some(idx) = line.find("LOADING LEVEL ") {
            let area = line[idx + "LOADING LEVEL ".len()..].trim();
            if !area.is_empty() {
                current_zone = Some(area.to_string());
            }
        }
        for ev in parser.process_line(line) {
            match ev {
                PlayerEvent::ActiveSkillsChanged { skill1, skill2, .. }
                | PlayerEvent::AbilitiesLoaded { skill1, skill2, .. } => {
                    current_combat_skills =
                        crate::coordinator::normalize_combat_loadout(&skill1, &skill2);
                }
                PlayerEvent::CorpseSearched {
                    timestamp,
                    corpse_entity_id,
                    corpse_name,
                    has_permission,
                } => {
                    if !has_permission {
                        continue;
                    }
                    // Keep the FIRST search per corpse (parity with the live path).
                    corpses.entry(corpse_entity_id).or_insert((
                        corpse_name,
                        timestamp,
                        current_zone.clone(),
                        current_combat_skills.clone(),
                    ));
                }
                PlayerEvent::LootPickedUp {
                    corpse_entity_id: Some(cid),
                    item_name: Some(item),
                    instance_id,
                    quantity,
                    ..
                } => {
                    if !item.is_empty() {
                        loot.push((cid, item, quantity, instance_id as i64));
                    }
                }
                _ => {}
            }
        }
    }

    // --- Pass B: persist kills, then attribute loot ---
    let mut result = IngestResult {
        kills_added: 0,
        loot_added: 0,
        already_ingested: false,
    };

    // corpse_entity_id -> kill_id (row id of the persisted kill)
    let mut kill_ids: std::collections::HashMap<u32, i64> = std::collections::HashMap::new();

    for (entity_id, (corpse_name, killed_at, zone, combat_skills)) in &corpses {
        let entity_id_str = entity_id.to_string();
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO enemy_kills
                    (enemy_name, enemy_entity_id, killing_ability,
                     health_damage, armor_damage, killed_at, character_name, server_name, zone, combat_skills)
                 VALUES (?1, ?2, '', 0, 0, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    corpse_name,
                    entity_id_str,
                    killed_at,
                    character_name,
                    server_name,
                    zone,
                    combat_skills,
                ],
            )
            .map_err(|e| format!("Failed to insert kill: {e}"))?;

        // Resolve the kill_id whether we inserted or an earlier ingest/live row
        // already covered this corpse — loot is deduped by instance_id either way.
        let kill_id = if inserted > 0 {
            result.kills_added += 1;
            conn.last_insert_rowid()
        } else {
            conn.query_row(
                "SELECT id FROM enemy_kills
                 WHERE character_name IS ?1 AND server_name IS ?2
                   AND enemy_entity_id = ?3 AND killed_at = ?4",
                rusqlite::params![character_name, server_name, entity_id_str, killed_at],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(-1)
        };
        if kill_id >= 0 {
            kill_ids.insert(*entity_id, kill_id);
        }
    }

    for (entity_id, item_name, quantity, instance_id) in &loot {
        let kill_id = match kill_ids.get(entity_id) {
            Some(id) => *id,
            None => continue, // loot from a corpse we didn't (or couldn't) record
        };
        if conn
            .execute(
                "INSERT OR IGNORE INTO enemy_kill_loot
                    (kill_id, item_name, quantity, instance_id)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![kill_id, item_name, quantity, instance_id],
            )
            .map(|n| n > 0)
            .unwrap_or(false)
        {
            result.loot_added += 1;
        }
    }

    // Record this file as ingested (idempotency).
    conn.execute(
        "INSERT OR REPLACE INTO player_prev_ingests
            (content_hash, source_path, ingested_at, kills_added, loot_added)
         VALUES (?1, ?2, CURRENT_TIMESTAMP, ?3, ?4)",
        rusqlite::params![
            hash,
            player_log_path.to_string_lossy(),
            result.kills_added as i64,
            result.loot_added as i64,
        ],
    )
    .map_err(|e| format!("Failed to record ingest: {e}"))?;

    Ok(result)
}

/// Spawn a background thread that backs up Player-prev.log into the lifetime
/// kill/loot database whenever the game rotates it (mtime change). Ingestion is
/// idempotent (content-hash guarded) and deduped against live data, so this is
/// safe to run alongside live tailing. Honors the `auto_ingest_player_prev`
/// setting, re-checked each tick so toggling it takes effect without a restart.
pub fn spawn_player_prev_watcher(
    settings: std::sync::Arc<crate::settings::SettingsManager>,
    db: DbPool,
) {
    std::thread::spawn(move || {
        // Let startup settle before the first scan.
        std::thread::sleep(Duration::from_secs(10));
        let mut last_mtime: Option<std::time::SystemTime> = None;

        loop {
            if settings.get_auto_ingest_player_prev() {
                if let Some(prev_path) = settings.get_player_prev_log_path() {
                    if let Ok(mtime) = std::fs::metadata(&prev_path).and_then(|m| m.modified()) {
                        if last_mtime != Some(mtime) {
                            last_mtime = Some(mtime);
                            match ingest_kill_loot_from_logs(prev_path.clone(), &db) {
                                Ok(r) if !r.already_ingested => {
                                    eprintln!(
                                        "[ingest] Player-prev backfill: +{} kills, +{} loot",
                                        r.kills_added, r.loot_added
                                    );
                                }
                                Ok(_) => {}
                                Err(e) => eprintln!("[ingest] Player-prev backfill failed: {e}"),
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

/// Tauri command: ingest a single Player.log into the lifetime kill/loot
/// database. Used for manual backfill of kept backups and by the automatic
/// Player-prev.log rotation watcher. Player.log is self-sufficient — no Chat.log
/// is needed.
#[tauri::command]
pub async fn ingest_player_log(
    player_log_path: String,
    app: AppHandle,
) -> Result<IngestResult, String> {
    let player_path = PathBuf::from(&player_log_path);
    if !player_path.exists() {
        return Err(format!("Player.log not found: {player_log_path}"));
    }

    let db = app.state::<DbPool>().inner().clone();
    let result = tokio::task::spawn_blocking(move || {
        ingest_kill_loot_from_logs(player_path, &db)
    })
    .await
    .map_err(|e| format!("Ingest task failed: {e}"))??;

    Ok(result)
}

#[cfg(test)]
mod ingest_tests {
    use super::*;
    use std::io::Write;

    /// Build a fresh migrated DB at a unique temp path.
    fn temp_pool() -> (DbPool, PathBuf) {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("glogger_ingest_test_{nanos}.db"));
        let pool = crate::db::init_pool(path.clone(), Some(0)).expect("init pool");
        (pool, path)
    }

    fn write_log(lines: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("glogger_ingest_test_{nanos}.log"));
        let mut f = std::fs::File::create(&path).expect("create log");
        f.write_all(lines.as_bytes()).expect("write log");
        path
    }

    #[test]
    fn corpse_search_ingest_logs_every_lootable_kill_and_keeps_duplicate_stacks() {
        // Corpse 500 (permission): two SEPARATE single-stacks of the same item
        // off one corpse (distinct instance ids 111/222) must stay two rows.
        // Corpse 600 (no permission): not the player's kill — excluded.
        // Corpse 700 (permission): a lootable kill that dropped nothing — still
        // logged (the "log every lootable kill" requirement).
        let log = "\
[15:41:09] Logged in as character TestPlayer. Time UTC=04/17/2026 15:41:09. Timezone Offset 00:00:00
[15:42:00] LocalPlayer: ProcessTalkScreen(500, \"Search Corpse of Goblin\", \"\", \"\", System.Int32[], System.String[], 0, Corpse)
[15:42:01] LocalPlayer: ProcessAddItem(HealthPotion(111), -1, True)
[15:42:01] LocalPlayer: ProcessRemoveLoot(111)
[15:42:02] LocalPlayer: ProcessAddItem(HealthPotion(222), -1, True)
[15:42:02] LocalPlayer: ProcessRemoveLoot(222)
[15:43:00] LocalPlayer: ProcessTalkScreen(600, \"Search Corpse of Wolf\", \"(You do not have permission to loot this corpse.)\", \"\", System.Int32[], System.String[], 0, Corpse)
[15:44:00] LocalPlayer: ProcessTalkScreen(700, \"Search Corpse of Rat\", \"\", \"\", System.Int32[], System.String[], 0, Corpse)
";
        let log_path = write_log(log);
        let (pool, db_path) = temp_pool();

        let r = ingest_kill_loot_from_logs(log_path.clone(), &pool).expect("ingest");
        assert!(!r.already_ingested);
        // Two permission corpses (Goblin + Rat); the no-permission Wolf excluded.
        assert_eq!(r.kills_added, 2, "lootable kills (permission corpses)");
        // Both single-stacks survive instance_id dedup.
        assert_eq!(r.loot_added, 2, "two distinct item instances");

        let conn = pool.get().unwrap();
        let kills: i64 = conn
            .query_row("SELECT COUNT(*) FROM enemy_kills", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kills, 2);
        // The Goblin's two HealthPotion instances are two distinct loot rows.
        let goblin_loot: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM enemy_kill_loot l
                 JOIN enemy_kills k ON k.id = l.kill_id
                 WHERE k.enemy_entity_id = '500'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(goblin_loot, 2, "duplicate single-stacks not collapsed");

        // Re-ingesting the same file is a content-hash no-op.
        let r2 = ingest_kill_loot_from_logs(log_path.clone(), &pool).expect("re-ingest");
        assert!(r2.already_ingested);
        assert_eq!(r2.kills_added, 0);
        assert_eq!(r2.loot_added, 0);

        drop(conn);
        let _ = std::fs::remove_file(&log_path);
        let _ = std::fs::remove_file(&db_path);
    }
}
