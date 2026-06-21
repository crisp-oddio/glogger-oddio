<template>
  <div class="flex flex-col gap-3 text-sm h-full">
    <!-- Header: session total -->
    <div class="flex items-baseline justify-between gap-2">
      <span class="text-xs text-text-dim uppercase tracking-wide">Wisdom this session</span>
      <span class="text-base font-semibold text-accent-gold tabular-nums">
        {{ session.total.toLocaleString() }}
      </span>
    </div>

    <div class="h-px bg-border-default" />

    <!-- Earned this session -->
    <div class="flex flex-col gap-1.5 min-h-0">
      <span class="text-xs text-text-dim uppercase tracking-wide">Earned this session</span>
      <div v-if="session.earns.length === 0" class="text-xs text-text-dim italic">
        No Combat Wisdom earned yet this session.
      </div>
      <div v-else class="flex flex-col gap-1 max-h-40 overflow-y-auto pr-1">
        <div
          v-for="(e, i) in session.earns"
          :key="i"
          class="flex items-baseline justify-between gap-2">
          <span class="text-text-primary truncate">
            {{ e.name ?? 'Prodigy Level' }}
            <span v-if="e.zone" class="text-text-dim text-[11px]">({{ e.zone }})</span>
          </span>
          <span class="text-accent-gold tabular-nums whitespace-nowrap">+{{ e.amount }}</span>
        </div>
      </div>
    </div>

    <div class="h-px bg-border-default" />

    <!-- Per-monster cooldowns -->
    <div class="flex flex-col gap-1.5 min-h-0 flex-1">
      <span class="text-xs text-text-dim uppercase tracking-wide">Monster cooldowns</span>
      <div v-if="cooldowns.length === 0" class="text-xs text-text-dim italic">
        No monster history yet.
      </div>
      <div v-else class="flex flex-col gap-1 overflow-y-auto pr-1">
        <div
          v-for="m in cooldowns"
          :key="m.name"
          class="flex items-baseline justify-between gap-2">
          <span class="text-text-primary truncate" :title="cooldownTitle(m)">{{ m.name }}</span>
          <span
            class="tabular-nums whitespace-nowrap text-xs"
            :class="m.ready ? 'text-emerald-400' : 'text-text-dim'">
            {{ m.ready ? 'Ready' : m.label }}
          </span>
        </div>
      </div>
    </div>

    <p class="text-[10px] text-text-dim/70 leading-tight mt-auto">
      Cooldowns are learned from your own kills (shortest gap seen), with a wiki default until then.
    </p>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import {
  useGameStateStore,
  type CombatWisdomMonster,
} from '../../../stores/gameStateStore'

const store = useGameStateStore()

// Live clock so countdowns tick.
const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | undefined

onMounted(() => {
  store.fetchCombatWisdomMonsters()
  timer = setInterval(() => {
    now.value = Date.now()
  }, 1000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const session = computed(() => store.combatWisdomSession)

/** Named/boss reuse timer cap (wiki: named soloable = once per 24h). */
const WIKI_MAX_COOLDOWN_SECS = 24 * 3600

/** Cooldown in seconds for a monster: observed minimum, else wiki default. */
function cooldownSecs(m: CombatWisdomMonster): number {
  // "Defeated" = elite: chance-based each kill, no real cooldown.
  if (m.verb === 'Defeated') return 0
  // Named/boss: the observed shortest gap is an *upper bound* on the true
  // cooldown, so cap it at the wiki max (24h). This lets a real boss timer
  // shorten below 24h (e.g. ~3h) while preventing a rarely-killed mob — whose
  // only observed gap is weeks — from showing an absurd multi-week countdown.
  if (m.min_gap_secs != null) return Math.min(m.min_gap_secs, WIKI_MAX_COOLDOWN_SECS)
  return WIKI_MAX_COOLDOWN_SECS
}

function formatDuration(secs: number): string {
  if (secs <= 0) return 'Ready'
  const h = Math.floor(secs / 3600)
  const min = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${h}h ${min}m`
  if (min > 0) return `${min}m ${s}s`
  return `${s}s`
}

const cooldowns = computed(() => {
  return store.combatWisdomMonsters
    .map((m) => {
      const readyAt = m.last_earned_ms + cooldownSecs(m) * 1000
      const remainMs = readyAt - now.value
      const ready = remainMs <= 0
      return {
        name: m.name,
        verb: m.verb,
        min_gap_secs: m.min_gap_secs,
        ready,
        remainMs,
        label: formatDuration(Math.ceil(remainMs / 1000)),
      }
    })
    .sort((a, b) => {
      // Not-ready first (soonest-ready first), then ready ones.
      if (a.ready !== b.ready) return a.ready ? 1 : -1
      return a.remainMs - b.remainMs
    })
})

function cooldownTitle(m: { verb: string; min_gap_secs: number | null }): string {
  if (m.min_gap_secs != null) {
    return `Observed cooldown: ${formatDuration(m.min_gap_secs)} (learned from your kills)`
  }
  return m.verb === 'Defeated'
    ? 'Elite — no cooldown (wiki default)'
    : 'Wiki default: 24h until a shorter gap is observed'
}
</script>
