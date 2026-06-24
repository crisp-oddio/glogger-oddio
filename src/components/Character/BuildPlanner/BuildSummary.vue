<template>
  <div class="flex flex-col h-full overflow-y-auto px-4 py-3 space-y-5">
    <!-- Effects section with view tabs -->
    <div v-if="store.presetMods.length > 0">
      <!-- Tab bar -->
      <div class="flex items-center gap-1 mb-3">
        <button
          v-for="tab in VIEW_TABS"
          :key="tab.id"
          class="px-2.5 py-1 rounded text-xs font-semibold cursor-pointer transition-colors"
          :class="activeTab === tab.id
            ? 'bg-accent-gold/20 text-accent-gold'
            : 'text-text-muted hover:text-text-secondary'"
          @click="activeTab = tab.id">
          {{ tab.label }}
        </button>
      </div>

      <div v-if="loadingEffects" class="text-sm text-text-muted py-4 text-center">
        Loading effects...
      </div>

      <!-- By Skill view (original) -->
      <div v-else-if="activeTab === 'skill'" class="space-y-4">
        <div v-for="group in effectGroups" :key="group.label">
          <h4 class="text-sm font-semibold mb-2" :class="group.labelClass">
            {{ group.label }} ({{ group.mods.length }})
          </h4>
          <div class="space-y-2">
            <div
              v-for="mod in group.mods"
              :key="mod.id"
              class="flex items-start gap-3 text-sm pl-2 py-1 rounded"
              :class="mod.is_augment ? 'bg-purple-900/10' : ''">
              <span class="text-text-dim shrink-0 w-24 text-xs pt-0.5">{{ slotLabel(mod.equip_slot) }}</span>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5">
                  <span class="font-medium text-text-primary">{{ resolvedNames[modKey(mod)] ?? mod.power_name }}</span>
                  <span v-if="mod.is_augment" class="text-[10px] font-semibold text-mod-augment uppercase">AUG</span>
                </div>
                <div v-if="resolvedEffects[modKey(mod)]" class="mt-0.5">
                  <EffectLine v-for="(effect, i) in resolvedEffects[modKey(mod)]" :key="i" :text="effect" />
                </div>
              </div>
            </div>
          </div>
        </div>

        <div v-if="effectGroups.length === 0" class="text-sm text-text-dim text-center py-4">
          No mods assigned yet
        </div>
      </div>

      <!-- Effect Totals view -->
      <div v-else-if="activeTab === 'totals'" class="space-y-3">
        <div v-if="aggregatedEffects.length === 0" class="text-sm text-text-dim text-center py-4">
          No effects resolved yet
        </div>
        <div v-else class="space-y-1">
          <div
            v-for="agg in aggregatedEffects"
            :key="agg.label"
            class="flex items-center gap-2 py-1 px-2 rounded"
            :class="agg.count > 1 ? 'bg-surface-elevated' : ''">
            <EffectLine
              :label="agg.label"
              :formatted-value="agg.formattedValue"
              :numeric-value="agg.numericValue"
              :icon-id="agg.iconId"
              class="flex-1" />
            <span v-if="agg.count > 1" class="text-[10px] text-text-muted shrink-0">
              {{ agg.count }} sources
            </span>
          </div>
        </div>
      </div>

      <!-- By Ability view -->
      <div v-else-if="activeTab === 'ability'" class="space-y-2">
        <div v-if="abilityEffectGroups.length === 0" class="text-sm text-text-dim text-center py-4">
          {{ store.presetAbilities.length === 0 ? 'No abilities assigned to ability bars' : 'No mod effects reference your abilities' }}
        </div>
        <AbilityDamageCard
          v-for="group in abilityEffectGroups"
          :key="group.abilityName"
          :ability-name="group.abilityName"
          :effects="group.effects" />
      </div>
    </div>

    <div v-else class="text-sm text-text-dim text-center py-8">
      No mods assigned. Select equipment slots and add mods to see your build summary.
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useBuildPlannerStore } from '../../../stores/buildPlannerStore'
import type { BuildPresetMod } from '../../../types/buildPlanner'
import { formatStatValue } from '../../../composables/useBuildStats'
import { useBuildModEffects, modKey, slotLabel } from '../../../composables/useBuildModEffects'
import EffectLine from './EffectLine.vue'
import AbilityDamageCard from './AbilityDamageCard.vue'

const VIEW_TABS = [
  { id: 'skill', label: 'By Skill' },
  { id: 'totals', label: 'Effect Totals' },
  { id: 'ability', label: 'By Ability' },
] as const

type ViewTab = typeof VIEW_TABS[number]['id']

