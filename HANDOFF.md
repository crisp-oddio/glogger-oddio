# glogger — Session Handoff

**Date:** 2026-06-18
**Machine:** Windows 11 (primary dev box)
**Outcome:** Two new dashboard widgets — **XP Rate** (combat vs. prodigy XP/hr + generalized
ETA) and **Prodigy Tracker** (same rates + a user-entered current-XP baseline for a precise
ETA). Backend prodigy-XP parsing + store wiring + widgets, all type-checked and unit-tested.
Verified live in the dev build.

---

## TL;DR

- **XP Rate** widget (`xp-rate`): three divided sections — Combat XP/hr + Combat XP/session,
  Prodigy XP/hr + Prodigy XP/session, and Next prodigy level ETA (generalized: assumes a full
  250M). Combat lines white, prodigy lines gold. Reset button.
- **Prodigy Tracker** widget (`prodigy-tracker`): identical rate sections, plus a persisted
  "Current prodigy XP" input, a progress line + bar (`current / 250M (%)`), and a Next-prodigy-
  level ETA based on *actual remaining* XP. `current = entered baseline + session prodigy XP`.
- New backend parser for prodigy XP, new store accumulator, two new Vue widgets. No coordinator
  change needed (it already broadcasts every `chat-status-event`).

---

## Prodigy XP model (confirmed from a real Chat.log)

- A **maxed combat skill** earns **Prodigy XP** as overflow. Each kill emits a PAIR of
  `[Status]` lines: `You earned N Prodigy XP in <Skill>.` (prodigy, from the maxed skill —
  e.g. "Pig") and `You earned N XP in <Skill>.` (normal XP for the skill being leveled).
- **One prodigy level = 250,000,000 combat XP** (per the Prodigy Potential wiki). The game
  does **not** report current progress within a level, so the ETA assumes a full 250M:
  `ETA = 250,000,000 ÷ prodigy XP/hr`.
