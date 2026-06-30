<template>
  <div class="flex flex-col gap-3 text-sm h-full">
    <!-- Header: total spins + last number -->
    <div class="flex items-baseline justify-between gap-2">
      <span class="text-xs text-text-dim uppercase tracking-wide">Spins recorded</span>
      <span class="text-base font-semibold text-accent-gold tabular-nums">
        {{ stats.total.toLocaleString() }}
      </span>
    </div>

    <div v-if="stats.total === 0" class="text-xs text-text-dim italic">
      No roulette spins recorded yet. Stand near a casino wheel — winning numbers
      are read from the chat log's <span class="font-mono">Roulette ball ended on N!</span> lines.
    </div>

    <template v-else>
      <!-- Donut + color legend -->
      <div class="flex items-center gap-4">
        <svg :viewBox="`0 0 ${SIZE} ${SIZE}`" class="shrink-0" :width="SIZE" :height="SIZE">
          <g :transform="`rotate(-90 ${SIZE / 2} ${SIZE / 2})`">
            <circle
              v-for="seg in segments"
              :key="seg.label"
              :cx="SIZE / 2"
              :cy="SIZE / 2"
              :r="RADIUS"
              fill="none"
              :stroke="seg.color"
              :stroke-width="THICKNESS"
              :stroke-dasharray="`${seg.len} ${CIRC - seg.len}`"
              :stroke-dashoffset="seg.offset" />
          </g>
          <!-- Center: last winning number -->
          <text
            :x="SIZE / 2"
            :y="SIZE / 2 - 4"
            text-anchor="middle"
            class="fill-text-dim"
            style="font-size: 9px; text-transform: uppercase; letter-spacing: 0.05em">
            Last
          </text>
          <text
            :x="SIZE / 2"
            :y="SIZE / 2 + 14"
            text-anchor="middle"
            :class="lastColorClass"
            style="font-size: 20px; font-weight: 700">
            {{ stats.last_number ?? '–' }}
          </text>
        </svg>

        <div class="flex flex-col gap-1.5 flex-1 min-w-0">
          <div
            v-for="seg in segments"
            :key="seg.label"
            class="flex items-center justify-between gap-2">
            <span class="flex items-center gap-2 min-w-0">
              <span
                class="inline-block w-2.5 h-2.5 rounded-full shrink-0"
                :style="{ backgroundColor: seg.color }" />
              <span class="text-text-primary truncate">{{ seg.label }}</span>
            </span>
            <span class="text-text-dim tabular-nums whitespace-nowrap">
              {{ seg.count }} ({{ pct(seg.count) }}%)
            </span>
          </div>
        </div>
      </div>

      <div class="h-px bg-border-default" />

      <!-- Per-number board: every number tiled in its wheel color -->
      <div class="flex flex-col gap-1.5 min-h-0 flex-1">
        <span class="text-xs text-text-dim uppercase tracking-wide">By number</span>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(34px,1fr))] gap-1 overflow-y-auto pr-1">
          <div
            v-for="n in board"
            :key="n.number"
            class="flex flex-col items-center justify-center rounded py-1 leading-none"
            :class="n.count === 0 ? 'opacity-30' : ''"
            :style="{ backgroundColor: colorOf(n.number) }"
            :title="`${n.number}: ${n.count} spin(s) (${pct(n.count)}%)`">
            <span class="text-[13px] font-semibold tabular-nums text-white/95">{{ n.number }}</span>
            <span class="text-[10px] tabular-nums text-white/75">{{ n.count }}</span>
          </div>
        </div>
      </div>
    </template>

    <p class="text-[10px] text-text-dim/70 leading-tight mt-auto">
      Outcomes only — bets and payouts are never written to the game logs.
    </p>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useGameStateStore } from '../../../stores/gameStateStore'

const store = useGameStateStore()

onMounted(() => {
  store.fetchRouletteStats()
})

const stats = computed(() => store.rouletteStats)

// ── European single-zero wheel color map ──────────────────────────────────
const RED_NUMBERS = new Set([
  1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36,
])
// Matte/pastel palette (consistent with the app's chart styling).
const COLOR_RED = '#d18585' // dusty rose
const COLOR_BLACK = '#4b5563' // slate/charcoal
const COLOR_GREEN = '#82c07b' // sage

function colorOf(n: number): string {
  if (n === 0) return COLOR_GREEN
  return RED_NUMBERS.has(n) ? COLOR_RED : COLOR_BLACK
}

const lastColorClass = computed(() => {
  const n = stats.value.last_number
  if (n == null) return 'fill-text-dim'
  if (n === 0) return 'fill-[color:#82c07b]'
  return RED_NUMBERS.has(n) ? 'fill-[color:#d18585]' : 'fill-text-primary'
})

function pct(count: number): string {
  if (stats.value.total === 0) return '0'
  return ((count / stats.value.total) * 100).toFixed(1)
}

// ── Donut geometry ────────────────────────────────────────────────────────
const SIZE = 96
const THICKNESS = 16
const RADIUS = (SIZE - THICKNESS) / 2
const CIRC = 2 * Math.PI * RADIUS

interface Bucket {
  label: string
  color: string
  count: number
}

/** Red / Black / Green totals derived from the per-number counts. */
const buckets = computed<Bucket[]>(() => {
  let red = 0
  let black = 0
  let green = 0
  for (const c of stats.value.counts) {
    if (c.number === 0) green += c.count
    else if (RED_NUMBERS.has(c.number)) red += c.count
    else black += c.count
  }
  return [
    { label: 'Red', color: COLOR_RED, count: red },
    { label: 'Black', color: COLOR_BLACK, count: black },
    { label: 'Green (0)', color: COLOR_GREEN, count: green },
  ]
})

/** Donut segments with dash length + cumulative offset. */
const segments = computed(() => {
  const total = stats.value.total || 1
  let cumulative = 0
  return buckets.value.map((b) => {
    const frac = b.count / total
    const len = frac * CIRC
    const offset = -cumulative * CIRC
    cumulative += frac
    return { ...b, len, offset }
  })
})

/** Every wheel number 0..=36 with its observed count (0 if never hit),
 *  ordered 0 then 1..36 so it reads like the table layout. */
const board = computed(() => {
  const byNumber = new Map(stats.value.counts.map((c) => [c.number, c.count]))
  return Array.from({ length: 37 }, (_, number) => ({
    number,
    count: byNumber.get(number) ?? 0,
  }))
})
</script>
