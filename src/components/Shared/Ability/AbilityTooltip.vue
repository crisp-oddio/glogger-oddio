<template>
  <div class="flex gap-2 items-start mb-2">
    <img
      v-if="iconSrc"
      :src="iconSrc"
      :alt="ability.name"
      class="w-8 h-8 object-contain bg-black/30 border border-border-light rounded shrink-0" />
    <div class="flex-1">
      <div class="font-bold text-entity-ability text-sm mb-0.5">{{ ability.name }}</div>
      <div class="flex gap-2 text-xs">
        <span v-if="ability.skill" class="text-entity-skill">{{ ability.skill }}</span>
        <span v-if="ability.level" class="text-text-muted">Level {{ ability.level }} Ability</span>
      </div>
    </div>
  </div>

  <div v-if="ability.description" class="text-text-secondary text-xs leading-relaxed mb-2 italic">
    {{ ability.description }}
  </div>

  <!-- Damage type / AoE badges -->
  <div v-if="displayDamageType || aoe != null" class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs mb-2">
    <span v-if="displayDamageType" class="text-red-400">
      {{ displayDamageType }}<span v-if="isConverted" class="text-text-dim font-normal"> (was {{ ability.damage_type }})</span>
    </span>
    <span v-if="aoe != null" class="text-text-muted">AoE: {{ aoe }}m</span>
  </div>

  <!-- Cost / Cooldown / Range / Target (mirrors the in-game ability card) -->
  <div v-if="statLines.length" class="space-y-0.5 text-xs mb-2">
    <div v-for="line in statLines" :key="line.label" class="flex items-baseline gap-2">
      <span class="text-text-muted w-20 shrink-0">{{ line.label }}:</span>
      <span class="text-text-primary font-medium">{{ line.value }}</span>
    </div>
  </div>

  <!-- Special effect prose (SpecialInfo) -->
  <div v-if="ability.special_info" class="flex items-baseline gap-2 text-xs mb-2">
    <span class="text-accent-gold shrink-0">Special:</span>
    <span class="text-text-secondary leading-relaxed">{{ ability.special_info }}</span>
  </div>

  <!-- Sidebar placeable note -->
  <div v-if="canBeOnSidebar" class="text-[10px] text-text-dim italic leading-snug mb-2">
    Sidebar Placeable: can be placed on your sidebar instead of your primary bar<span v-if="ability.skill"> (still requires {{ ability.skill }} active)</span>.
  </div>

  <div v-if="ability.keywords?.length" class="flex flex-wrap gap-1">
    <span
      v-for="keyword in ability.keywords"
      :key="keyword"
      class="bg-entity-ability/10 text-entity-ability px-1.5 py-0.5 rounded-sm text-[10px] uppercase tracking-wide"
    >
      {{ keyword }}
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { AbilityInfo } from "../../../types/gameData";

const props = defineProps<{
  ability: AbilityInfo;
  iconSrc: string | null;
  /** Build-effective damage type (e.g. a Viper Halberd converting Crushing → Slashing).
   *  When set and different from the ability's own type, it replaces the type badge. */
  damageTypeOverride?: string | null;
}>();

/** Converted type wins over the ability's inherent one. */
const displayDamageType = computed(() => props.damageTypeOverride ?? props.ability.damage_type);

const isConverted = computed(
  () =>
    !!props.damageTypeOverride &&
    !!props.ability.damage_type &&
    props.damageTypeOverride !== props.ability.damage_type,
);

/** Costs, range, AoE etc. live under PvE in the CDN, not at the ability root, so
 *  prefer PvE (fall back to PvP, then the root fields for the rare ability that
 *  keeps them there). */
const stats = computed(() => props.ability.pve ?? props.ability.pvp ?? null);

function asNumber(v: unknown): number | null {
  return typeof v === "number" ? v : null;
}

const extra = computed(() => (stats.value?.extra ?? {}) as Record<string, unknown>);

const aoe = computed(() => asNumber(extra.value.AoE));

const powerCost = computed(() => stats.value?.power_cost ?? props.ability.power_cost ?? null);
const manaCost = computed(() => props.ability.mana_cost ?? asNumber(extra.value.ManaCost));
const healthCost = computed(() => props.ability.health_cost ?? asNumber(extra.value.HealthCost));
const armorCost = computed(() => props.ability.armor_cost ?? asNumber(extra.value.ArmorCost));

/** "24 Power", "116 Power + 30 Health", … — only non-zero costs. */
const costText = computed(() => {
  const parts: string[] = [];
  if (powerCost.value) parts.push(`${powerCost.value} Power`);
  if (manaCost.value) parts.push(`${manaCost.value} Mana`);
  if (healthCost.value) parts.push(`${healthCost.value} Health`);
  if (armorCost.value) parts.push(`${armorCost.value} Armor`);
  return parts.join(" + ");
});

const rangeValue = computed(() => stats.value?.range ?? props.ability.range ?? null);

/** Self-target abilities read "Self" in game; everything else shows metres. */
const rangeText = computed(() => {
  if (props.ability.target === "Self") return "Self";
  return rangeValue.value != null ? `${rangeValue.value}m` : null;
});

const canBeOnSidebar = computed(() => props.ability.raw_json?.CanBeOnSidebar === true);

const statLines = computed(() => {
  const lines: { label: string; value: string }[] = [];
  if (costText.value) lines.push({ label: "Cost", value: costText.value });
  if (props.ability.reset_time)
    lines.push({ label: "Cooldown", value: `${props.ability.reset_time} seconds` });
  if (rangeText.value) lines.push({ label: "Range", value: rangeText.value });
  // Target is already conveyed by "Range: Self"; only add it for the non-self cases
  // where it clarifies who the ability affects.
  if (props.ability.target && props.ability.target !== "Self")
    lines.push({ label: "Target", value: props.ability.target });
  return lines;
});
</script>
