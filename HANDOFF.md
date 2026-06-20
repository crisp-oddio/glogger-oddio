# glogger — Session Handoff

**Date:** 2026-06-20
**Machine:** Windows 11 (primary dev box)
**Branch:** `dev` == `main` (both at `151f14b`, version strings v0.9.13)
**Outcome:** **Statehelm gifting widget polish** — high-favor NPCs now drop off the
gifting list. Frontend-only follow-up to Session 8's skill-driven rework. Committed
on `dev` and synced to `main` (absorbed a `release: v0.9.13` bump from main via merge).

---

## Session 10 — Statehelm widget: drop high-favor NPCs (2026-06-20)

**Outcome:** Small follow-up to the Session-8 Statehelm Gifting widget. NPCs now fall
off the widget based on **favor standing** (not just weekly gifts), so you stop seeing
NPCs that no longer benefit from gifting. `vue-tsc` clean; trusted in by the user
without a live re-test (small, localized change).

- **`useStatehelmTracker.ts`** — new `isExcludedByFavor(status)`, applied inside
  `representativeFor()` next to the maxed-gift filter (so an excluded NPC falls off and
  the next-highest skill of that category backfills the slot):
  - **Soul Mates** (top tier) → always excluded.
  - **Like Family** → excluded **unless** the NPC offers storage (storage capacity keeps
    scaling past Like Family, so storage NPCs stay worth gifting). Uses the existing
    `hasStorage()` from `useNpcServices`.
  - Tiers below Like Family are unaffected.
- **`docs/.../widget-statehelm-summary.md`** — falloff section documents the new rules.
- Commit `ffb6344`. Pushing `dev:main` hit the usual release-bump divergence (main had
  `release: v0.9.13` + PR-merge commits dev lacked); resolved per the standing gotcha —
  `git merge origin/main` into `dev` (clean, version strings only), then push. Both
  branches now at `151f14b`.

---

## Session 9 — Survey loot summary from Chat.log `[Status]` (2026-06-19)

**Outcome:** The Economics → Surveying **session loot summary was always empty**
for the user; now it populates live and matches Kaeus' GorgonSurveyTracker ("GST")
summary. Root-caused, fixed, full `cargo test` green (399 + 5 new), **verified live**.

### 🔎 Root cause

`loot_summary_for_session` ([survey/commands.rs](src-tauri/src/survey/commands.rs))
only counts `item_transactions` rows linked via `source_details->>'survey_use_id'`,
and that link was injected **only** on the Player.log path
(`survey::aggregator::process_event`, driven by `ProcessAddItem`/mining context).
PG's verbose Player.log item logging wasn't reaching the user, so the summary stayed
empty even while loot flowed. **Kaeus' GST reads the always-present Chat.log
`[Status]` "added to inventory" lines** — which glogger already parsed and wrote to
`item_transactions` (`source='chat_status'`) but **never linked to the session**
(NULL `source_details`/`item_type_id`). Note: **"GST" == Kaeus' GorgonSurveyTracker
exe**, downloaded/launched by `gst_manager.rs` — not an in-house tracker.

### ✅ The fix (chat-authoritative, native — committed `34160f8`)

- **[survey/persistence.rs](src-tauri/src/survey/persistence.rs)** —
  `latest_use_id_for_session`: most-recent use to attach chat loot to.
- **[survey/aggregator.rs](src-tauri/src/survey/aggregator.rs)** —
  `attribute_chat_gain`: attributes a `[Status]` loot gain to the active session's
  latest survey use, bumps `loot_qty` + loot timestamps, returns the `survey_use_id`.
  (Chat lines carry no node/map id → attribution is at the **use** level, fine for
  the sequential use→collect→use workflow.) +3 unit tests.
- **[coordinator.rs](src-tauri/src/coordinator.rs)** — the chat `ItemGained` handler
  now always populates `internal_name`/`item_type_id` (needed for valuation), calls
  `attribute_chat_gain` for `loot`-context gains, and tags the row
  `source_kind='survey_chat'` + `source_details` `{survey_use_id}`.
