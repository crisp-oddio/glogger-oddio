// TypeScript types for the Farming Calculator feature

// === Active Session State ===

export interface FarmingSkillEntry {
  baseline: number
  baselineTnl: number
  gained: number
  level: number
  tnl: number
  levelsGained: number
}

export interface FarmingFavorEntry {
  delta: number
}

// Per-item loot tally from a single enemy type, this session.
export interface FarmingKillLoot {
  quantity: number  // total amount of the item looted from this enemy
  drops: number     // number of distinct corpse pickups that yielded the item
}

export interface FarmingKillEntry {
  count: number
  loot: Record<string, FarmingKillLoot>  // item_name -> loot tally from this enemy type
}

// Per-item extract tally from skinning/butchering a corpse, this session.
export interface FarmingExtractLoot {
  quantity: number  // total amount extracted from this enemy type
  drops: number     // number of distinct extract events
  skill: string     // "Butchering" | "Skinning"
}

// === Loot drop breakdown (session tab item popover) ===

// All-time per-enemy loot stats returned by the `get_enemy_kill_stats` backend command.
export interface EnemyLootStat {
  item_name: string
  total_quantity: number
  times_dropped: number
  drop_rate: number  // 0..1, times_dropped / total_kills (all-time)
}

export interface EnemyKillStats {
  enemy_name: string
  total_kills: number
  loot: EnemyLootStat[]
}

// One row of the per-item drop breakdown shown in the hover popover.
export interface ItemDropBreakdownRow {
  enemyName: string
  // This session
  sessionQuantity: number
  sessionDrops: number
  sessionKills: number
  // All-time (from DB) — null until stats load
  allTimeKills: number | null
  allTimeDropRate: number | null      // 0..1, times_dropped / total_kills
  lootTableSharePct: number | null    // item's share of this enemy's full loot table, by quantity
}

export interface FarmingSession {
  name: string
  notes: string
  startTime: string          // "HH:MM:SS"
  endTime: string | null
  isPaused: boolean
  pauseStartTime: string | null
  totalPausedSeconds: number

  // XP tracking keyed by skill name
  skillXp: Record<string, FarmingSkillEntry>

  // Item tracking — net quantity change keyed by item_name
  itemDeltas: Record<string, number>

  // Items the user wants to hide from the display
  ignoredItems: Set<string>

  // Favor tracking keyed by npc_name
  favorDeltas: Record<string, FarmingFavorEntry>

  // Kill tracking keyed by enemy_name
  kills: Record<string, FarmingKillEntry>

  // Skinning/butchering extracts (not loot-table drops), keyed by enemy_name
  // (corpse), then item_name. Separate category from corpse loot.
  extracts: Record<string, Record<string, FarmingExtractLoot>>

  // Gold earned from vendor sales
  vendorGold: number
}

// === Activity Log ===

export type FarmingLogKind =
  | 'session-start'
  | 'item-gained'
  | 'item-lost'
  | 'xp-gain'
  | 'level-up'
  | 'favor-change'
  | 'vendor-sale'
  | 'enemy-killed'
  | 'session-end'

export interface FarmingLogEntry {
  kind: FarmingLogKind
  timestamp: string
  label: string
  detail?: string
}

// === Persistence (save to DB) ===

export interface SaveFarmingSessionInput {
  name: string
  notes: string
  start_time: string
  end_time: string | null
  elapsed_seconds: number
  total_paused_seconds: number
  vendor_gold: number
  skills: Array<{ skill_id: number; skill_name: string; xp_gained: number; levels_gained: number }>
  items: Array<{ item_name: string; net_quantity: number }>
  favors: Array<{ npc_key: string; npc_name: string; delta: number }>
  kills: Array<{ enemy_name: string; kill_count: number }>
}

// === Historical (loaded from DB) ===

export interface HistoricalFarmingSession {
  id: number
  name: string
  notes: string
  start_time: string
  end_time: string | null
  elapsed_seconds: number
  total_paused_seconds: number
  vendor_gold: number
  created_at: string
  skills: Array<{ skill_id: number; skill_name: string; xp_gained: number; levels_gained: number }>
  items: Array<{ item_name: string; net_quantity: number }>
  favors: Array<{ npc_key: string; npc_name: string; delta: number }>
  kills: Array<{ enemy_name: string; kill_count: number }>
}