const store = useBuildPlannerStore()
const activeTab = ref<ViewTab>('skill')

// Resolved mod-effect data + the mod→ability cross-reference are shared with the
// ability-bar tooltips via this composable (single resolution, kept in sync).
const {
  loading: loadingEffects,
  resolvedEffects,
  resolvedNames,
  modSkills,
  structuredEffects,
  effectsForAbility,
} = useBuildModEffects()

interface EffectGroup {
  label: string
  labelClass: string
  mods: BuildPresetMod[]
}

const effectGroups = computed((): EffectGroup[] => {
  const primary = store.activePreset?.skill_primary
  const secondary = store.activePreset?.skill_secondary
  const groups: EffectGroup[] = []

  if (primary) {
    const primaryMods = store.presetMods.filter(m => modSkills.value[modKey(m)] === primary)
    if (primaryMods.length > 0) {
      groups.push({ label: primary, labelClass: 'text-blue-400', mods: primaryMods })
    }
  }

  if (secondary) {
    const secondaryMods = store.presetMods.filter(m => modSkills.value[modKey(m)] === secondary)
    if (secondaryMods.length > 0) {
      groups.push({ label: secondary, labelClass: 'text-emerald-400', mods: secondaryMods })
    }
  }

  const enduranceMods = store.presetMods.filter(m => {
    const skill = modSkills.value[modKey(m)]
    return skill === 'Endurance' && skill !== primary && skill !== secondary
  })
  if (enduranceMods.length > 0) {
    groups.push({ label: 'Endurance', labelClass: 'text-amber-400', mods: enduranceMods })
  }

  const genericMods = store.presetMods.filter(m => {
    const skill = modSkills.value[modKey(m)]
    return !skill || skill === 'AnySkill'
  })
  if (genericMods.length > 0) {
    groups.push({ label: 'Generic', labelClass: 'text-text-muted', mods: genericMods })
  }

  const grouped = new Set(groups.flatMap(g => g.mods.map(m => m.id)))
  const remaining = store.presetMods.filter(m => !grouped.has(m.id))
  if (remaining.length > 0) {
    groups.push({ label: 'Other', labelClass: 'text-text-dim', mods: remaining })
  }

  return groups
})

// ── Effect Totals ────────────────────────────────────────────────────────────

interface AggregatedEffect {
  label: string
  numericValue: number
  formattedValue: string
  displayType: string
  iconId: number | null
  count: number
}

const aggregatedEffects = computed((): AggregatedEffect[] => {
  const totals = new Map<string, AggregatedEffect>()

  for (const mod of store.presetMods) {
    const key = modKey(mod)
    const effects = structuredEffects.value[key]
    if (!effects) continue

    for (const effect of effects) {
      const numVal = parseFloat(effect.value) || 0
      const existing = totals.get(effect.label)
      if (existing) {
        existing.numericValue += numVal
        existing.count += 1
      } else {
        totals.set(effect.label, {
          label: effect.label,
          numericValue: numVal,
          formattedValue: '',
          displayType: effect.displayType,
          iconId: effect.iconId,
          count: 1,
        })
      }
    }
  }

  // Format aggregated values
  const results = Array.from(totals.values())
  for (const agg of results) {
    agg.formattedValue = formatStatValue(agg.numericValue, agg.displayType)
  }

  // Sort: highest absolute value first
  results.sort((a, b) => Math.abs(b.numericValue) - Math.abs(a.numericValue))
  return results
})

// ── By Ability view ──────────────────────────────────────────────────────────

interface AbilityEffectEntry {
  label: string
  numericValue: number
  formattedValue: string
  iconId: number | null
  source: string
}

interface AbilityEffectGroup {
  abilityName: string
  effects: AbilityEffectEntry[]
}

/**
 * Ability effect groups derived from the shared mod→ability cross-reference.
 * Recomputes automatically as the composable resolves effects / the build changes.
 */
const abilityEffectGroups = computed((): AbilityEffectGroup[] => {
  if (store.presetAbilities.length === 0 || store.presetMods.length === 0) return []

  // Unique assigned abilities (keep the first display name seen per id).
  const abilityNames = new Map<number, string>()
  for (const a of store.presetAbilities) {
    if (!abilityNames.has(a.ability_id)) {
      abilityNames.set(a.ability_id, a.ability_name ?? `Ability #${a.ability_id}`)
    }
  }

  const result: AbilityEffectGroup[] = []
  for (const [abilityId, name] of abilityNames) {
    const effects = effectsForAbility(abilityId)
    if (effects.length > 0) result.push({ abilityName: name, effects })
  }
  result.sort((a, b) => b.effects.length - a.effects.length)
  return result
})
</script>