- **[survey/commands.rs](src-tauri/src/survey/commands.rs)** —
  `loot_summary_for_session` is **chat-authoritative**: per use, count `survey_chat`
  rows when present; **else fall back to Player.log rows** (`NOT EXISTS` guard) so
  historical/pre-existing sessions still render and no gain is double-counted. +2
  tests. Analytics zone/type totals come from the denormalized `loot_qty` (which the
  chat path bumps), so they populate for free — **no analytics query changes needed.**

### ⚠️ Caveats / known limits

- **Speed-bonus is lost on the chat path** — `[Status]` lines have no "(speed bonus!)"
  marker, so chat-attributed loot has `bonus_qty = 0`. The speed-bonus **analytics**
  (zone/type bonus CTEs) remain Player.log-only; they'll show 0 for chat-only users.
- A hypothetical user whose **Player.log attribution also works** would see
  `loot_qty` (denorm) double-bumped, but the user-visible loot **table** is
  de-duplicated by the `survey_chat`-wins `NOT EXISTS` guard. Real users are
  chat-only (the whole reason for this work), so no double-count in practice.
- The harmless `Cargo.toml` LF↔CRLF artifact reappeared (dev server re-touches it);
  **left uncommitted** — `git checkout -- src-tauri/Cargo.toml` to clear.

### Repo state at end of session

- `dev` @ **`34160f8`**, pushed. `main` @ **`5cab469`** (merge of dev onto the
  v0.9.13 release commit), pushed. Version strings unchanged at **v0.9.13** (feature
  merge, not a release). Working tree clean (modulo the Cargo.toml EOL artifact).
- Memory written: `memory/project_survey_chat_loot.md`.
- `npm run survey-test` (accuracy replay) **couldn't run locally** — it needs
  `docs/samples/CDN-full-examples/items.json`, absent in this checkout (panics before
  replaying; unrelated to this change).

---

## Session 8 — Statehelm widget skill-driven rework + gift-count fixes (2026-06-19)

**Outcome:** The dashboard **Statehelm Gifting** widget (`statehelm-summary`) now
surfaces the NPCs that train the player's most-relevant skills instead of an
alphabetical "fewest gifts first" list, and two real bugs in gift tracking were
root-caused and fixed. All type-checked (`vue-tsc` clean), `cargo test` green (107
parser tests incl. 2 new), and **verified live** by the user.

### ✨ Widget rework — `useStatehelmTracker.ts` + `StatehelmSummaryWidget.vue`

Two labeled sections driven by the player's own skills:
- **Combat:** NPCs for the **2 equipped** combat skills + the player's **top 4**
  combat skills (by **base level**) — up to `4 + equipped` slots, equipped first and
  marked with a gold ✦.
- **Non-Combat:** NPCs for the player's **top 2** non-combat skills.

Design rules (decided with the user):
- **Skill → combat/non-combat** comes from the CDN `combat` flag (`get_all_skills`,
  cached module-level so both StatehelmView + the widget share one fetch).
- **NPC category = "combat wins ties"**: an NPC that trains *any* combat skill is a
  combat NPC; only purely non-combat NPCs are eligible for the non-combat section.
- **Skill → NPC**: when several Statehelm NPCs train a skill (e.g. Geology → 3), the
  one with the **highest current favor standing** represents it (`tierIndex`).
- **Falloff + backfill**: `representativeFor()` skips any NPC already at 5/5 this
  week, so when *any* NPC (equipped included) is maxed it drops off and the
  next-highest skill of that category backfills the slot — the combat section walks
  equipped-first then ranked, capped at `4 + equipped`, deduped by NPC.

### 🐞 Fix 1 — Statehelm Sewers NPCs no longer appear

`statehelmNpcs` filtered on `area.includes('statehelm')`, which also matched
**"Statehelm Sewers"** (`AreaName == AreaStatehelmCaves`) — home to the Pig/Rabbit
animal-form trainers (Hamilton, Fuzzlebun) that have **no** weekly 5-gift cap. Filter
now requires `area_name === 'AreaStatehelm'` exactly (the city proper, 63 NPCs).

