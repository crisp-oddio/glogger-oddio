<template>
  <div v-if="mods.length > 0" class="mt-2 pt-2 border-t border-border-default/60 space-y-1.5">
    <div class="text-[10px] font-semibold uppercase tracking-wide text-text-muted">
      Applied mods ({{ mods.length }})
    </div>
    <div
      v-for="mod in mods"
      :key="mod.modKey"
      class="space-y-0.5">
      <div class="flex items-center gap-1.5">
        <span class="text-xs font-medium text-text-primary">{{ mod.name }}</span>
        <span v-if="mod.isAugment" class="text-[9px] font-semibold text-mod-augment uppercase">AUG</span>
        <span class="text-[10px] text-text-dim ml-auto shrink-0">{{ mod.slotLabel }}</span>
      </div>
      <div v-if="mod.effects.length > 0" class="pl-2">
        <EffectLine
          v-for="(effect, i) in mod.effects"
          :key="i"
          :label="effect.label"
          :formatted-value="effect.formattedValue"
          :numeric-value="effect.numericValue"
          :icon-id="effect.iconId" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import EffectLine from './EffectLine.vue'
import type { AbilityModEntry } from '../../../composables/useBuildModEffects'

defineProps<{
  mods: AbilityModEntry[]
}>()
</script>
