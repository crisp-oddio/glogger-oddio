import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useGameDataStore } from '../stores/gameDataStore'
import { useGameStateStore } from '../stores/gameStateStore'
import { useSettingsStore } from '../stores/settingsStore'
import { tierIndex } from './useFavorTiers'
import { hasStorage } from './useNpcServices'
import type { NpcInfo } from '../types/gameData/npcs'
import type { SkillInfo } from '../types/gameData/skills'

export interface GiftLogEntry {
  npc_key: string
  npc_name: string
  gifted_at: string
  favor_delta: number
}

export interface StatehelmNpcStatus {
  npc: NpcInfo
  giftsThisWeek: number
  maxGifts: number
  giftLog: GiftLogEntry[]
  favorTier: string | null
}

/** A prioritized gift target: an NPC surfaced because it trains one of the
 *  player's top (or equipped) skills, annotated with the skill that put it on
 *  the list. */
export interface StatehelmGiftTarget extends StatehelmNpcStatus {
  /** Display name of the skill that drove this NPC onto the list */
  drivingSkill: string
  /** True when surfaced because the skill is currently equipped (combat only) */
  equipped: boolean
}

const MAX_GIFTS_PER_WEEK = 5

/** How many of the player's top combat / non-combat skills to surface */
const TOP_COMBAT_SKILLS = 4
const TOP_NONCOMBAT_SKILLS = 2

// Module-level cache of CDN skill metadata — shared across every composable
// instance (StatehelmView + the dashboard widget both call useStatehelmTracker).
const cachedSkills = ref<SkillInfo[]>([])
let skillsLoadPromise: Promise<void> | null = null

async function ensureSkillMeta() {
  if (cachedSkills.value.length > 0) return
  if (!skillsLoadPromise) {
    skillsLoadPromise = invoke<SkillInfo[]>('get_all_skills')
      .then((skills) => { cachedSkills.value = skills })
      .catch((e) => { console.error('Failed to load skill metadata:', e); skillsLoadPromise = null })
  }
  return skillsLoadPromise
}

/** Get the Monday 00:00 UTC boundary for the current week */
function getCurrentWeekStart(): Date {
  const now = new Date()
  const utcDay = now.getUTCDay() // 0=Sun, 1=Mon, ...
  const daysSinceMonday = utcDay === 0 ? 6 : utcDay - 1
  const monday = new Date(Date.UTC(
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate() - daysSinceMonday,
    0, 0, 0, 0
  ))
  return monday
}

