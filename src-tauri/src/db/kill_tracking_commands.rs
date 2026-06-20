/// Kill tracking queries — enemy kill stats and loot drop rates.
///
/// Three data scopes:
/// - "mine": the player's own observed kills (`enemy_kills` / `enemy_kill_loot`)
/// - "imported": aggregated data imported from other players' exports
///   (`imported_enemy_kills_agg` / `imported_enemy_kill_loot_agg`), tagged by
///   `source_label` so a re-import of the same file replaces rather than
///   double-counts
/// - "combined": mine + imported, summed
use super::DbPool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use tauri::State;

#[derive(Serialize)]
pub struct EnemyLootDrop {
    pub item_name: String,
    pub total_quantity: i64,
    pub times_dropped: i64,
    /// How many kills had this item drop (times_dropped / total_kills)
    pub drop_rate: f64,
}

#[derive(Serialize)]
pub struct EnemyKillStats {
    pub enemy_name: String,
    pub total_kills: i64,
    pub loot: Vec<EnemyLootDrop>,
}

fn mine_total_kills(conn: &rusqlite::Connection, enemy_name: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM enemy_kills WHERE enemy_name = ?1",
        [enemy_name],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn imported_total_kills(conn: &rusqlite::Connection, enemy_name: &str) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(total_kills), 0) FROM imported_enemy_kills_agg WHERE enemy_name = ?1",
        [enemy_name],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn mine_loot_rows(conn: &rusqlite::Connection, enemy_name: &str) -> Vec<(String, i64, i64)> {
    let mut stmt = match conn.prepare(
        "SELECT l.item_name, SUM(l.quantity), COUNT(DISTINCT l.kill_id)
         FROM enemy_kill_loot l
         JOIN enemy_kills k ON l.kill_id = k.id
         WHERE k.enemy_name = ?1
         GROUP BY l.item_name",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([enemy_name], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

fn imported_loot_rows(conn: &rusqlite::Connection, enemy_name: &str) -> Vec<(String, i64, i64)> {
    let mut stmt = match conn.prepare(
        "SELECT item_name, SUM(total_quantity), SUM(times_dropped)
         FROM imported_enemy_kill_loot_agg
         WHERE enemy_name = ?1
         GROUP BY item_name",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([enemy_name], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

fn combine_loot_rows(mine: Vec<(String, i64, i64)>, imported: Vec<(String, i64, i64)>) -> Vec<(String, i64, i64)> {
    let mut combined: HashMap<String, (i64, i64)> = HashMap::new();
    for (item, qty, dropped) in mine.into_iter().chain(imported.into_iter()) {
        let entry = combined.entry(item).or_insert((0, 0));
        entry.0 += qty;
        entry.1 += dropped;
    }
    combined.into_iter().map(|(item, (qty, dropped))| (item, qty, dropped)).collect()
}

#[tauri::command]
pub fn get_enemy_kill_stats(
    db: State<'_, DbPool>,
    enemy_name: String,
    scope: String,
) -> Result<EnemyKillStats, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    let total_kills = match scope.as_str() {
        "mine" => mine_total_kills(&conn, &enemy_name),
        "imported" => imported_total_kills(&conn, &enemy_name),
        _ => mine_total_kills(&conn, &enemy_name) + imported_total_kills(&conn, &enemy_name),
    };

    if total_kills == 0 {
        return Ok(EnemyKillStats {
            enemy_name,
            total_kills: 0,
            loot: Vec::new(),
        });
    }

    let loot_rows = match scope.as_str() {
        "mine" => mine_loot_rows(&conn, &enemy_name),
        "imported" => imported_loot_rows(&conn, &enemy_name),
        _ => combine_loot_rows(mine_loot_rows(&conn, &enemy_name), imported_loot_rows(&conn, &enemy_name)),
    };

    let mut loot: Vec<EnemyLootDrop> = loot_rows
        .into_iter()
        .map(|(item_name, total_quantity, times_dropped)| EnemyLootDrop {
            item_name,
            total_quantity,
            times_dropped,
            drop_rate: times_dropped as f64 / total_kills as f64,
        })
        .collect();
    loot.sort_by(|a, b| b.times_dropped.cmp(&a.times_dropped).then(b.total_quantity.cmp(&a.total_quantity)));

    Ok(EnemyKillStats {
        enemy_name,
        total_kills,
        loot,
    })
}

#[derive(Serialize)]
pub struct ItemDropSource {
    pub enemy_name: String,
    pub total_kills: i64,
    pub times_dropped: i64,
    pub total_quantity: i64,
    pub drop_rate: f64,
}

/// Given an item name (display or internal), find all enemies that have dropped it and their drop rates.
#[tauri::command]
pub fn get_item_drop_sources(
    db: State<'_, DbPool>,
    item_name: String,
    internal_name: Option<String>,
    scope: String,
) -> Result<Vec<ItemDropSource>, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    let mut per_enemy: HashMap<String, (i64, i64)> = HashMap::new(); // enemy -> (times_dropped, total_qty)

    if scope == "mine" || scope == "combined" {
        let mut stmt = conn
            .prepare(
                "SELECT k.enemy_name, COUNT(DISTINCT l.kill_id), SUM(l.quantity)
                 FROM enemy_kill_loot l
                 JOIN enemy_kills k ON l.kill_id = k.id
                 WHERE l.item_name = ?1 OR (?2 IS NOT NULL AND l.item_name = ?2)
                 GROUP BY k.enemy_name",
            )
            .map_err(|e| format!("Failed to prepare drop source query: {e}"))?;
        let rows = stmt
            .query_map(rusqlite::params![&item_name, &internal_name], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            })
            .map_err(|e| format!("Drop source query failed: {e}"))?;
        for row in rows {
            let (enemy_name, times_dropped, qty) = row.map_err(|e| format!("Drop source row error: {e}"))?;
            let entry = per_enemy.entry(enemy_name).or_insert((0, 0));
            entry.0 += times_dropped;
            entry.1 += qty;
        }
    }

    if scope == "imported" || scope == "combined" {
        let mut stmt = conn
            .prepare(
                "SELECT enemy_name, SUM(times_dropped), SUM(total_quantity)
                 FROM imported_enemy_kill_loot_agg
                 WHERE item_name = ?1 OR (?2 IS NOT NULL AND item_name = ?2)
                 GROUP BY enemy_name",
            )
            .map_err(|e| format!("Failed to prepare imported drop source query: {e}"))?;
        let rows = stmt
            .query_map(rusqlite::params![&item_name, &internal_name], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            })
            .map_err(|e| format!("Imported drop source query failed: {e}"))?;
        for row in rows {
            let (enemy_name, times_dropped, qty) = row.map_err(|e| format!("Imported drop source row error: {e}"))?;
            let entry = per_enemy.entry(enemy_name).or_insert((0, 0));
            entry.0 += times_dropped;
            entry.1 += qty;
        }
    }

    let mut sources = Vec::new();
    for (enemy_name, (times_dropped, total_quantity)) in per_enemy {
        let total_kills = match scope.as_str() {
            "mine" => mine_total_kills(&conn, &enemy_name),
            "imported" => imported_total_kills(&conn, &enemy_name),
            _ => mine_total_kills(&conn, &enemy_name) + imported_total_kills(&conn, &enemy_name),
        };
        sources.push(ItemDropSource {
            enemy_name,
            total_kills,
            times_dropped,
            total_quantity,
            drop_rate: if total_kills > 0 {
                times_dropped as f64 / total_kills as f64
            } else {
                0.0
            },
        });
    }
    sources.sort_by(|a, b| b.times_dropped.cmp(&a.times_dropped).then(b.total_quantity.cmp(&a.total_quantity)));

    Ok(sources)
}