### 🐞 Fix 2 — bulk-gifting a stack only counted as 1

Root cause: gifting a **stack** of N identical items emits a **single**
`ProcessDeltaFavor(npcId, "NPC_X", delta, True)` line with N× the per-item favor
(confirmed: Corinth `58.464 = 5 × 11.6928`). The gift log counted one row per favor
event → undercount. **But the game prints the authoritative count** in the gift
dialog: `<Npc> will accept up to <b>5</b> gifts per calendar week and has received
<b>N</b> so far this week` (in `ProcessTalkScreen` post-gift / `ProcessPromptForItem`
pre-gift).

- **`player_event_parser.rs`** — new `PlayerEvent::GiftCountObserved { npc_name,
  received, max }` + `parse_gift_count` (matches the sentence substring, walks back
  from `" will accept up to <b>"` past the last `". "` to capture multi-word names
  like "Sir Brooker"). Dispatched before the `ProcessTalkScreen` branch. 2 unit tests.
- **`game_state.rs`** — `GiftCountObserved` arm **reconciles** that NPC's gift-log
  rows for the current week (Monday 00:00 UTC, new `current_week_start_utc()` helper)
  up/down to the game's exact count, leaving other NPCs and the manual +/- controls
  intact. Coexists with the existing per-`FavorChanged` insert (which still covers
  non-Statehelm gifts): the reconcile is absolute, not additive, so no double count.
  - **Note:** the reconcile only fires when the game re-emits that note (i.e. next
    time you open/complete a gift dialog for that NPC) — it won't retroactively fix a
    gift given before this build without re-opening the NPC's gift dialog.

### Files touched

- `src/composables/useStatehelmTracker.ts` (sewer filter + skill-driven targets)
- `src/components/Dashboard/widgets/StatehelmSummaryWidget.vue` (two-section UI)
- `src-tauri/src/player_event_parser.rs` (`GiftCountObserved` + parser + tests)
- `src-tauri/src/game_state.rs` (reconcile handler + `current_week_start_utc`)
- `docs/features/screens/dashboard/widget-statehelm-summary.md` (behavior doc)

### ⚠️ Gotcha discovered (dev-loop tooling)

Running `cargo test` while `npm run tauri dev` is mid-rebuild **collides on the
`target/` build lock** — it wedged the dev process and the app exited. If you need to
run tests during a live session, stop the dev build first (or expect to relaunch it).
Cleanup used here: kill `cargo`/`glogger`, free port 1420, relaunch `npm run tauri dev`.

---

## Session 7 — Flatpak CI resurrection + v0.9.12 prep (2026-06-20)

**Outcome:** The Flatpak bundle hadn't attached to a release since v0.9.2. Root-
caused it (two separate problems), fixed the real gap, verified the full
pipeline live against two real releases, committed the uncommitted button work,
and synced `dev`→`main` so v0.9.12 picks everything up.

### 🔎 Why the Flatpak "bricked" (two problems, not one)

1. **`gh: command not found` (the visible v0.9.2 failures).** The build always
   *succeeded* — it produced a valid `glogger.flatpak` (~7.8 MB) and uploaded it
   as a workflow artifact. The only failure was the final `gh release upload`
   step, which ran **inside the GNOME-47 flatpak-builder container** where the
   `gh` CLI doesn't exist. This was **already fixed** back in commit `fadbc47`
   (two-job split: build in container, `attach` on a bare runner) — but that fix
   landed *after* the v0.9.2 runs and had never actually been exercised.

2. **It never auto-triggered after v0.9.2 (the real, unfixed gap).** `release.yml`'s
   `prepare` job pushes the tag with `git push origin main --tags` using the
   default `GITHUB_TOKEN`. **GitHub suppresses workflow triggers from tags pushed
   by `GITHUB_TOKEN`** (anti-recursion), so `flatpak.yml`'s `push: tags` trigger
   never fired for v0.9.5/8/9/10/11. v0.9.2 only worked because that tag was
   pushed *manually* from a Linux box (a real user push, which does trigger).

