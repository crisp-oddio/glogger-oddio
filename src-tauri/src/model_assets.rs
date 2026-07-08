//! Model Viewer asset backend.
//!
//! Locates the user's local Project: Gorgon install, drives the offline
//! extractor (`tools/model_extractor/extract.py`) that converts Unity bundles
//! into a web-renderable cache (`{appdata}/models/`), and exposes commands the
//! frontend three.js viewer uses to resolve an item to its mesh/material/dye
//! assets.
//!
//! No game art ships in the repo or the app — everything is extracted from the
//! player's own install into their local cache. See the plan and
//! `game_data::appearance` for the item→appearance mapping.

use crate::cdn_commands::GameDataState;
use crate::game_data::appearance::{self, AppearanceDirective};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

/// Steam application id for Project: Gorgon.
const PG_STEAM_APPID: &str = "342940";
/// Relative path from a game install root to the addressable bundle dir.
const BUNDLE_SUBPATH: &str =
    "WindowsPlayer_Data/StreamingAssets/aa/StandaloneWindows64";

// ── Cache location ────────────────────────────────────────────────────────────

/// `{appdata}/models/` — where the extractor writes glb + textures + catalog.
pub fn model_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data dir: {e}"))?;
    let dir = base.join("models");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create models dir: {e}"))?;
    Ok(dir)
}

fn catalog_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(model_cache_dir(app)?.join("catalog.json"))
}

// ── Install detection ─────────────────────────────────────────────────────────

/// Candidate Steam roots to probe for `libraryfolders.vdf`.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"] {
        if let Ok(p) = std::env::var(var) {
            roots.push(PathBuf::from(p).join("Steam"));
        }
    }
    // Common non-default drives.
    for drive in ["C", "D", "E", "G"] {
        roots.push(PathBuf::from(format!("{drive}:\\SteamLibrary")));
        roots.push(PathBuf::from(format!("{drive}:\\Steam")));
    }
    roots
}

/// Parse the `"path"` values out of a Steam `libraryfolders.vdf`.
fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let line = line.trim();
        // e.g.  "path"		"C:\\Program Files (x86)\\Steam"
        if let Some(rest) = line.strip_prefix("\"path\"") {
            if let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                if let Some(end) = after.find('"') {
                    let raw = &after[..end];
                    out.push(PathBuf::from(raw.replace("\\\\", "\\")));
                }
            }
        }
    }
    out
}

/// Try to locate the addressable bundle directory of the local PG install.
/// Checks an explicit override first, then Steam library folders.
pub fn locate_bundles_dir(explicit: Option<&str>) -> Option<PathBuf> {
    // 1. Explicit override — accept either the install root or the bundle dir.
    if let Some(e) = explicit.filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(e);
        let direct = p.join("StandaloneWindows64");
        if direct.is_dir() {
            return Some(direct);
        }
        let sub = p.join(BUNDLE_SUBPATH);
        if sub.is_dir() {
            return Some(sub);
        }
        if p.is_dir() && p.ends_with("StandaloneWindows64") {
            return Some(p);
        }
    }

    // 2. Steam library detection.
    let mut library_paths: Vec<PathBuf> = Vec::new();
    for root in steam_roots() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(contents) = std::fs::read_to_string(&vdf) {
            library_paths.extend(parse_library_paths(&contents));
        }
        // The root itself is always an implicit library.
        library_paths.push(root);
    }
    library_paths.sort();
    library_paths.dedup();

    for lib in library_paths {
        let candidate = lib
            .join("steamapps")
            .join("common")
            .join("Project Gorgon")
            .join(BUNDLE_SUBPATH);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    let _ = PG_STEAM_APPID; // reserved for future manifest-based detection
    None
}

// ── Catalog access ────────────────────────────────────────────────────────────

fn load_catalog(app: &AppHandle) -> Result<serde_json::Value, String> {
    let path = catalog_path(app)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read catalog ({}): {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("catalog.json parse error: {e}"))
}

