/**
 * Crafting XP drop-off ("experience scaling") helpers.
 *
 * In Project Gorgon a recipe rewards full skill XP until your skill level
 * reaches the recipe's drop-off level (always recipe level + 10 in the game
 * data). Past that point the reward declines smoothly — roughly linearly —
 * down to zero at 65 levels above the recipe level (i.e. 55 levels past the
 * drop-off level).
 *
 * This was derived from in-game observation rather than documented values:
 * "Reinforce Metal Pants" (recipe level 15, base 60 XP, drop-off 25) awards
 * 11 XP at skill level 70 — i.e. 60 × (1 − 45/55) ≈ 10.9. A "10% per 5 levels"
 * step model is ruled out because it can only ever yield multiples of 10% of
 * the base (the in-game 11 is not such a multiple).
 *
 * The reduction never makes XP negative (multiplier is clamped to 0..1).
 */

/**
 * Levels over which the reward declines from full (at the drop-off level) to
 * zero. The drop-off level is recipe level + 10, so zero XP is reached at
 * recipe level + 65.
 */
const DROP_OFF_SPAN = 55;

/**
 * The XP reward multiplier (0..1) for crafting a recipe at the given skill
 * level, based on the recipe's drop-off level.
 *
 * @param skillLevel   The crafter's effective skill level (incl. bonus levels).
 * @param dropOffLevel The recipe's `reward_skill_xp_drop_off_level` (recipe
 *                     level + 10). When null (no drop-off data) full XP is
 *                     assumed.
 */
export function xpDropOffMultiplier(
  skillLevel: number,
  dropOffLevel: number | null | undefined,
): number {
  if (dropOffLevel === null || dropOffLevel === undefined) return 1;

  const levelsPast = skillLevel - dropOffLevel;
  if (levelsPast <= 0) return 1; // full XP through the drop-off level

  const mult = 1 - levelsPast / DROP_OFF_SPAN;
  return Math.max(0, Math.min(1, mult));
}