### ✅ The fix (committed `5a92271`, on `main`)

- **`flatpak.yml`** — added a `workflow_call` trigger (+ a required `tag` input on
  `workflow_dispatch`), and a top-level `TARGET_TAG: ${{ inputs.tag || github.ref_name }}`
  resolver that works for all three triggers. The build job now `checkout`s
  `TARGET_TAG`; the `attach` job derives `TAG` from it and runs for any trigger
  (`if: inputs.tag != '' || startsWith(github.ref, 'refs/tags/')`).
- **`release.yml`** — new `flatpak` job (`needs: [prepare, publish]`) that
  **calls** `flatpak.yml` via `uses:` + `with: { tag: <new tag> }`, bypassing the
  anti-recursion block entirely. Every future release now builds + attaches a
  Flatpak with **zero manual steps**.

### ✅ Verified live (real releases, not exp)

- Dispatched the (already-fixed) two-job workflow against **v0.9.10** → built +
  attached `glogger.flatpak` (7.85 MB). Both jobs green.
- Same against **v0.9.11** → attached `glogger.flatpak` (7.84 MB). Both jobs green.
- So v0.9.10 and v0.9.11 now *have* Flatpaks; the build mechanics + `gh`-fix are
  proven end-to-end. The **auto-trigger `workflow_call` path itself** is YAML-valid
  and on `main` but its *first real exercise* will be the v0.9.12 Release run. If
  it hiccups, it'll be in that run's `flatpak` job; fallback is still
  `gh workflow run Flatpak --ref vX.Y.Z` (uses the `tag` dispatch input).

### ✅ Also committed this session

- **`aa26337` feat(menubar): one-click live-tailing toggle** — the Session-6
  uncommitted 📡 button is now committed (vue-tsc clean). No behavior change from
  the Session-6 description.
- Reverted the harmless `Cargo.toml` CRLF-only artifact instead of committing it.

### Repo state at end of session

- `dev` and `main` both at **`9ac3036`**, working tree clean, version strings at
  **v0.9.11** (merged the `release: v0.9.11` bump from main into dev, then
  advanced main to dev's tip — they're identical now).
- **Ready for v0.9.12:** trigger the **Release** workflow as usual; it will now
  also produce the Linux `.flatpak` automatically.

### ⚠️ Gotcha for next time

- A literal `git push origin dev:main` fast-forward was **not** possible because
  `main` carried the bot's `release: v0.9.11` version-bump commit that `dev`
  lacked. Resolution: `git merge origin/main` into `dev` first (absorbs the bump,
  conflict-free — only version strings), then push `dev:main`. Expect this every
  release, since the Release workflow commits the bump to `main` only.

---

## Session 6 — Gourmand live tracking + live-tailing button (2026-06-19)

**Committed/pushed:** Gourmand real-time eaten-food tracking — the open
investigation from Session 5 is **solved**. **In the working tree
(uncommitted):** an "all-in-one" live-tailing toggle button.

### ✅ Gourmand live tracking (committed `b635038`, pushed to origin/dev)

The Session 5 blocker ("no reliable signature for food-eaten in Player.log")
turned out to have a clean answer that needs **no** instance-ID/effect-ID
resolution at all:

```
[23:19:03] LocalPlayer: ProcessDoDelayLoop(1.5, Eat, "Using Human-Style Pizza with Tofu", 5845, AbortIfAttacked)
```

`ProcessDoDelayLoop(1.5, Eat, "Using <Food Name>", …)` fires **exactly once per
food eaten**, with the food name in **plain text**. The parser already emits
this as `PlayerEvent::DelayLoopStarted { action_type: "Eat", label, .. }`.
Confirmed against the user's live log with all 3 foods eaten that day
(Human-Style Pizza with Tofu, Candied Evu Fruit, Ratkin Survival Cheese).