#[derive(Serialize)]
pub struct ModelViewerStatus {
    /// Absolute bundle dir if a local install was found.
    pub game_bundles_dir: Option<String>,
    /// True if `{appdata}/models/catalog.json` exists.
    pub cache_ready: bool,
    /// Absolute cache root (for building asset URLs on the frontend).
    pub cache_dir: String,
    pub appearance_count: usize,
    pub material_count: usize,
    pub weapon_count: usize,
    /// Source bundle recorded in the catalog (for staleness checks).
    pub source_bundle: Option<String>,
}

#[tauri::command]
pub async fn model_viewer_status(
    app: AppHandle,
    game_dir: Option<String>,
) -> Result<ModelViewerStatus, String> {
    let cache_dir = model_cache_dir(&app)?;
    let bundles = locate_bundles_dir(game_dir.as_deref());
    let (cache_ready, appearance_count, material_count, weapon_count, source_bundle) =
        match load_catalog(&app) {
            Ok(cat) => {
                let count = |k: &str| cat.get(k).and_then(|v| v.as_object()).map_or(0, |o| o.len());
                (
                    true,
                    count("appearances"),
                    count("materials"),
                    count("weapons"),
                    cat.get("sourceBundle")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                )
            }
            Err(_) => (false, 0, 0, 0, None),
        };
    Ok(ModelViewerStatus {
        game_bundles_dir: bundles.map(|p| p.to_string_lossy().to_string()),
        cache_ready,
        cache_dir: cache_dir.to_string_lossy().to_string(),
        appearance_count,
        material_count,
        weapon_count,
        source_bundle,
    })
}

/// Absolute cache root; the frontend joins `meshes/…`/`textures/…` and passes
/// the result to Tauri's `convertFileSrc`.
#[tauri::command]
pub async fn model_cache_root(app: AppHandle) -> Result<String, String> {
    Ok(model_cache_dir(&app)?.to_string_lossy().to_string())
}

/// Raw catalog.json (appearances / materials / weapons).
#[tauri::command]
pub async fn get_model_catalog(app: AppHandle) -> Result<serde_json::Value, String> {
    load_catalog(&app)
}

// ── Item → appearance resolution ──────────────────────────────────────────────

#[derive(Serialize)]
pub struct ResolvedSlot {
    pub slot: String,
    pub mesh_key: Option<String>,
    pub material_key: Option<String>,
    pub dye_order: Option<String>,
    pub dyeable: bool,
    /// Relative glb path in the cache, if the mesh was extracted.
    pub mesh_file: Option<String>,
    /// Relative texture paths (skin/mask1..3/normal/metallic), if a material matched.
    pub textures: Option<serde_json::Value>,
    pub dye_channels: u8,
    /// Default dye colors from the material (`_Color0.._Color3`).
    pub default_colors: Option<serde_json::Value>,
    /// True if the mesh is a weapon/held prop (not skinned body gear). The
    /// viewer orients these by bounding box (longest axis up) instead of the
    /// character Z-up→Y-up rotation.
    pub is_weapon: bool,
}

#[derive(Serialize)]
pub struct ResolvedItemAppearance {
    pub item_id: u32,
    pub item_name: String,
    pub sex: char,
    pub source_field: String,
    pub slots: Vec<ResolvedSlot>,
    pub flags: std::collections::BTreeMap<String, String>,
    /// True if at least one slot has a mesh present in the cache.
    pub renderable: bool,
    pub not_dyeable_keyword: bool,
}