export function useStatehelmTracker() {
  const gameData = useGameDataStore()
  const gameState = useGameStateStore()
  const settings = useSettingsStore()

  const giftLog = ref<GiftLogEntry[]>([])
  const loading = ref(false)

  const statehelmNpcs = computed<NpcInfo[]>(() => {
    const allNpcs = Object.values(gameData.npcsByKey)
    return allNpcs.filter(npc => {
      // Only the Statehelm city proper (AreaStatehelm) uses the weekly 5-gift
      // favor cap. Exclude the Statehelm Sewers (AreaStatehelmCaves), whose
      // animal-form trainers (Pig/Rabbit/etc.) have no such cap.
      const area = (npc.area_name ?? '').toLowerCase()
      return area === 'areastatehelm' && npc.preferences.length > 0
    }).sort((a, b) => a.name.localeCompare(b.name))
  })

  const weekStart = computed(() => getCurrentWeekStart())
  const weekStartIso = computed(() => weekStart.value.toISOString())

  const giftsThisWeek = computed(() => {
    const cutoff = weekStartIso.value
    const counts: Record<string, GiftLogEntry[]> = {}
    for (const entry of giftLog.value) {
      if (entry.gifted_at >= cutoff) {
        if (!counts[entry.npc_key]) counts[entry.npc_key] = []
        counts[entry.npc_key].push(entry)
      }
    }
    return counts
  })

  const npcStatuses = computed<StatehelmNpcStatus[]>(() => {
    return statehelmNpcs.value.map(npc => {
      const gifts = giftsThisWeek.value[npc.key] ?? []
      const favorData = gameState.favorByNpc[npc.key]
      return {
        npc,
        giftsThisWeek: gifts.length,
        maxGifts: MAX_GIFTS_PER_WEEK,
        giftLog: gifts,
        favorTier: favorData?.favor_tier ?? null,
      }
    })
  })

  // ── Skill-driven gift prioritization ──────────────────────────────────
  // The dashboard widget surfaces the NPCs that train the player's top combat
  // skills (+ the 2 currently equipped) and top non-combat skills, until each
  // NPC's 5 weekly gifts are donated — at which point it falls off and the next
  // highest skill of that category takes its place.

  /** internal skill name → { id, combat } */
  const skillMetaByInternal = computed(() => {
    const m = new Map<string, { id: number; combat: boolean }>()
    for (const s of cachedSkills.value) {
      m.set(s.internal_name, { id: s.id, combat: s.combat === true })
    }
    return m
  })

  /** skill id → combat flag */
  const skillCombatById = computed(() => {
    const m = new Map<number, boolean>()
    for (const s of cachedSkills.value) m.set(s.id, s.combat === true)
    return m
  })

  /** skill id → internal name */
  const skillInternalById = computed(() => {
    const m = new Map<number, string>()
    for (const s of cachedSkills.value) m.set(s.id, s.internal_name)
    return m
  })

  /** internal skill name → display name */
  const skillDisplayByInternal = computed(() => {
    const m = new Map<string, string>()
    for (const s of cachedSkills.value) m.set(s.internal_name, s.name)
    return m
  })

  /** NPC key → true when the NPC trains at least one combat skill ("combat wins
   *  ties": a mixed NPC is treated as combat-only for categorization). */
  const npcIsCombatByKey = computed(() => {
    const meta = skillMetaByInternal.value
    const m = new Map<string, boolean>()
    for (const st of npcStatuses.value) {
      m.set(st.npc.key, st.npc.trains_skills.some((int) => meta.get(int)?.combat === true))
    }
    return m
  })

  /** internal skill name → statuses of NPCs that train it */
  const statusesBySkillInternal = computed(() => {
    const m = new Map<string, StatehelmNpcStatus[]>()
    for (const st of npcStatuses.value) {
      for (const int of st.npc.trains_skills) {
        if (!m.has(int)) m.set(int, [])
        m.get(int)!.push(st)
      }
    }
    return m
  })

  /** Player's combat skill internal names, highest base level first (level > 0). */
  const rankedCombatSkillInternals = computed(() => rankPlayerSkills(true))
  /** Player's non-combat skill internal names, highest base level first. */
  const rankedNonCombatSkillInternals = computed(() => rankPlayerSkills(false))

  function rankPlayerSkills(wantCombat: boolean): string[] {
    const combatById = skillCombatById.value
    const internalById = skillInternalById.value
    return [...gameState.skills]
      .filter((s) => s.base_level > 0 && (combatById.get(s.skill_id) === true) === wantCombat)
      .sort((a, b) => b.base_level - a.base_level)
      .map((s) => internalById.get(s.skill_id))
      .filter((x): x is string => !!x)
  }

  /** Currently equipped combat skills as internal names. */
  const equippedSkillInternals = computed(() => {
    const a = gameState.activeSkills
    if (!a) return []
    const internalById = skillInternalById.value
    return [a.skill1_id, a.skill2_id]
      .map((id) => internalById.get(id))
      .filter((x): x is string => !!x)
  })

  /** An NPC drops off the widget regardless of remaining gifts when its favor
   *  standing makes further gifting pointless:
   *   - SoulMates (the top tier): always excluded.
   *   - LikeFamily: excluded unless the NPC offers storage (storage capacity keeps
   *     scaling with favor past LikeFamily, so those stay worth gifting). */
  function isExcludedByFavor(status: StatehelmNpcStatus): boolean {
    const tier = status.favorTier
    if (tier === 'SoulMates') return true
    if (tier === 'LikeFamily' && !hasStorage(status.npc)) return true
    return false
  }

  /** Pick the representative NPC for a skill: the one whose category matches and
   *  that has the highest current favor standing, skipping NPCs that are maxed
   *  (5/5 this week) or excluded by favor standing. Returns null when none remain. */
  function representativeFor(internal: string, wantCombat: boolean): StatehelmNpcStatus | null {
    const candidates = (statusesBySkillInternal.value.get(internal) ?? []).filter((st) => {
      if ((npcIsCombatByKey.value.get(st.npc.key) ?? false) !== wantCombat) return false
      if (st.giftsThisWeek >= st.maxGifts) return false
      if (isExcludedByFavor(st)) return false
      return true
    })
    if (candidates.length === 0) return null
    candidates.sort((a, b) => {
      const fa = tierIndex(a.favorTier ?? 'Neutral')
      const fb = tierIndex(b.favorTier ?? 'Neutral')
      if (fa !== fb) return fa - fb // lower index = higher favor
      return a.npc.name.localeCompare(b.npc.name)
    })
    return candidates[0]
  }

  function displaySkill(internal: string): string {
    return skillDisplayByInternal.value.get(internal) ?? internal
  }

  /** Combat gift targets: the NPCs for the 2 equipped combat skills plus the
   *  player's top combat skills, up to `4 + equipped` slots. Equipped skills are
   *  prioritized (and keep their ✦ marker even when also top-ranked). Maxed NPCs
   *  are skipped, so when any NPC — equipped included — has all 5 gifts donated it
   *  falls off and the next-highest combat skill backfills the slot. */
  const combatGiftTargets = computed<StatehelmGiftTarget[]>(() => {
    // Dedupe equipped (both slots could, in theory, hold the same skill).
    const equipped = [...new Set(equippedSkillInternals.value)]
    const equippedSet = new Set(equipped)
    const cap = TOP_COMBAT_SKILLS + equipped.length

    // Priority order of skills: equipped first, then ranked by base level.
    // Dedupe by skill so an equipped skill that's also top-ranked is processed
    // once (keeping its equipped marker).
    const orderedSkills: string[] = []
    const skillSeen = new Set<string>()
    for (const internal of [...equipped, ...rankedCombatSkillInternals.value]) {
      if (skillSeen.has(internal)) continue
      skillSeen.add(internal)
      orderedSkills.push(internal)
    }

    const out: StatehelmGiftTarget[] = []
    const npcSeen = new Set<string>()
    for (const internal of orderedSkills) {
      if (out.length >= cap) break
      const rep = representativeFor(internal, true)
      if (rep && !npcSeen.has(rep.npc.key)) {
        npcSeen.add(rep.npc.key)
        out.push({ ...rep, drivingSkill: displaySkill(internal), equipped: equippedSet.has(internal) })
      }
    }

    return out
  })

  /** Non-combat gift targets: the top 2 non-combat-skill NPCs (maxed skipped). */
  const nonCombatGiftTargets = computed<StatehelmGiftTarget[]>(() => {
    const out: StatehelmGiftTarget[] = []
    const seen = new Set<string>()

    for (const internal of rankedNonCombatSkillInternals.value) {
      if (out.length >= TOP_NONCOMBAT_SKILLS) break
      const rep = representativeFor(internal, false)
      if (rep && !seen.has(rep.npc.key)) {
        seen.add(rep.npc.key)
        out.push({ ...rep, drivingSkill: displaySkill(internal), equipped: false })
      }
    }

    return out
  })

  const totalGiftsGiven = computed(() =>
    npcStatuses.value.reduce((sum, s) => sum + s.giftsThisWeek, 0)
  )

  const totalGiftsMax = computed(() =>
    npcStatuses.value.length * MAX_GIFTS_PER_WEEK
  )

  async function loadGiftLog() {
    const characterName = settings.settings.activeCharacterName
    const serverName = settings.settings.activeServerName
    if (!characterName || !serverName) return

    loading.value = true
    try {
      giftLog.value = await invoke<GiftLogEntry[]>('get_gift_log', {
        characterName,
        serverName,
      })
    } catch (e) {
      console.error('Failed to load gift log:', e)
    } finally {
      loading.value = false
    }
  }

  async function addGift(npcKey: string, npcName: string) {
    const characterName = settings.settings.activeCharacterName
    const serverName = settings.settings.activeServerName
    if (!characterName || !serverName) return

    await invoke('add_manual_gift', {
      characterName,
      serverName,
      npcKey,
      npcName,
    })
    await loadGiftLog()
  }

  async function removeGift(npcKey: string) {
    const characterName = settings.settings.activeCharacterName
    const serverName = settings.settings.activeServerName
    if (!characterName || !serverName) return

    await invoke('remove_last_gift', {
      characterName,
      serverName,
      npcKey,
      weekStart: weekStartIso.value,
    })
    await loadGiftLog()
  }

  // Watch for real-time gift events via the favor activity feed
  watch(
    () => gameState.favorChanges,
    (changes) => {
      if (changes.length === 0) return
      const latest = changes[0]
      if (latest.detail === 'gift') {
        loadGiftLog()
      }
    },
    { deep: true }
  )

  // Also watch the DB-backed favor data — this is always refreshed by
  // game-state-updated events regardless of which page is active, so
  // the gift log stays current even when the statehelm page isn't open.
  watch(
    () => gameState.favor,
    () => loadGiftLog(),
    { deep: true }
  )

  return {
    statehelmNpcs,
    npcStatuses,
    combatGiftTargets,
    nonCombatGiftTargets,
    totalGiftsGiven,
    totalGiftsMax,
    loading,
    loadGiftLog,
    loadSkillMeta: ensureSkillMeta,
    addGift,
    removeGift,
    weekStart,
  }
}