- **`db/gourmand_commands.rs`** — new `record_food_eaten(conn, food_name)`:
  `INSERT … ON CONFLICT(food_name) DO UPDATE SET times_eaten = times_eaten + 1`.
  Leaves `manually_marked` untouched so a manual flag survives a live update.
- **`coordinator.rs`** — new match arm on the `PlayerEvent` side-channel:
  `DelayLoopStarted { action_type, label, .. } if action_type == "Eat"` strips
  the `"Using "` prefix and calls `record_food_eaten`, then emits the existing
  `gourmand-updated` event. The frontend `gourmandStore` already listens for
  that event, so the UI refreshes live with **zero frontend changes**.
- **Coexists with the `/report gourmand` import** (no double-count): report
  import still does `DELETE WHERE manually_marked = 0` + full resync, which
  cleanly reconciles any live increments to the authoritative report numbers.
- `cargo check` clean; all 392 lib tests pass.

### 🚧 All-in-one live-tailing button (UNCOMMITTED in working tree)

Goal: one header button (gold 📡, left of the ⚙ gear) to enable/disable live
tailing of **all** log files at once, and to bake in the old manual workaround
("save a fresh file in-game, then refresh the app"). User confirmed **both**
halves of that workaround were needed.

- **glogger can only automate the "refresh" half** — it reads files; it cannot
  make Project Gorgon flush fresh data to disk. So the button does the
  automatable half and shows a toast reminding the user to do the in-game save.
- **`MenuBar.vue`** — new 📡 button; `isAllTailing` computed (both watchers
  active); `toggleAllTailing()`: if all on → stop both; else start whichever
  aren't running → `startPolling()` → `pollWatchers()` (forced catch-up read,
  the "refresh" half) → `refreshStatus()` → info toast. Wired in `useToast`.
- **`coordinatorStore.ts`** — new `pollWatchers()` action wrapping the existing
  backend `poll_watchers` command (already registered in `lib.rs`).
- `vue-tsc --noEmit` clean. **Not yet verified live** — testing was blocked
  (see gotcha below). Open question for the user: the reminder toast currently
  fires on *every* enable; consider gating it to once-per-session or a
  "don't remind me again" pref if noisy.

### ⚠️ Gotchas discovered this session

- **Two `glogger.exe` installs confuse computer-use.** Start-menu "glogger"
  resolves to a **portable install** at `a:\portableapps\glogger\glogger.exe`,
  which is a release version **behind** the dev build. The dev build runs from
  `src-tauri\target\debug\glogger.exe` (different path) → computer-use masks the
  dev window because the grant locked onto the portable exe. To drive the dev
  build via computer-use, close the portable install or grant the
  `target\debug` exe by exact path/basename. Alternative used here: watch the
  dev build's stdout log for catch-up/poll lines while the user clicks.
- `npm run tauri dev` auto-rebuilds on `Cargo.toml` change (picked up the
  v0.9.10 version bump without a manual restart).

### Repo state at end of session

- `dev` fast-forwarded to **v0.9.10** (origin/main had released it; it was just
  version-string bumps on work dev already had). `origin/dev` now also at
  v0.9.10 after the push.
- `dev` is clean except the **uncommitted button work** (`MenuBar.vue`,
  `coordinatorStore.ts`) + a harmless `Cargo.toml` CRLF artifact (LF↔CRLF only,
  no content change — safe to `git checkout --`).

---

## Session 5 — Words of Power: wiki-sourced categories + CSV export (2026-06-19)

**Outcome:** Widget now groups discovered words by category → level (instead of one flat list per
power name), and can export the full word list to CSV (date/time discovered, word, power name,
category, level). `cargo check`, `cargo test`, and `vue-tsc --noEmit` all clean.