fn join_catalog(cat: &serde_json::Value, d: &AppearanceDirective) -> ResolvedSlot {
    let lookup = |section: &str, key: &str| -> Option<serde_json::Value> {
        cat.get(section)
            .and_then(|s| s.get(key))
            .cloned()
    };

    // Mesh: appearances (skinned body gear) first, then weapons (held props).
    let gear_entry = d.mesh_key.as_ref().and_then(|k| lookup("appearances", k));
    let weapon_entry = d
        .mesh_key
        .as_ref()
        .filter(|_| gear_entry.is_none())
        .and_then(|k| lookup("weapons", k));
    let is_weapon = gear_entry.is_none() && weapon_entry.is_some();
    let mesh_file = gear_entry
        .as_ref()
        .or(weapon_entry.as_ref())
        .and_then(|e| e.get("meshFile").and_then(|v| v.as_str()).map(String::from));

    // Material: explicit ^Armor= key, else the appearance's defaultMaterial,
    // else a weapon's material.
    let mat_key = d.material_key.clone().or_else(|| {
        d.mesh_key.as_ref().and_then(|k| {
            lookup("appearances", k)
                .and_then(|e| e.get("defaultMaterial").and_then(|v| v.as_str()).map(String::from))
                .or_else(|| {
                    lookup("weapons", k)
                        .and_then(|e| e.get("material").and_then(|v| v.as_str()).map(String::from))
                })
        })
    });

    let mat = mat_key.as_ref().and_then(|k| lookup("materials", k));
    let textures = mat.as_ref().and_then(|m| m.get("textures").cloned());
    let dye_channels = mat
        .as_ref()
        .and_then(|m| m.get("dyeChannels").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u8;
    let default_colors = mat.as_ref().and_then(|m| m.get("colors").cloned());

    ResolvedSlot {
        slot: d.slot.clone(),
        mesh_key: d.mesh_key.clone(),
        material_key: mat_key,
        dye_order: d.dye_order.clone(),
        dyeable: d.dyeable,
        mesh_file,
        textures,
        dye_channels,
        default_colors,
        is_weapon,
    }
}

/// Resolve any item reference to its 3D appearance, joined against the local
/// asset catalog. `sex` is `"m"` or `"f"` (defaults to male).
#[tauri::command]
pub async fn resolve_item_appearance(
    reference: String,
    sex: Option<String>,
    app: AppHandle,
    state: State<'_, GameDataState>,
) -> Result<Option<ResolvedItemAppearance>, String> {
    let sex_ch = sex
        .as_deref()
        .and_then(|s| s.chars().next())
        .filter(|c| *c == 'm' || *c == 'f')
        .unwrap_or('m');

    let data = state.read().await;
    let item = match data.resolve_item(&reference) {
        Some(i) => i,
        None => return Ok(None),
    };

    let parsed = match appearance::resolve_item_appearance(
        item.equip_appearance.as_deref(),
        item.equip_appearance2.as_deref(),
        sex_ch,
    ) {
        Some(p) => p,
        None => return Ok(None),
    };

    let cat = load_catalog(&app).unwrap_or_else(|_| serde_json::json!({}));
    let slots: Vec<ResolvedSlot> = parsed
        .directives
        .iter()
        .map(|d| join_catalog(&cat, d))
        .collect();
    let renderable = slots.iter().any(|s| s.mesh_file.is_some());

    Ok(Some(ResolvedItemAppearance {
        item_id: item.id,
        item_name: item.name.clone(),
        sex: sex_ch,
        source_field: parsed.source_field.to_string(),
        slots,
        flags: parsed.flags,
        renderable,
        not_dyeable_keyword: item.keywords.iter().any(|k| k == "NotDyeable"),
    }))
}

/// A single browsable dyeable/equippable item for the Model Viewer picker.
#[derive(Serialize)]
pub struct BrowsableItem {
    pub id: u32,
    pub name: String,
    pub icon_id: Option<u32>,
    pub equip_slot: Option<String>,
    /// True if this item's appearance resolves to a mesh present in the cache.
    /// Many cosmetics (holiday hats, masks, floating gems) have no game mesh.
    pub has_model: bool,
}

/// Does any mesh key of this item's appearance exist in the extracted catalog?
fn item_has_model(item: &crate::game_data::ItemInfo, cat: &serde_json::Value) -> bool {
    let parsed = match appearance::resolve_item_appearance(
        item.equip_appearance.as_deref(),
        item.equip_appearance2.as_deref(),
        'm',
    ) {
        Some(p) => p,
        None => return false,
    };
    parsed.directives.iter().any(|d| {
        d.mesh_key.as_ref().is_some_and(|k| {
            cat.get("appearances").and_then(|s| s.get(k)).is_some()
                || cat.get("weapons").and_then(|s| s.get(k)).is_some()
        })
    })
}

/// List items that carry an equipped 3D appearance (for the viewer's item
/// picker). Optionally filter by equip slot. Each item is flagged `has_model`
/// so the UI can hide/mark ones with no extractable mesh.
#[tauri::command]
pub async fn list_appearance_items(
    slot: Option<String>,
    app: AppHandle,
    state: State<'_, GameDataState>,
) -> Result<Vec<BrowsableItem>, String> {
    // Fail open: if the catalog can't load, mark everything has_model so we
    // never hide the whole list.
    let cat = load_catalog(&app).ok();
    let data = state.read().await;
    let mut out: Vec<BrowsableItem> = data
        .items
        .values()
        .filter(|i| i.equip_appearance.is_some() || i.equip_appearance2.is_some())
        .filter(|i| match &slot {
            // "OffHand" also covers shields (equip_slot "OffHandShield").
            Some(s) if s == "OffHand" => matches!(
                i.equip_slot.as_deref(),
                Some("OffHand") | Some("OffHandShield")
            ),
            Some(s) => i.equip_slot.as_deref() == Some(s.as_str()),
            None => true,
        })
        .map(|i| BrowsableItem {
            id: i.id,
            name: i.name.clone(),
            icon_id: i.icon_id,
            equip_slot: i.equip_slot.clone(),
            has_model: cat.as_ref().map_or(true, |c| item_has_model(i, c)),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

// ── Extraction ────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct ExtractionProgress {
    stage: String,
    message: String,
    done: bool,
    ok: bool,
}

/// Locate the extractor script. In dev this is the repo's
/// `tools/model_extractor/extract.py`; release builds ship a sidecar (TODO).
fn extractor_script() -> Option<PathBuf> {
    // Dev: resolve relative to the crate manifest dir captured at build time.
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        let p = Path::new(manifest)
            .parent() // repo root (manifest is src-tauri)
            .map(|r| r.join("tools").join("model_extractor").join("extract.py"));
        if let Some(p) = p {
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Run the extractor against the local install, writing into the model cache.
/// Emits `model-extraction-progress` events. Long-running.
#[tauri::command]
pub async fn start_model_extraction(
    app: AppHandle,
    game_dir: Option<String>,
) -> Result<String, String> {
    let bundles = locate_bundles_dir(game_dir.as_deref())
        .ok_or_else(|| "Could not locate a Project: Gorgon install".to_string())?;
    let out = model_cache_dir(&app)?;
    let script = extractor_script()
        .ok_or_else(|| "Extractor script not found (dev: tools/model_extractor/extract.py)".to_string())?;

    let emit = |app: &AppHandle, stage: &str, message: &str, done: bool, ok: bool| {
        let _ = app.emit(
            "model-extraction-progress",
            ExtractionProgress {
                stage: stage.into(),
                message: message.into(),
                done,
                ok,
            },
        );
    };
    emit(&app, "start", &format!("Extracting from {}", bundles.display()), false, true);

    let py = if cfg!(windows) { "python" } else { "python3" };
    let output = tokio::process::Command::new(py)
        .arg(&script)
        .arg("--game-dir")
        .arg(&bundles)
        .arg("--out")
        .arg(&out)
        .output()
        .await
        .map_err(|e| format!("Failed to launch extractor ({py}): {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        emit(&app, "error", &stderr, true, false);
        return Err(format!(
            "Extractor failed (code {:?}):\n{stderr}",
            output.status.code()
        ));
    }
    let summary = stdout.lines().last().unwrap_or("done").to_string();
    emit(&app, "done", &summary, true, true);
    Ok(summary)
}
