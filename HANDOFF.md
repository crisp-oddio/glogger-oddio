# glogger — Session Handoff

**Date:** 2026-06-18
**Machine:** Windows 11 (primary dev box)
**Outcome:** Crafting leveling XP math corrected; **v0.9.3** released via the Release workflow with all five installers (Windows `.exe`, macOS `.dmg`, Linux `.deb` + `.AppImage`, and the Linux `.flatpak`).

---

## TL;DR

- Fixed three crafting **XP Leveling Optimizer** bugs (`src/components/Crafting/LevelingTab.vue`).
- Verified the fixes live in the running dev build, not just in tests.
- Hardened the Flatpak `attach` job so the bundle reliably lands on releases created by `release.yml`.
- Cut **v0.9.3** through the Release workflow so every platform installer is built and attached.

---

## The crafting XP fixes

Project Gorgon's crafting XP rules (confirmed by Cheb/oddio, who designs this) that the optimizer was getting wrong:

1. **First-craft total was double-counted.** The CDN's `RewardSkillXpFirstTime` is the *total* XP for the
   first craft (e.g. 40), **not** an additive bonus. The old code did `base + firstTime` (10 + 40 = 50).
   Fixed: the first-time *bonus* is `firstTime − base` (30), so the first craft totals the correct 40.

2. **First-time bonus is static.** It is **not** scaled by XP buffs and **not** reduced by the over-level
   XP drop-off. Only the per-craft *base* XP gets buff + drop-off applied. So `effectiveFirstTimeXp` is
   the raw bonus, with no multiplier and no drop-off.

3. **Synergy/bonus levels must not count toward the XP drop-off.** XP is earned as if at your *base* skill
   level. Example: base 6 + 5 synergy = effective 11, but XP is calculated as level 6. Split the two uses
   of level — `planningLevel` (base + synergy) still gates recipe *unlock*, while the new `planningBaseLevel`
   drives the drop-off (`src/utils/craftingXp.ts` now documents that it takes the base level).

**Display:** the recipe list shows a single combined XP number — the first-craft total (gold) while the
first-time bonus is still available, then the base XP (muted) once the recipe has been crafted. Hover for
the full breakdown.

These rules apply uniformly across all crafting disciplines.

## Release / CI note

`release.yml` (workflow_dispatch) is the only path that builds Windows/macOS/Linux and creates the release;
a bare tag push only triggers `flatpak.yml`. Pushing a tag manually is what left **v0.9.2 without a Windows
`.exe`** — do not do that. Always run the **Release** workflow.

The Flatpak `attach` job now waits (up to 30 min, polling) for the release to exist before
`gh release upload`, because `release.yml` pushes the tag early (prepare job) but only creates the release
after its ~15-20 min multi-platform build — well after the ~9.5 min Flatpak build finishes.

## Open items / next steps

- The same XP rules (static first-time bonus, base-level drop-off) likely belong in **Quick Calc** and any
  live skill-XP tracking, but `xpDropOffMultiplier` is currently only wired into the Leveling tab. Those
  other surfaces don't apply drop-off at all yet — worth a follow-up pass.