- Wiki (https://wiki.projectgorgon.com/wiki/Prodigy_Potential) is sparse; the format above
  came from the user's own log.

## Implementation

- **Backend** — `src-tauri/src/chat_status_parser.rs`: new `ChatStatusEvent::ProdigyXpGained`
  variant + `try_prodigy_xp_gained` (runs before `try_xp_gained`; normal XP lines are unaffected
  because `"N Prodigy"` fails the u32 parse). Two new unit tests. Emitted via the existing
  `chat-status-event` (coordinator `_ => {}` arm already forwards it — no change there).
- **Store** — `src/stores/gameStateStore.ts`: `xpRateSession` accumulator (combat vs prodigy,
  wall-clock keyed so the rate stays live). Combat line is filtered to combat skills only via
  `get_combat_skills` (CDN `Combat: true`), loaded lazily in `loadAll` and cached in
  `combatSkillNames`. Helpers: `accrueXpRate`, `xpRateOf(kind, nowMs)`, `prodigyEta`,
  `resetXpRateSession` (also called from `resetSessionSkills`, i.e. on character login).
  `formatEta` (shared ETA formatter) and `prodigyEta` exported for the widgets.
  Cases added in `handleChatStatusEvent` for `XpGained` (combat-filtered) and `ProdigyXpGained`.
- **UI** — both widgets registered in `dashboardWidgets.ts` (small, right after Live Skill
  Tracking), each with a 1s ticking clock and compact K/M number formatting:
  - `XpRateWidget.vue` (`xp-rate`) — generalized ETA (assumes a full 250M).
  - `ProdigyTrackerWidget.vue` (`prodigy-tracker`) — adds a "Current prodigy XP" input persisted
    via `useViewPrefs('widget.prodigy-tracker', { startXp })`; `currentXp = startXp + session
    prodigy XP` (capped at 250M); ETA = `(250M − currentXp) ÷ prodigy XP/hr`. Shows a progress
    bar + `Ready!` at cap. NOTE: Reset zeroes the shared session accumulator, so after a Reset
    you must re-enter your current in-game prodigy total.

## Design decisions (from the user)

1. **Split:** non-prodigy line = non-maxed **combat** skills only (crafting/tradeskill excluded);
   prodigy line = the maxed-skill overflow.
2. **ETA:** assume a full 250M per level (no current-progress source in the logs).
3. Session totals are shown as their own line items grouped with the matching rate.

## Open items / next steps

- The Prodigy Tracker's current-XP baseline is entered by hand. If a current-prodigy-progress
  value is ever found in the logs (an attribute, UI value, or log line), auto-populate it so the
  XP Rate widget's ETA can also count down from real remaining XP without manual input.
- The rate window is session-start → now (noisy in the first few seconds, settles as you play);
  Reset restarts it. Could add a rolling/“last N minutes” window if the lifetime average feels stale.
- Per the previous handoff, the crafting flat-4×-first-craft / repeat-craft drop-off model still
  wants wiring into **Quick Calc** (`resolveRecipeIngredients` only sets `xp_first_time = base × 3`
  and doesn't model repeat-craft drop-off yet).

## Release / CI note (unchanged)

Always use the **Release** workflow (`release.yml`, workflow_dispatch) — it's the only path that
builds Windows/macOS/Linux and creates the release. A bare tag push only triggers `flatpak.yml`
(this is what left v0.9.2 without a Windows `.exe`). The Flatpak `attach` job waits (up to 30 min,
polling) for the release to exist before `gh release upload`.

---

# Session 2 — Survey investigation + signed-ID fix (2026-06-18, on v0.9.5)

**Outcome:** Found and fixed the root-cause bug behind unreliable survey tracking (signed
instance IDs). Explored a native survey-map/trilateration engine, then **reverted it** at the
user's request — branch is back to clean **v0.9.5 plus the ID fix only**.

## ✅ Kept: signed instance-ID parser fix (`player_event_parser.rs`)

- **Bug:** Project: Gorgon logs item instance IDs as **signed 32-bit** ints (often negative, e.g.
  `ProcessDeleteItem(-1796085135)`). The parser read them as `u64`, so `parse::<u64>()` failed on
  every negative ID and the parser **silently dropped the whole line** — losing ~half of all item
  add/delete/loot events (coin-flip on the sign bit). This is why surveys tracked only
  intermittently: a survey map's craft/consume/loot were dropped whenever its instance ID was
  negative.
- **Fix:** shared `parse_instance_id(&str) -> Option<u64>` that parses `i64` then casts `as u64`
  (bit-preserving, so the same string maps to the same registry key). Routed all 8 item-instance
  parse sites through it (add, updateitemcode, delete, vendor sold/update, storage add/remove,
  remove-loot). Entity/NPC IDs (`u32`) left as-is — separate concern.
- **Test:** `test_negative_instance_id_add_and_delete_resolve` (uses the real Rubywall log lines).
- **Status:** uncommitted in the working tree; 390 lib tests pass, `vue-tsc` clean. Memory note:
  `memory/project_signed_instance_ids.md`.

## 🔎 Findings worth keeping (for if the survey work resumes)

- **glogger's survey tracker is 100% Player.log-driven**, not Chat.log. The aggregator
  (`survey::aggregator`) only sees `ItemAdded`/`ItemDeleted`/`DelayLoopStarted` from the player
  event path (coordinator ~L846). Chat `[Status]` lines never reach it. So integrating
  GorgonSurveyTracker (which reads Chat.log `[Status]` distance msgs) was never going to work —
  different signals, different files.
- **Crystal/mineral (geology) surveys** reveal exact node coords in Player.log via
  `ProcessMapFx((x,y,z), …, "X is here", …, "The X is 395m west and 873m south.")`. **Motherlode
  (mining) surveys** give **distance only** (`"The treasure is N meters from here."`) — the game
  withholds location by design (wiki confirms: no red dot, circle of radius N). So motherlode
  location *requires* trilateration; crystals don't.
- **Player position is never logged continuously.** Only at `SPAWNING LOCAL PLAYER AT (x,y,z)`
  (login/zone-entry, ~4×/session) and derivable from any directional `ProcessMapFx` (node coord +
  bearing → invert for player pos). No movement/heading stream. This is the real blocker for
  passive motherlode trilateration — you can't know where you stood at each distance reading.
- **Bare zone maps:** the cleanest source is the **game client itself** — Unity Addressable
  bundles at `<PG install>/WindowsPlayer_Data/StreamingAssets/aa/StandaloneWindows64/maps_assets_assets_art_maps_map_area<zone>.png_<hash>.bundle`,
  one `Texture2D` each (~1024–2048px, label-free, coordinate-aligned). Extractable with UnityPy
  (Python prototype worked for all 14 outdoor zones). **Key insight:** extract from each user's
  *own* install at runtime → zero redistribution of Elder Game's art. The community wiki maps
  (`*MarkedMap.jpg`) are cluttered and have murky licensing by comparison.

## ↩️ Reverted (not in the tree anymore)

The native survey UI was built and then removed at the user's request: top-level **Survey
Tracking** header (Sessions/Analytics/Map), `SurveyMapView.vue` (pan/zoom + node dots + 2-point
per-zone calibration), `surveyMapStore.ts`, the `ProcessMapFx` → `SurveyNodeLocated` parser/event,
and the staged `zone-maps/` assets. If resumed, the findings above are the starting point; the
extraction + parsing approaches were proven working before removal.

---

# Session 3 — Upstreaming the cprivitere (TwinkleofToes) fork PRs (2026-06-19, on v0.9.5→v0.9.8)

**Context:** TwinkleofToes (GitHub `cprivitere`, repo `cprivitere/glogger`) opened PRs against our
repo to fold in fixes he'd made for the abandoned "zenith" fork, intending to switch to us as his
upstream. Two PRs landed; both reviewed before merge.

## ✅ PR #1 — macOS fixes + UI tweaks (merged via #3)

Cherry-picked his 5 commits onto a branch, **excluded the `GeneralSettings.vue` rework** (extra
path fields/buttons — the author himself flagged the layout as rough) by reverting just that file
to main's version, then merged as **PR #3**. Kept:
- macOS Player.log / game-data **path split** (they live in different dirs on macOS): new
  `get_default_player_log_path()` + `get_default_game_data_path()` in `settings.rs`, wired through
  `setup_commands.rs` / `settingsStore.ts`. Windows path behavior preserved via empty-string fallback.
- Clickable MenuBar status dots (toggle tailing), Start Tailing button on the Inventory warning
  banner, default file-picker paths, and a **Cmd+Q fix** in `useKeyboard.ts` (tab-cycling handler
  no longer swallows ⌘Q). `vue-tsc` + `cargo check` both clean.

## ✅ PR #2 — Node/Action/npm version bumps (merged, then **two bugs fixed on main**)

Merged the Node 22→24, GitHub-Actions, and npm-lockfile bumps — **but the PR was broken** and the
author's "frontend build succeeds" claim missed it because the `validate` CI job only runs
`cargo check`/`cargo test`/`vite build`, none of which invoke `tauri build` (the step that enforces
version parity). Found and fixed both via live Experimental runs:

1. **`tauri-apps/tauri-action@v1` doesn't exist** — the action has no `v1` tag (latest major is
   `v0`, currently → v0.6.2). Every build job failed at action resolution. **Fixed** → pinned back
   to `@v0` in `release.yml` + `experimental.yml`.
2. **Tauri JS/Rust version mismatch** — PR bumped the **npm** packages (`@tauri-apps/api` 2.11.1,
   `plugin-dialog` 2.7.1) but left the **Rust crates** at `tauri` 2.10.3 / `tauri-plugin-dialog`
   2.6.0. `tauri build` requires matching major/minor across both sides → all builds failed.
   **Fixed (user's call: align npm *down* to the known-good Rust crates)** → pinned
   `@tauri-apps/api` to `~2.10.1` and `@tauri-apps/plugin-dialog` to `~2.6.0` (tilde so npm can't
   drift back into 2.11/2.7). All other npm bumps from PR #2 (vue/vite/tailwind/etc.) retained.
   0 npm vulnerabilities.

**Validation:** Experimental workflow run #27839157547 — **all 6 jobs green**, published
`v0.9.8-exp` prerelease with Windows/macOS/Linux installers. Both fixes exercised end-to-end.

## ⚠️ Gotchas discovered (read before touching CI again)

- **The `Experimental` workflow can only be dispatched from `main`.** Its `prepare` job does a
  hardcoded `git push origin main --tags`; dispatched from any other branch it fails instantly with
  `src refspec main does not match any` (before the build even runs). So there's **no branch-based
  dry-run** — you must merge to main first, then test. Each successful (or build-failed-but-prepare-
  succeeded) run **bumps the patch version on main and tags `vX.Y.Z-exp`**. This is why main walked
  0.9.5 → 0.9.8 over three runs; dangling `-exp` tags from failed runs were deleted as cleanup.
- If you want a true dry-run vehicle later, the workflow would need a guard to skip the
  commit/push/tag when dispatched off a non-main ref.

## 📋 State at end of session

- `main` @ **v0.9.8**, all fixes pushed. PRs #1, #2, #3 closed/merged.
- The Session 2 **signed-instance-ID fix** (`player_event_parser.rs`) was finally **committed** this
  session alongside this HANDOFF.md (390 lib tests green) — no longer dangling in the working tree.
- No real (non-exp) release cut this session — next real release via the normal `Release` workflow
  will pick up everything from 0.9.8.

---

# Session 4 — Settings path auto-detect buttons + startup auto-detect (2026-06-19, on v0.9.8)

**Outcome:** Added three path-management buttons and an on-startup auto-detect checkbox to
**Settings → General → Game Data Directory**. All platform-aware (Windows/macOS/Linux), type-checked,
`cargo check` clean, and **verified live end-to-end** in the dev build (including a restart test).

## What was added (UI: `src/components/Settings/GeneralSettings.vue`)

Replaced the old single "Use Default Player.log Location" button with:

- **Auto-Detect Game Path** — sets `gameDataPath` to the OS default via the existing backend command
  `get_default_game_data_path_command`.
- **Auto-Detect Player.log Path** — calls `get_default_player_log_path_command`; that returns an
  explicit path on macOS and **empty on Windows** (logs live inside the game folder), so it falls
  back to `<gameDataPath>/Player.log`. Mixed `\`+`/` separators are cosmetic and work fine on Windows.
- **Reset Paths** — restores both paths to the raw backend defaults (`logFilePath` ends up empty on
  Windows, i.e. derived at read time by `get_player_log_path`).
- **Checkbox "Auto-detect game & Player.log paths on startup"** — persists `autoDetectPathsOnStartup`.

Each button writes an inline status line (green = ok, red = unsupported OS / no default found).
Unsupported OSes (Linux) get a "set it manually" message instead of a blank path.

## Backend wiring

- **`settings.rs`** — new field `auto_detect_paths_on_startup: bool` (`#[serde(default)]`, default
  `false`); added to the `Default` impl. The platform path helpers (`get_default_game_data_path`,
  `get_default_player_log_path`) and their `*_command` Tauri wrappers already existed and are
  registered in `lib.rs`.
- **`lib.rs` — new "Step 2b"** runs right after `Settings loaded` and **before** the coordinator
  starts the log watchers (so refreshed paths take effect that same launch). When the flag is on and
  the default differs, it overwrites `game_data_path`/`log_file_path` and saves. Logs
  `Auto-detected paths on startup (enabled in settings)`. NOTE: `startup_log!` carries a trailing
  semicolon, so its match arms must be wrapped in `{ … }` blocks (bare expression form won't compile).
- **`settingsStore.ts`** — `autoDetectPathsOnStartup` added to both interfaces, both converters
  (`to`/`fromBackendSettings`), and `getDefaultSettings()`.

## Placement note (for the user)

Buttons/checkbox live under the **General** tab's existing "Game Data Directory" section (next to the
path field they act on), **not** the separate "App Settings" tab (which holds Appearance/font/opacity).
This was a deliberate UX call; mentioned to the user, easy to move if they prefer.

## Verification

1. `npx vue-tsc --noEmit` clean; `cargo check` clean.
2. Live: set the path field to a bogus value → each button restored/derived the correct path with a
   status line. Reset Paths restored platform defaults.
3. **Startup test:** enabled the checkbox, set `game_data_path` to `Z:\wrong\on\purpose`, confirmed it
   persisted, **restarted the dev build** → log showed `Auto-detected paths on startup`, `settings.json`
   was corrected back to the real LocalLow path, and Player.log catch-up started normally. (Test wrote
   to the **dev** profile `%APPDATA%\glogger.Dev\settings.json`, separate from the real install.)
