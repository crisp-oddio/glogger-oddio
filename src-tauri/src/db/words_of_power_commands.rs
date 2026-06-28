use super::word_of_power_catalog;
use super::DbPool;
use serde::{Deserialize, Serialize};
use tauri::State;

// ── Output types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WordOfPower {
    pub id: i64,
    pub character_name: String,
    pub server_name: String,
    pub word: String,
    pub power_name: String,
    pub description: Option<String>,
    pub discovered_at: String,
    pub source: String,
    /// Looked up from the static catalog (`word_of_power_catalog`); "Unknown"
    /// for names not yet cataloged (e.g. manual entries with a custom name).
    pub category: String,
    /// Looked up from the static catalog; `None` if `power_name` is uncataloged.
    pub level: Option<u32>,
}

impl WordOfPower {
    fn with_catalog_lookup(
        id: i64,
        character_name: String,
        server_name: String,
        word: String,
        power_name: String,
        description: Option<String>,
        discovered_at: String,
        source: String,
    ) -> Self {
        let (category, level) = word_of_power_catalog::lookup(&power_name);
        Self {
            id,
            character_name,
            server_name,
            word,
            power_name,
            description,
            discovered_at,
            source,
            category: category.to_string(),
            level,
        }
    }
}

// ── Input types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddWordInput {
    pub word: String,
    pub power_name: String,
    pub description: Option<String>,
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_words_of_power(
    db: State<'_, DbPool>,
    character_name: String,
    server_name: String,
) -> Result<Vec<WordOfPower>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, character_name, server_name, word, power_name, description, discovered_at, source
             FROM words_of_power
             WHERE character_name = ?1 AND server_name = ?2
             ORDER BY discovered_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([&character_name, &server_name], |row| {
            Ok(WordOfPower::with_catalog_lookup(
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut words = Vec::new();
    for row in rows {
        words.push(row.map_err(|e| e.to_string())?);
    }
    Ok(words)
}

#[tauri::command]
pub fn add_word_of_power(
    db: State<'_, DbPool>,
    character_name: String,
    server_name: String,
    input: AddWordInput,
) -> Result<WordOfPower, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    conn.execute(
        "INSERT INTO words_of_power (character_name, server_name, word, power_name, description, discovered_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'manual')",
        rusqlite::params![
            &character_name,
            &server_name,
            &input.word,
            &input.power_name,
            &input.description,
            &now,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    Ok(WordOfPower::with_catalog_lookup(
        id,
        character_name,
        server_name,
        input.word,
        input.power_name,
        input.description,
        now,
        "manual".to_string(),
    ))
}

#[tauri::command]
pub fn delete_word_of_power(db: State<'_, DbPool>, id: i64) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM words_of_power WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── CSV import ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn import_words_of_power_csv(
    db: State<'_, DbPool>,
    character_name: String,
    server_name: String,
    file_path: String,
) -> Result<usize, String> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read CSV file: {e}"))?;

    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("Failed to read CSV headers: {e}"))?
        .clone();

    let col_word = find_header(&headers, &["Word", "word"])?;
    let col_power = find_header(&headers, &["Power Name", "power_name", "Power Name"])?;

    let col_date = find_header_opt(&headers, &["Date", "date"]);
    let col_time = find_header_opt(&headers, &["Time", "time"]);
    let col_desc = find_header_opt(&headers, &["Description", "description"]);

    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut imported = 0usize;

    for result in reader.records() {
        let record = result.map_err(|e| format!("Failed to read CSV record: {e}"))?;

        let word = record
            .get(col_word)
            .ok_or_else(|| "Missing Word field in CSV row".to_string())?
            .trim();
        let power_name = record
            .get(col_power)
            .ok_or_else(|| "Missing Power Name field in CSV row".to_string())?
            .trim();

        if word.is_empty() || power_name.is_empty() {
            continue;
        }

        let discovered_at = if let (Some(di), Some(ti)) = (col_date, col_time) {
            let date = record.get(di).unwrap_or("");
            let time = record.get(ti).unwrap_or("");
            let combined = format!("{} {}", date, time).trim().to_string();
            if combined.is_empty() || combined == " " {
                now.clone()
            } else {
                // Try parsing common date/time formats into ISO 8601
                parse_csv_datetime(&combined).unwrap_or_else(|| now.clone())
            }
        } else {
            now.clone()
        };

        let description = col_desc
            .and_then(|i| record.get(i))
            .filter(|s: &&str| !s.is_empty())
            .map(|s| s.to_string());

        conn.execute(
            "INSERT INTO words_of_power (character_name, server_name, word, power_name, description, discovered_at, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'csv-import')",
            rusqlite::params![
                &character_name,
                &server_name,
                word,
                power_name,
                description,
                &discovered_at,
            ],
        )
        .map_err(|e| format!("Failed to insert word '{}': {e}", word))?;

        imported += 1;
    }

    Ok(imported)
}

fn find_header(headers: &csv::StringRecord, candidates: &[&str]) -> Result<usize, String> {
    for c in candidates {
        if let Some(i) = headers.iter().position(|h| h.trim().eq_ignore_ascii_case(c)) {
            return Ok(i);
        }
    }
    Err(format!(
        "CSV is missing a required column. Expected one of: {}",
        candidates.join(", ")
    ))
}

fn find_header_opt(headers: &csv::StringRecord, candidates: &[&str]) -> Option<usize> {
    for c in candidates {
        if let Some(i) = headers.iter().position(|h| h.trim().eq_ignore_ascii_case(c)) {
            return Some(i);
        }
    }
    None
}

/// Best-effort parsing of common date+time formats into ISO 8601.
/// Accepts "MM/DD/YYYY HH:MM:SS AM/PM", "YYYY-MM-DD HH:MM:SS", etc.
fn parse_csv_datetime(s: &str) -> Option<String> {
    let s = s.trim();

    // Try common formats in order
    let formats = &[
        "%m/%d/%Y %I:%M:%S %p",
        "%m/%d/%Y %I:%M %p",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ];

    for fmt in formats {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        }
    }

    // If it's already ISO-like, return as-is
    if s.contains('T') && s.ends_with('Z') {
        return Some(s.to_string());
    }

    None
}

// ── Internal helper (called from coordinator, not a Tauri command) ──────────

pub fn insert_word_of_power(
    conn: &rusqlite::Connection,
    character_name: &str,
    server_name: &str,
    word: &str,
    power_name: &str,
    description: Option<&str>,
    discovered_at: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO words_of_power (character_name, server_name, word, power_name, description, discovered_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'auto')",
        rusqlite::params![character_name, server_name, word, power_name, description, discovered_at],
    )?;
    Ok(conn.last_insert_rowid())
}
