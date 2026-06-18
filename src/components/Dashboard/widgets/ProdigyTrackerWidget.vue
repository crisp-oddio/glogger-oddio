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

    <!-- Current prodigy XP input + progress -->
    <div class="flex flex-col gap-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs text-text-dim">Current prodigy XP</label>
        <input
          :value="startXpInput"
          type="text"
          inputmode="numeric"
          placeholder="0"
          class="w-28 bg-surface-card border border-border-default rounded px-2 py-1 text-right text-sm text-text-primary tabular-nums focus:outline-none focus:border-accent-gold"
          @input="onStartXpInput" />
      </div>
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-xs text-text-dim">Progress</span>
        <span class="text-sm font-medium text-text-primary tabular-nums">
          {{ currentXp.toLocaleString() }} / {{ PRODIGY_XP_PER_LEVEL.toLocaleString() }}
          <span class="text-text-dim">({{ percent }}%)</span>
        </span>
      </div>
      <!-- Progress bar -->
      <div class="h-1.5 w-full rounded bg-surface-card overflow-hidden">
        <div class="h-full bg-accent-gold transition-all" :style="{ width: percent + '%' }" />
      </div>
    </div>

    <div class="h-px bg-border-default" />

    <!-- ETA from remaining XP -->
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
import { useViewPrefs } from '../../../composables/useViewPrefs'

const store = useGameStateStore()

interface ProdigyTrackerPrefs {
  startXp: number
  [key: string]: unknown
}
const { prefs, update } = useViewPrefs<ProdigyTrackerPrefs>('widget.prodigy-tracker', {
  startXp: 0,
})

const PRODIGY_XP_PER_LEVEL = store.PRODIGY_XP_PER_LEVEL

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

// Local text mirror of the persisted starting XP so the input stays editable.
const startXpInput = ref(prefs.value.startXp ? String(prefs.value.startXp) : '')

function onStartXpInput(e: Event) {
  const raw = (e.target as HTMLInputElement).value
  startXpInput.value = raw
  const parsed = Number(raw.replace(/[, ]/g, ''))
  update({ startXp: Number.isFinite(parsed) && parsed >= 0 ? parsed : 0 })
}

const session = computed(() => store.xpRateSession)
const combatRate = computed(() => store.xpRateOf('combat', now.value))
const prodigyRate = computed(() => store.xpRateOf('prodigy', now.value))

/** Current XP = entered baseline + prodigy XP earned this session, capped at one level. */
const currentXp = computed(() =>
  Math.min(prefs.value.startXp + session.value.prodigyXp, PRODIGY_XP_PER_LEVEL),
)
const remainingXp = computed(() => Math.max(PRODIGY_XP_PER_LEVEL - currentXp.value, 0))
const percent = computed(() =>
  ((currentXp.value / PRODIGY_XP_PER_LEVEL) * 100).toFixed(1),
)

const eta = computed(() => {
  if (remainingXp.value <= 0) return 'Ready!'
  if (prodigyRate.value <= 0) return '—'
  return store.formatEta(remainingXp.value / prodigyRate.value)
})

/** Compact XP formatting: 1234 → 1.2K, 2_500_000 → 2.5M. */
function formatRate(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1) + 'M'
  if (n >= 10_000) return Math.round(n / 1000) + 'K'
  if (n >= 1_000) return (n / 1000).toFixed(1) + 'K'
  return n.toLocaleString()
}
</script>
