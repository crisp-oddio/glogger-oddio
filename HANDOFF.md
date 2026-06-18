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
