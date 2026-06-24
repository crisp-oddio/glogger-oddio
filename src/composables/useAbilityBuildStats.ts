// Per-ability "effective stats under this build" resolver for the Build Planner.
//
// Hovering an equipped ability needs the ability's combat stats folded together with
// the build's assigned gear mods (effective damage/heal/DoT/cost). The math lives in the
// Rust `compute_ability_build_stats` command; this composable just calls it lazily and
// caches the result per ability id, invalidating whenever the build's mods change.
//
// Uses the same detached module-level singleton pattern as useBuildModEffects so the
// cache survives component unmounts and stays shared across the bar's slots.

import { ref, effectScope, watch } from "vue";
import { useBuildPlannerStore } from "../stores/buildPlannerStore";
import { useGameDataStore } from "../stores/gameDataStore";
import { slotLabel } from "./useBuildModEffects";
import type { AbilityBuildStats, AbilityModRef } from "../types/abilityStats";

const cache = ref<Record<number, AbilityBuildStats | null>>({});
const inFlight = new Set<number>();
let scope: ReturnType<typeof effectScope> | null = null;

export function useAbilityBuildStats() {
  const store = useBuildPlannerStore();
  const gameData = useGameDataStore();

  /** The build's assigned mods, shaped for the backend command. */
  function currentMods(): AbilityModRef[] {
    return store.presetMods
      .filter((m) => m.tier != null)
      .map((m) => ({
        power_name: m.power_name,
        tier: m.tier as number,
        slot_label: slotLabel(m.equip_slot),
      }));
  }

  async function load(abilityId: number) {
    if (inFlight.has(abilityId)) return;
    inFlight.add(abilityId);
    try {
      const result = await gameData.computeAbilityBuildStats(abilityId, currentMods());
      cache.value = { ...cache.value, [abilityId]: result };
    } catch {
      cache.value = { ...cache.value, [abilityId]: null };
    } finally {
      inFlight.delete(abilityId);
    }
  }

  /**
   * Reactive accessor: returns the cached stats for an ability, kicking off a fetch on
   * first access. Returns `undefined` until loaded, `null` if the ability has no stats.
   */
  function statsForAbility(abilityId: number): AbilityBuildStats | null | undefined {
    if (!(abilityId in cache.value) && !inFlight.has(abilityId)) {
      void load(abilityId);
    }
    return cache.value[abilityId];
  }

  /** Warm the cache for a set of abilities (e.g. all equipped bar abilities). */
  function prefetch(ids: number[]) {
    for (const id of ids) {
      if (!(id in cache.value) && !inFlight.has(id)) void load(id);
    }
  }

  // Invalidate the whole cache when the build's mods change (the reference changes on
  // any mod edit or preset switch).
  if (!scope) {
    scope = effectScope(true);
    scope.run(() => {
      watch(
        () => store.presetMods,
        () => {
          cache.value = {};
          inFlight.clear();
        },
      );
    });
  }

  return { cache, statsForAbility, prefetch };
}
