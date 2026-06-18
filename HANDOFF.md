# glogger — Session Handoff

**Date:** 2026-06-18
**Machine:** Windows 11 (primary dev box)
**Outcome:** Crafting leveling XP math corrected (again) to the right model — first craft is a flat 4× base, with diminishing returns applying to repeat crafts only. Verified live in the dev build.

---

## TL;DR

- Corrected the crafting **XP Leveling Optimizer** math in `src/components/Crafting/LevelingTab.vue`.
- The v0.9.3 model (treat `RewardSkillXpFirstTime` as the first-craft total; static first-time bonus) was **wrong**. Replaced with the rules below.
- Verified the fixes live in the running dev build.

---

## The crafting XP model (corrected — supersedes v0.9.3)

Project Gorgon's crafting XP rules, confirmed by Cheb/oddio (who designs this):

1. **First craft = flat 4× base XP, total.** A recipe's first-ever craft awards exactly `base × 4`
   (e.g. base 10 → 40). The CDN `RewardSkillXpFirstTime` field is **not** used for this.

2. **The first-craft 4× total is fixed.** It is **not** scaled by the XP buff and **not** reduced by the
   over-level drop-off. It is always `base × 4`.

3. **Diminishing returns apply to repeat crafts only.** Crafts 2..N award `base × buff × dropOff`. The
   XP buff and the over-level drop-off both apply here — never to the first craft.

4. **Synergy/bonus levels don't count toward XP.** XP (and the drop-off) is computed from the *base* skill
   level. `planningLevel` (base + synergy) gates recipe *unlock*; `planningBaseLevel` drives the drop-off.

**Implementation:** `FIRST_CRAFT_XP_MULTIPLIER = 4`. `effectiveXp = round(base × buff × dropOff)` is the
repeat-craft value; the first-time bonus is stored as `base×4 − effectiveXp` so that
`effectiveXp + firstTimeXp` always lands on the flat `base × 4` regardless of buff/drop-off. The over-level
drop-off curve lives in `src/utils/craftingXp.ts` (full XP until recipe level + 10, linear decline to 0 at
recipe level + 65).

**Display:** the recipe list shows a single combined XP number — the first-craft total (gold) while the
first-time bonus is still available, then the repeat XP (muted) once the recipe has been crafted. Hover for
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

- The same XP rules (flat 4× first craft, repeat-craft drop-off) likely belong in **Quick Calc** and any
  live skill-XP tracking, but `xpDropOffMultiplier` is currently only wired into the Leveling tab. Quick
  Calc's `resolveRecipeIngredients` sets `xp_first_time = base × 3` (so first craft = 4×) but does not model
  the repeat-craft drop-off yet — worth a follow-up pass.
