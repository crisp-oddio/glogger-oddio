// Shared resolution of build-planner mod effects and the TSys↔Ability mapping.
//
// Both the Build Summary's "By Ability" view and the ability-bar hover tooltips
// need the same data: each assigned mod's structured effects, and which mods
// reference which abilities. Resolving it in one place (a module-level singleton
// driven off the active preset's mods) avoids duplicate backend round-trips and
// keeps the two views in sync.

import { ref, effectScope, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useBuildPlannerStore } from "../stores/buildPlannerStore";
import { useGameDataStore } from "../stores/gameDataStore";
import { EQUIPMENT_SLOTS, type BuildPresetMod } from "../types/buildPlanner";
import { formatStatValue } from "./useBuildStats";

export interface StructuredEffect {
  label: string;
  value: string;
  displayType: string;
  formatted: string;
  iconId: number | null;
}

/** A single resolved effect line attributed to its source mod. */
export interface AbilityEffectEntry {
  label: string;
  numericValue: number;
  formattedValue: string;
  iconId: number | null;
  source: string;
}

/** A mod that applies to a given ability, with its resolved effects. */
export interface AbilityModEntry {
  modKey: string;
  name: string;
  slotLabel: string;
  isAugment: boolean;
  effects: AbilityEffectEntry[];
}

// ── Shared (module-level) state ──────────────────────────────────────────────
const loading = ref(false);
const resolvedEffects = ref<Record<string, string[]>>({});
const resolvedNames = ref<Record<string, string>>({});
const modSkills = ref<Record<string, string | null>>({});
const structuredEffects = ref<Record<string, StructuredEffect[]>>({});
/** power_name (internal name) → ability ids the mod references */
const tsysAbilityMap = ref<Record<string, number[]>>({});

let scope: ReturnType<typeof effectScope> | null = null;

export function modKey(mod: BuildPresetMod): string {
  return `${mod.power_name}:${mod.tier ?? 0}`;
}

export function slotLabel(slotId: string): string {
  return EQUIPMENT_SLOTS.find((s) => s.id === slotId)?.label ?? slotId;
}

export function useBuildModEffects() {
  const store = useBuildPlannerStore();
  const gameData = useGameDataStore();

  async function resolve() {
    loading.value = true;
    const effects: Record<string, string[]> = {};
    const skills: Record<string, string | null> = {};
    const names: Record<string, string> = {};
    const structured: Record<string, StructuredEffect[]> = {};
    try {
      for (const mod of store.presetMods) {
        if (mod.tier == null) continue;
        const key = modKey(mod);
        if (effects[key]) continue;
        try {
          const info = await invoke<{
            internal_name: string;
            skill: string | null;
            prefix: string | null;
            suffix: string | null;
            tier_effects: string[];
            tier_effects_structured: Array<{
              label: string;
              value: string;
              display_type: string;
              formatted: string;
              icon_id: number | null;
            }>;
          } | null>("get_tsys_power_info", { powerName: mod.power_name, tier: mod.tier });
          if (info) {
            if (info.tier_effects) effects[key] = info.tier_effects;
            skills[key] = info.skill;
            const displayName = info.prefix ?? info.suffix ?? mod.power_name;
            if (displayName !== mod.power_name) names[key] = displayName;
            if (info.tier_effects_structured) {
              structured[key] = info.tier_effects_structured.map((e) => ({
                label: e.label,
                value: e.value,
                displayType: e.display_type,
                formatted: e.formatted,
                iconId: e.icon_id,
              }));
            }
          }
        } catch {
          // Power might not resolve
        }
      }

      // Cross-reference mods → abilities via the precomputed backend index.
      const powerNames = [...new Set(store.presetMods.map((m) => m.power_name))];
      let map: Record<string, number[]> = {};
      if (powerNames.length > 0) {
        try {
          map = await gameData.getTsysAbilityMap(powerNames);
        } catch {
          /* ignore */
        }
      }
      tsysAbilityMap.value = map;
    } finally {
      resolvedEffects.value = effects;
      modSkills.value = skills;
      resolvedNames.value = names;
      structuredEffects.value = structured;
      loading.value = false;
    }
  }

  // Register the driver once, in a detached scope so it survives the unmount of
  // whichever component first calls this composable.
  if (!scope) {
    scope = effectScope(true);
    scope.run(() => {
      watch(() => store.presetMods, () => resolve(), { immediate: true });
    });
  }

  /** Resolved effect lines (with source attribution) that target an ability. */
  function effectsForAbility(abilityId: number): AbilityEffectEntry[] {
    const out: AbilityEffectEntry[] = [];
    for (const mod of store.presetMods) {
      const abilityIds = tsysAbilityMap.value[mod.power_name];
      if (!abilityIds || !abilityIds.includes(abilityId)) continue;
      const key = modKey(mod);
      const modEffects = structuredEffects.value[key];
      if (!modEffects) continue;
      const sourceName = `${resolvedNames.value[key] ?? mod.power_name} (${slotLabel(mod.equip_slot)})`;
      for (const e of modEffects) {
        const num = parseFloat(e.value) || 0;
        out.push({
          label: e.label,
          numericValue: num,
          formattedValue: formatStatValue(num, e.displayType),
          iconId: e.iconId,
          source: sourceName,
        });
      }
    }
    return out;
  }

  /** Mods (grouped) that apply to an ability, each with its effects. */
  function modsForAbility(abilityId: number): AbilityModEntry[] {
    const out: AbilityModEntry[] = [];
    for (const mod of store.presetMods) {
      const abilityIds = tsysAbilityMap.value[mod.power_name];
      if (!abilityIds || !abilityIds.includes(abilityId)) continue;
      const key = modKey(mod);
      const modEffects = structuredEffects.value[key] ?? [];
      out.push({
        modKey: key,
        name: resolvedNames.value[key] ?? mod.power_name,
        slotLabel: slotLabel(mod.equip_slot),
        isAugment: mod.is_augment,
        effects: modEffects.map((e) => {
          const num = parseFloat(e.value) || 0;
          return {
            label: e.label,
            numericValue: num,
            formattedValue: formatStatValue(num, e.displayType),
            iconId: e.iconId,
            source: "",
          };
        }),
      });
    }
    return out;
  }

  return {
    loading,
    resolvedEffects,
    resolvedNames,
    modSkills,
    structuredEffects,
    tsysAbilityMap,
    modKey,
    slotLabel,
    effectsForAbility,
    modsForAbility,
    resolve,
  };
}