// ── Search (Database tab) ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EnemySearchResult {
    pub enemy_name: String,
    pub total_kills: i64,
    pub distinct_loot_items: i64,
}

#[derive(Serialize)]
pub struct ItemSearchResult {
    pub item_name: String,
    pub total_quantity: i64,
    pub distinct_enemies: i64,
}

#[tauri::command]
pub fn search_database_enemies(
    db: State<'_, DbPool>,
    query: String,
    scope: String,
    limit: Option<usize>,
) -> Result<Vec<EnemySearchResult>, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;
    let limit = limit.unwrap_or(50) as i64;
    let pattern = format!("%{}%", query.to_lowercase());

    let mut names: Vec<String> = Vec::new();
    if scope == "mine" || scope == "combined" {
        let mut stmt = conn
            .prepare("SELECT DISTINCT enemy_name FROM enemy_kills WHERE LOWER(enemy_name) LIKE ?1")
            .map_err(|e| format!("Failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map([&pattern], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Query failed: {e}"))?;
        for row in rows {
            names.push(row.map_err(|e| format!("Row error: {e}"))?);
        }
    }
    if scope == "imported" || scope == "combined" {
        let mut stmt = conn
            .prepare("SELECT DISTINCT enemy_name FROM imported_enemy_kills_agg WHERE LOWER(enemy_name) LIKE ?1")
            .map_err(|e| format!("Failed to prepare imported query: {e}"))?;
        let rows = stmt
            .query_map([&pattern], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Imported query failed: {e}"))?;
        for row in rows {
            let name = row.map_err(|e| format!("Row error: {e}"))?;
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }

    let mut results: Vec<EnemySearchResult> = names
        .into_iter()
        .map(|enemy_name| {
            let total_kills = match scope.as_str() {
                "mine" => mine_total_kills(&conn, &enemy_name),
                "imported" => imported_total_kills(&conn, &enemy_name),
                _ => mine_total_kills(&conn, &enemy_name) + imported_total_kills(&conn, &enemy_name),
            };
            let loot_rows = match scope.as_str() {
                "mine" => mine_loot_rows(&conn, &enemy_name),
                "imported" => imported_loot_rows(&conn, &enemy_name),
                _ => combine_loot_rows(mine_loot_rows(&conn, &enemy_name), imported_loot_rows(&conn, &enemy_name)),
            };
            EnemySearchResult {
                enemy_name,
                total_kills,
                distinct_loot_items: loot_rows.len() as i64,
            }
        })
        .collect();
    results.sort_by(|a, b| b.total_kills.cmp(&a.total_kills));
    results.truncate(limit as usize);

    Ok(results)
}

#[tauri::command]
pub fn search_database_items(
    db: State<'_, DbPool>,
    query: String,
    scope: String,
    limit: Option<usize>,
) -> Result<Vec<ItemSearchResult>, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;
    let limit = limit.unwrap_or(50) as i64;
    let pattern = format!("%{}%", query.to_lowercase());

    let mut agg: HashMap<String, (i64, std::collections::HashSet<String>)> = HashMap::new();

    if scope == "mine" || scope == "combined" {
        let mut stmt = conn
            .prepare(
                "SELECT l.item_name, SUM(l.quantity), k.enemy_name
                 FROM enemy_kill_loot l
                 JOIN enemy_kills k ON l.kill_id = k.id
                 WHERE LOWER(l.item_name) LIKE ?1
                 GROUP BY l.item_name, k.enemy_name",
            )
            .map_err(|e| format!("Failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map([&pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|e| format!("Query failed: {e}"))?;
        for row in rows {
            let (item_name, qty, enemy_name) = row.map_err(|e| format!("Row error: {e}"))?;
            let entry = agg.entry(item_name).or_insert((0, std::collections::HashSet::new()));
            entry.0 += qty;
            entry.1.insert(enemy_name);
        }
    }

    if scope == "imported" || scope == "combined" {
        let mut stmt = conn
            .prepare(
                "SELECT item_name, SUM(total_quantity), enemy_name
                 FROM imported_enemy_kill_loot_agg
                 WHERE LOWER(item_name) LIKE ?1
                 GROUP BY item_name, enemy_name",
            )
            .map_err(|e| format!("Failed to prepare imported query: {e}"))?;
        let rows = stmt
            .query_map([&pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|e| format!("Imported query failed: {e}"))?;
        for row in rows {
            let (item_name, qty, enemy_name) = row.map_err(|e| format!("Row error: {e}"))?;
            let entry = agg.entry(item_name).or_insert((0, std::collections::HashSet::new()));
            entry.0 += qty;
            entry.1.insert(enemy_name);
        }
    }

    let mut results: Vec<ItemSearchResult> = agg
        .into_iter()
        .map(|(item_name, (total_quantity, enemies))| ItemSearchResult {
            item_name,
            total_quantity,
            distinct_enemies: enemies.len() as i64,
        })
        .collect();
    results.sort_by(|a, b| b.total_quantity.cmp(&a.total_quantity));
    results.truncate(limit as usize);

    Ok(results)
}

// ── Import / Export (community drop-rate sharing) ──────────────────────────

#[derive(Serialize, Deserialize)]
struct ExportedLoot {
    item_name: String,
    total_quantity: i64,
    times_dropped: i64,
}

#[derive(Serialize, Deserialize)]
struct ExportedEnemy {
    enemy_name: String,
    total_kills: i64,
    loot: Vec<ExportedLoot>,
}

#[derive(Serialize, Deserialize)]
struct ExportBundle {
    format_version: u32,
    exported_at: String,
    enemies: Vec<ExportedEnemy>,
}

/// Export the player's own personally-observed kills/loot (never previously
/// imported data) to a JSON file at `path`. No character name, server, or
/// timestamp data is included — only aggregate enemy/item counts.
#[tauri::command]
pub fn export_kill_loot_database(db: State<'_, DbPool>, path: String) -> Result<usize, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    let mut enemy_stmt = conn
        .prepare("SELECT DISTINCT enemy_name FROM enemy_kills")
        .map_err(|e| format!("Failed to prepare query: {e}"))?;
    let enemy_names: Vec<String> = enemy_stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut enemies = Vec::with_capacity(enemy_names.len());
    for enemy_name in enemy_names {
        let total_kills = mine_total_kills(&conn, &enemy_name);
        let loot = mine_loot_rows(&conn, &enemy_name)
            .into_iter()
            .map(|(item_name, total_quantity, times_dropped)| ExportedLoot {
                item_name,
                total_quantity,
                times_dropped,
            })
            .collect();
        enemies.push(ExportedEnemy {
            enemy_name,
            total_kills,
            loot,
        });
    }

    let count = enemies.len();
    let bundle = ExportBundle {
        format_version: 1,
        exported_at: chrono::Local::now().to_rfc3339(),
        enemies,
    };

    let json = serde_json::to_string_pretty(&bundle).map_err(|e| format!("Failed to serialize: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write file: {e}"))?;

    Ok(count)
}

#[derive(Serialize)]
pub struct ImportSummary {
    pub source_label: String,
    pub enemies_imported: usize,
    pub loot_rows_imported: usize,
}

/// Import a previously-exported drop-rate bundle. Tagged by the file's name
/// (`source_label`) so re-importing the same file replaces just that
/// source's rows instead of double-counting. Never touches the player's own
/// `enemy_kills`/`enemy_kill_loot` ground truth.
#[tauri::command]
pub fn import_kill_loot_database(db: State<'_, DbPool>, path: String) -> Result<ImportSummary, String> {
    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {e}"))?;
    let bundle: ExportBundle = serde_json::from_str(&content).map_err(|e| format!("Failed to parse file: {e}"))?;

    let source_label = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "imported".to_string());

    let mut conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    let tx = conn.transaction().map_err(|e| format!("Failed to start transaction: {e}"))?;

    // Idempotent re-import: clear any prior rows for this exact source label.
    tx.execute(
        "DELETE FROM imported_enemy_kill_loot_agg WHERE source_label = ?1",
        [&source_label],
    )
    .map_err(|e| format!("Failed to clear prior loot rows: {e}"))?;
    tx.execute(
        "DELETE FROM imported_enemy_kills_agg WHERE source_label = ?1",
        [&source_label],
    )
    .map_err(|e| format!("Failed to clear prior kill rows: {e}"))?;
    tx.execute(
        "INSERT INTO imported_kill_sources (source_label, display_name, imported_at)
         VALUES (?1, ?2, CURRENT_TIMESTAMP)
         ON CONFLICT(source_label) DO UPDATE SET imported_at = CURRENT_TIMESTAMP",
        rusqlite::params![source_label, source_label],
    )
    .map_err(|e| format!("Failed to record import source: {e}"))?;

    let mut loot_rows_imported = 0usize;
    for enemy in &bundle.enemies {
        tx.execute(
            "INSERT INTO imported_enemy_kills_agg (source_label, enemy_name, total_kills) VALUES (?1, ?2, ?3)",
            rusqlite::params![source_label, enemy.enemy_name, enemy.total_kills],
        )
        .map_err(|e| format!("Failed to import enemy '{}': {e}", enemy.enemy_name))?;

        for loot in &enemy.loot {
            tx.execute(
                "INSERT INTO imported_enemy_kill_loot_agg
                    (source_label, enemy_name, item_name, total_quantity, times_dropped)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    source_label,
                    enemy.enemy_name,
                    loot.item_name,
                    loot.total_quantity,
                    loot.times_dropped
                ],
            )
            .map_err(|e| format!("Failed to import loot row: {e}"))?;
            loot_rows_imported += 1;
        }
    }

    tx.commit().map_err(|e| format!("Failed to commit import: {e}"))?;

    Ok(ImportSummary {
        source_label,
        enemies_imported: bundle.enemies.len(),
        loot_rows_imported,
    })
}

#[derive(Serialize)]
pub struct ImportedSource {
    pub source_label: String,
    pub display_name: String,
    pub imported_at: String,
    pub enemy_count: i64,
}

#[tauri::command]
pub fn list_imported_sources(db: State<'_, DbPool>) -> Result<Vec<ImportedSource>, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;

    let mut stmt = conn
        .prepare(
            "SELECT s.source_label, s.display_name, datetime(s.imported_at),
                    (SELECT COUNT(*) FROM imported_enemy_kills_agg a WHERE a.source_label = s.source_label)
             FROM imported_kill_sources s
             ORDER BY s.imported_at DESC",
        )
        .map_err(|e| format!("Failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ImportedSource {
                source_label: row.get(0)?,
                display_name: row.get(1)?,
                imported_at: row.get(2)?,
                enemy_count: row.get(3)?,
            })
        })
        .map_err(|e| format!("Query failed: {e}"))?;

    let mut sources = Vec::new();
    for row in rows {
        sources.push(row.map_err(|e| format!("Row error: {e}"))?);
    }
    Ok(sources)
}

