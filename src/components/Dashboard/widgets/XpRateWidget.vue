<template>
  <div class="flex flex-col gap-3 text-sm h-full">
    <!-- Combat -->
    <div class="flex flex-col gap-2">
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-xs text-text-dim uppercase tracking-wide">Combat XP/hr</span>
        <span class="text-base font-semibold text-text-primary tabular-nums">
          {{ combatRate > 0 ? formatRate(combatRate) : '—' }}
        </span>
      </div>
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-xs text-text-dim uppercase tracking-wide">Combat XP/session</span>
        <span class="text-sm font-medium text-text-primary tabular-nums">
          {{ formatRate(session.combatXp) }}
        </span>
      </div>
    </div>

    <div class="h-px bg-border-default" />

    <!-- Prodigy -->
    <div class="flex flex-col gap-2">
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-xs text-accent-gold/80 uppercase tracking-wide">Prodigy XP/hr</span>
        <span class="text-base font-semibold text-accent-gold tabular-nums">
          {{ prodigyRate > 0 ? formatRate(prodigyRate) : '—' }}
        </span>
      </div>
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-xs text-accent-gold/80 uppercase tracking-wide">Prodigy XP/session</span>
        <span class="text-sm font-medium text-accent-gold tabular-nums">
          {{ formatRate(session.prodigyXp) }}
        </span>
      </div>
    </div>

    <div class="h-px bg-border-default" />

    <!-- Prodigy ETA -->
    <div class="flex items-baseline justify-between gap-2">
      <span class="text-xs text-text-dim">Next prodigy level</span>
      <span class="text-sm font-medium text-text-primary">{{ eta }}</span>
    </div>

    <!-- Reset -->
    <div class="mt-auto flex justify-end">
      <button
        class="text-[11px] text-text-dim hover:text-text-primary transition-colors"
        title="Reset XP-rate session"
        @click="store.resetXpRateSession()">
        Reset
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useGameStateStore } from '../../../stores/gameStateStore'

const store = useGameStateStore()

// Live clock so rates and ETA keep updating between XP gains.
const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | undefined

onMounted(() => {
  timer = setInterval(() => {
    now.value = Date.now()
  }, 1000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const session = computed(() => store.xpRateSession)
const combatRate = computed(() => store.xpRateOf('combat', now.value))
const prodigyRate = computed(() => store.xpRateOf('prodigy', now.value))
const eta = computed(() => store.prodigyEta(prodigyRate.value))

/** Compact XP formatting: 1234 → 1.2K, 2_500_000 → 2.5M. */
function formatRate(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1) + 'M'
  if (n >= 10_000) return Math.round(n / 1000) + 'K'
  if (n >= 1_000) return (n / 1000).toFixed(1) + 'K'
  return n.toLocaleString()
}
</script>