- **New: `src-tauri/src/db/word_of_power_catalog.rs`** — static `(power_name → category, level)`
  lookup table derived from https://wiki.projectgorgon.com/wiki/Words_of_Power (per the user's
  direction to use the wiki as source of truth). The wiki organizes scrolls into six tiers (levels
  0/3/5/7/9/19); several effect names recur across tiers with longer durations at higher tiers
  (e.g. "Super Jumping"). Since the discovery log line only gives the effect name — not which tier
  scroll produced it — each name is pinned to the **lowest** tier it appears at (documented in the
  file's module doc-comment as a deliberate, acknowledged approximation). Unknown names fall back
  to `category: "Unknown", level: None` rather than failing. Two unit tests cover known/unknown
  lookups.
- **`words_of_power_commands.rs`** — `WordOfPower` struct gained `category: String` and
  `level: Option<u32>`; both `get_words_of_power` and `add_word_of_power` now construct rows via a
  new `WordOfPower::with_catalog_lookup(...)` helper that calls into the catalog.
- **`WordsOfPowerWidget.vue`** — regrouped: top-level collapsible **category** sections, each with
  a **Level N** (or "Level unknown") sub-heading, individual words listed underneath (now showing
  `power_name` inline since it's no longer the group header). New **Export to CSV** button next to
  the word list, reusing the existing generic `export_text_file` command + `@tauri-apps/plugin-dialog`
  `save()` pattern (same approach as `craftingStore.ts`/`gourmandStore.ts`).

### Dropped this session: Gourmand real-time tracking investigation

Explored whether "food eaten" could be detected live from Player.log (currently requires a manual/
auto import of the in-game `/report gourmand` text file). Findings, in case this gets picked up
again:
- `DeleteContext::Consumed` in `player_event_parser.rs` is defined but **never constructed** —
  there's no existing signal.
- Found a plausible candidate pattern in a live log window (~21:21–21:24 same-day): an item delete
  immediately followed by `ProcessAddEffects`, occasionally paired with a Gourmand `ProcessUpdateSkill`
  XP gain. But the item's instance ID had no prior `ProcessAddItem` in that session (item was already
  in inventory before tailing started), so it never resolved to a name in `item_transactions`, and the
  corresponding effect ID wasn't in `game_state_effects` either (whose `effect_name` column is mostly
  `NULL` anyway — effect-ID→name resolution is incomplete project-wide).
- Conclusion: no reliable signature is in hand yet. To pin one down, would need either (a) exact
  food name + wall-clock time from the user to search a narrow window, or (b) tail the live log while
  the user eats something with a fresh PlayerLog session (so the item has a resolvable AddItem first).
  **Do not guess at a heuristic from ambiguous data** — risks corrupting the eaten-foods table.

---

# Previous Session

**Date:** 2026-06-19
**Machine:** Windows 11 (primary dev box)
**Branch:** `dev` (created from `main` @ v0.9.9; both synced to v0.9.9)
**Outcome:** **Economics → Farming** session overhaul. The active-session item view is now a
4-column layout — **Skills | Looted Items | Gathered | Activity Log** — where each item is
hover-interactable (0.5s) and pops a per-source drop-rate breakdown. New backend
`CorpseExtract` event distinguishes skinning/butchering yields from loot-table drops. Mining
and survey gains are tracked by source in the Gathered column. Type-checked; 105 parser tests
pass; verified live in the dev build.

---

## TL;DR (this session)

- **Looted Items** column = true corpse loot only (`LootPickedUp`, i.e. `ProcessRemoveLoot`).
  Headline = total quantity looted this session. Hover → [ItemDropBreakdown](src/components/Farming/ItemDropBreakdown.vue):
  per-enemy session drops **+ all-time drop rate & loot-table share** from the DB
  (`get_enemy_kill_stats`).
- **Gathered** column = everything that isn't a loot-table drop, tagged by source skill:
  `SKINNING`/`BUTCHERING` (corpse extracts), `MINING`, `SURVEY`. Hover → session-only per-source
  breakdown (no all-time DB data exists for these).
- Removed the old kill-gate that silently dropped corpse loot when the kill wasn't tracked this
  session — loot now always shows (creates the enemy entry with `count: 0`).

## What is a "looted item" vs not (decided with the user)

- **Looted Items** = `LootPickedUp` only (fires from the corpse loot window; excludes
  skinning/butchering, which grant Butchering/Skinning XP and produce **no** `ProcessRemoveLoot`).