#[tauri::command]
pub fn delete_imported_source(db: State<'_, DbPool>, source_label: String) -> Result<(), String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;
    conn.execute(
        "DELETE FROM imported_kill_sources WHERE source_label = ?1",
        [&source_label],
    )
    .map_err(|e| format!("Failed to delete import source: {e}"))?;
    Ok(())
}

#[derive(Serialize)]
pub struct ExtractDetail {
    pub corpse_name: Option<String>,
    pub skill: String,
    pub times: i64,
    pub total_quantity: i64,
    /// Representative (current) values. Skill/anatomy levels only ever rise, so
    /// MAX is the player's latest level; equipment bonus is stable per setup.
    pub skill_level: Option<i64>,
    pub equipment_bonus: Option<i64>,
    pub anatomy_family: Option<String>,
    pub anatomy_level: Option<i64>,
}

/// Per-corpse butchering/skinning detail for an item: the conditions under which
/// it was harvested (Butchering/Skinning level, equipment bonus, and the anatomy
/// family + level for that monster type). Drives the farming item hover tooltip.
#[tauri::command]
pub fn get_corpse_extract_details(
    db: State<'_, DbPool>,
    item_name: String,
) -> Result<Vec<ExtractDetail>, String> {
    let conn = db
        .get()
        .map_err(|e| format!("Database connection error: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT corpse_name, skill, COUNT(*) AS times, SUM(quantity) AS total_quantity,
                    MAX(skill_level), MAX(equipment_bonus), anatomy_family, MAX(anatomy_level)
             FROM corpse_extracts
             WHERE item_name = ?1
             GROUP BY corpse_name, skill, anatomy_family
             ORDER BY total_quantity DESC",
        )
        .map_err(|e| format!("Failed to prepare extract query: {e}"))?;
    let rows = stmt
        .query_map([&item_name], |row| {
            Ok(ExtractDetail {
                corpse_name: row.get(0)?,
                skill: row.get(1)?,
                times: row.get(2)?,
                total_quantity: row.get(3)?,
                skill_level: row.get(4)?,
                equipment_bonus: row.get(5)?,
                anatomy_family: row.get(6)?,
                anatomy_level: row.get(7)?,
            })
        })
        .map_err(|e| format!("Failed to query extract details: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read extract details: {e}"))?;
    Ok(rows)
}