- **Gathered** = skinning/butchering extracts + Mining/SurveyMapUse provenance gains. Filtered by
  **provenance, not item type** — so it captures everything a node/survey yields, not just things
  named "ore/metal." (User accepted this; motherlodes often log no node name → bucket
  "Mining (unknown node)".)
- Vendor buys, storage withdrawals, and craft outputs are excluded from both columns (they still
  appear in the Activity Log via `ItemAdded`/`ItemStackChanged`).

## Implementation (this session)

- **Backend** — [player_event_parser.rs](src-tauri/src/player_event_parser.rs):
  - New `PlayerEvent::CorpseExtract { item_name, item_type_id, quantity, skill, corpse_name }`.
  - `parse_corpse_extract` hooks `ProcessUpdateSkill` lines: when `type=Butchering`/`Skinning`,
    the just-added item (`last_item_event`) is the extract — emit `CorpseExtract` and consume it.
  - `parse_remove_loot` now sets `last_item_event = None` after resolving, so a true loot drop can
    never leak into the extract path.
  - Coordinator needs no change — `PlayerEventParsed` events are batched/forwarded regardless of
    kind (`_ => {}` arms); `CorpseExtract` is **not** persisted to the DB (session-only).
- **Frontend store** — [farmingStore.ts](src/stores/farmingStore.ts):
  - `LootPickedUp` handler de-gated (records even for untracked kills; falls back to
    "Unknown enemy" when no corpse-search context).
  - `CorpseExtract` handler + `recordGathered(s, provenance, item, qty)` helper that routes
    Mining/SurveyMapUse gains into `extracts` keyed by source (node name / survey map).
  - `s.extracts: Record<sourceName, Record<itemName, { quantity, drops, skill }>>` (new session
    field). Computeds: `lootedItems`, `extractedItems`; helpers: `sessionEnemiesForItem`,
    `sessionEnemiesForExtract`, `fetchEnemyStats` (DB cache keyed by enemy name).
  - Loot tally type changed to `{ quantity, drops }` (per-item drop count for drop-rate math).
- **Frontend UI**:
  - [FarmingSessionCard.vue](src/components/Farming/FarmingSessionCard.vue) — grid is now
    `240px 1fr 1fr 280px`; Looted Items + Gathered are separate boxes; each row uses
    `EntityTooltipWrapper` (`:delay="500"`, interactive) with the breakdown in `#tooltip`. Item
    names are resolved to display names via a local `displayName()` (plain text, **not**
    `ItemInline`, to avoid a competing nested tooltip on the hover target).
  - [ItemDropBreakdown.vue](src/components/Farming/ItemDropBreakdown.vue) — `mode: 'loot' | 'extract'`.
    Loot mode fetches all-time DB stats and shows a drop-rate bar + "% of table"; extract mode is
    session-only (source shown as plain text since it may be a node/survey, not an enemy).

## Caveats / known limits

- `CorpseExtract`/Gathered are **session-only** — no all-time history (the `enemy_kill_loot` DB
  table only records `LootPickedUp`).
- `ItemAdded` carries no quantity to the frontend, so new-stack gains count as `+1` (existing
  `itemDeltas` behavior); stack growth via `ItemStackChanged` uses the real delta.
- Survey source names are the raw internal map name (not yet resolved to a display name).
- A stale HMR session object (created before `extracts` existed) once blanked the panel via
  `Object.values(undefined)`; computeds/handlers now guard with `?? {}`. A clean restart clears it.

## Verification (this session)

1. `npx vue-tsc --noEmit` clean; `cargo test --lib player_event_parser` → 105 passed.
2. Confirmed against a real Player.log: corpse loot (`Goblin Calling Card`, `Goblin Skirt`,
   `Health Potion`) → Looted Items; butchering extracts (`Goblin Skull`, `Impressive Goblin
   Skull`, which have Butchering XP and no `RemoveLoot`) → Gathered.
3. App runs interactive in the dev build; Start Session + both columns render.

---

# Previous Session

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
