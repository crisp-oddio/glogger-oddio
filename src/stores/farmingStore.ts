import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type {
  FarmingSession,
  FarmingLogEntry,
  FarmingLogKind,
  SaveFarmingSessionInput,
  EnemyKillStats,
} from "../types/farming";
import type { PlayerEvent, ItemProvenance } from "../types/playerEvents";
import { useGameDataStore } from "./gameDataStore";
import { formatTimeFull, formatDuration } from "../composables/useTimestamp";

export const useFarmingStore = defineStore("farming", () => {
  const sessionActive = ref(false);
  const session = ref<FarmingSession | null>(null);
  const log = ref<FarmingLogEntry[]>([]);

  // All-time per-enemy loot stats, fetched lazily from the DB on hover and
  // cached by enemy name (shared across items that drop from the same enemy).
  const enemyStatsCache = ref<Record<string, EnemyKillStats>>({});

  // Live timer tick — increments every second to drive reactive elapsed display
  const timerTick = ref(0);
  let timerInterval: ReturnType<typeof setInterval> | null = null;

  function startTimer() {
    stopTimer();
    timerInterval = setInterval(() => { timerTick.value++; }, 1000);
  }

  function stopTimer() {
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }
  }

  // ── Event Handlers ──────────────────────────────────────────────────────

  function handlePlayerEvent(event: PlayerEvent) {
    if (!sessionActive.value || !session.value) return;
    if (session.value.isPaused) return;

    const s = session.value;

    switch (event.kind) {
      case "ItemStackChanged": {
        if (event.delta === 0) break;
        const name = event.item_name ?? `item#${event.item_type_id}`;
        s.itemDeltas[name] = (s.itemDeltas[name] ?? 0) + event.delta;

        const kind: FarmingLogKind = event.delta > 0 ? "item-gained" : "item-lost";
        const sign = event.delta > 0 ? "+" : "";
        pushLog(kind, event.timestamp, `${name} ${sign}${event.delta}`);

        // Mining/survey yields are gathered, not corpse loot — track them with
        // their source (node/survey) in the "Gathered" column.
        if (event.delta > 0) recordGathered(s, event.provenance, name, event.delta);
        break;
      }

      case "ItemAdded": {
        // New item entering inventory (creates a fresh stack)
        // This is distinct from ItemStackChanged which tracks existing stack size changes
        if (!event.is_new) break; // Skip session-load items
        const addedName = event.item_name;
        s.itemDeltas[addedName] = (s.itemDeltas[addedName] ?? 0) + 1;
        pushLog("item-gained", event.timestamp, `${addedName} +1`);

        recordGathered(s, event.provenance, addedName, 1);
        break;
      }

      case "LootPickedUp": {
        // Ground truth: this item was picked up from a corpse loot window
        // (skinning/butchering items do NOT produce LootPickedUp).
        if (!event.item_name) break;
        // Attribute to the searched corpse. Fall back to a generic bucket when
        // the corpse-search context wasn't captured, so the item still appears.
        const enemyName = event.corpse_name ?? "Unknown enemy";
        // Create the enemy entry even if the kill wasn't tracked this session
        // (count stays 0). Corpse loot must never be silently dropped just
        // because we didn't see the kill — it still shows with an empty
        // per-session breakdown, backed by all-time DB stats on hover.
        if (!s.kills[enemyName]) s.kills[enemyName] = { count: 0, loot: {} };

        const tally = s.kills[enemyName].loot[event.item_name] ?? { quantity: 0, drops: 0 };
        tally.quantity += event.quantity;
        tally.drops += 1;
        s.kills[enemyName].loot[event.item_name] = tally;
        break;
      }

      case "CorpseExtract": {
        // Skinning/butchering yield — tracked as its own category, NOT loot.
        const enemyName = event.corpse_name ?? "Unknown enemy";
        if (!s.extracts) s.extracts = {};
        if (!s.extracts[enemyName]) s.extracts[enemyName] = {};
        const tally = s.extracts[enemyName][event.item_name] ?? {
          quantity: 0,
          drops: 0,
          skill: event.skill,
        };
        tally.quantity += event.quantity;
        tally.drops += 1;
        tally.skill = event.skill;
        s.extracts[enemyName][event.item_name] = tally;
        break;
      }

      case "ItemDeleted": {
        // Only count consumed/unknown as farming losses
        // StorageTransfer and VendorSale are intentional moves
        if (event.context === "Consumed" || event.context === "Unknown") {
          const name = event.item_name ?? "Unknown Item";
          s.itemDeltas[name] = (s.itemDeltas[name] ?? 0) - 1;
          pushLog("item-lost", event.timestamp, `${name} consumed`);
        }
        break;
      }

      case "FavorChanged": {
        const existing = s.favorDeltas[event.npc_name];
        if (existing) {
          existing.delta += event.delta;
        } else {
          s.favorDeltas[event.npc_name] = {
            delta: event.delta,
          };
        }
        const sign = event.delta > 0 ? "+" : "";
        pushLog(
          "favor-change",
          event.timestamp,
          `${event.npc_name} favor ${sign}${event.delta}`
        );
        break;
      }

      case "VendorSold": {
        s.vendorGold += event.price;
        pushLog(
          "vendor-sale",
          event.timestamp,
          `Sold ${event.item_name} for ${event.price}g`
        );
        break;
      }
    }
  }

  function handleSkillUpdate(payload: {
    skill_type: string;
    xp: number;
    level: number;
    tnl: number;
    timestamp: string;
  }) {
    if (!sessionActive.value || !session.value) return;

    const s = session.value;
    const key = payload.skill_type;

    if (!s.skillXp[key]) {
      // First event for this skill during session — set baseline
      s.skillXp[key] = {
        baseline: payload.xp,
        baselineTnl: payload.tnl,
        gained: 0,
        level: payload.level,
        tnl: payload.tnl,
        levelsGained: 0,
      };
      return;
    }

    const entry = s.skillXp[key];
    const prevLevel = entry.level;

    entry.level = payload.level;
    entry.tnl = payload.tnl;

    if (payload.level > prevLevel) {
      // Level-up: add remaining XP in old level + current XP in new level
      const xpToFinishOldLevel = entry.baselineTnl - entry.baseline;
      entry.gained += xpToFinishOldLevel + payload.xp;
      entry.levelsGained += payload.level - prevLevel;
      entry.baseline = payload.xp;
      entry.baselineTnl = payload.tnl;

      pushLog("level-up", payload.timestamp, `${key} leveled up to ${payload.level}!`);
    } else if (payload.xp >= entry.baseline) {
      entry.gained += payload.xp - entry.baseline;
      entry.baseline = payload.xp;
      entry.baselineTnl = payload.tnl;
    }

    if (entry.gained > 0) {
      // Update the last xp-gain log entry for this skill or add new one
      pushLog("xp-gain", payload.timestamp, `${key} +${entry.gained.toLocaleString()} XP`);
    }
  }

  function handleEnemyKilled(payload: {
    enemy_name: string;
    enemy_entity_id: string;
    killing_ability: string;
    health_damage: number;
    armor_damage: number;
    timestamp: string;
  }) {
    if (!sessionActive.value || !session.value) return;
    if (session.value.isPaused) return;

    const s = session.value;
    const name = payload.enemy_name;

    if (!s.kills[name]) {
      s.kills[name] = { count: 0, loot: {} };
    }
    s.kills[name].count++;

    pushLog("enemy-killed", payload.timestamp, `Killed ${name}`);
  }

  // ── Session Controls ────────────────────────────────────────────────────

  function startSession(name?: string) {
    if (sessionActive.value) return;

    const ts = getCurrentTimestamp();
    sessionActive.value = true;
    session.value = {
      name: name ?? "Farming Session",
      notes: "",
      startTime: ts,
      endTime: null,
      isPaused: false,
      pauseStartTime: null,
      totalPausedSeconds: 0,
      skillXp: {},
      itemDeltas: {},
      ignoredItems: new Set(),
      favorDeltas: {},
      kills: {},
      extracts: {},
      vendorGold: 0,
    };
    log.value = [];

    pushLog("session-start", ts, "Farming session started");
    startTimer();
  }

  async function endSession() {
    if (!session.value) return;

    const ts = getCurrentTimestamp();
    session.value.endTime = ts;
    pushLog("session-end", ts, "Farming session ended");
    stopTimer();

    // Persist to database
    try {
      const s = session.value;
      const input: SaveFarmingSessionInput = {
        name: s.name,
        notes: s.notes,
        start_time: s.startTime,
        end_time: s.endTime,
        elapsed_seconds: getActiveSeconds(),
        total_paused_seconds: s.totalPausedSeconds,
        vendor_gold: s.vendorGold,
        skills: await Promise.all(
          Object.entries(s.skillXp)
            .filter(([, v]) => v.gained > 0 || v.levelsGained > 0)
            .map(async ([skillType, v]) => {
              const gameData = useGameDataStore();
              const resolved = await gameData.resolveSkill(skillType);
              return {
                skill_id: resolved?.id ?? 0,
                skill_name: resolved?.name ?? skillType,
                xp_gained: v.gained,
                levels_gained: v.levelsGained,
              };
            })
        ),
        items: Object.entries(s.itemDeltas)
          .filter(([name, qty]) => qty !== 0 && !s.ignoredItems.has(name))
          .map(([item_name, net_quantity]) => ({ item_name, net_quantity })),
        favors: await Promise.all(
          Object.entries(s.favorDeltas)
            .filter(([, v]) => v.delta !== 0)
            .map(async ([npcName, v]) => {
              const gameData = useGameDataStore();
              const resolved = await gameData.resolveNpc(npcName);
              return {
                npc_key: resolved?.key ?? npcName,
                npc_name: resolved?.name ?? npcName,
                delta: v.delta,
              };
            })
        ),
        kills: Object.entries(s.kills)
          .filter(([, v]) => v.count > 0)
          .map(([enemy_name, v]) => ({ enemy_name, kill_count: v.count })),
      };

      await invoke("save_farming_session", { input });
      console.log("[farming] Session saved to database");
    } catch (e) {
      console.error("[farming] Failed to save session:", e);
    }
  }

  function togglePause() {
    if (!session.value) return;

    if (session.value.isPaused) {
      if (session.value.pauseStartTime) {
        const pauseStart = tsToSeconds(session.value.pauseStartTime);
        const now = tsToSeconds(getCurrentTimestamp());
        const pauseDiff = now - pauseStart;
        session.value.totalPausedSeconds += pauseDiff >= 0 ? pauseDiff : pauseDiff + 86400;
        session.value.pauseStartTime = null;
      }
      session.value.isPaused = false;
      startTimer();
    } else {
      session.value.isPaused = true;
      session.value.pauseStartTime = getCurrentTimestamp();
      stopTimer();
    }
  }

  function updateName(name: string) {
    if (session.value) session.value.name = name;
  }

  function updateNotes(notes: string) {
    if (session.value) session.value.notes = notes;
  }

  function reset() {
    sessionActive.value = false;
    session.value = null;
    log.value = [];
    stopTimer();
  }

  // ── Computed ────────────────────────────────────────────────────────────

  function getActiveSeconds(): number {
    if (!session.value) return 0;
    const start = tsToSeconds(session.value.startTime);

    let endSeconds: number;
    if (session.value.endTime) {
      endSeconds = tsToSeconds(session.value.endTime);
    } else if (session.value.isPaused && session.value.pauseStartTime) {
      endSeconds = tsToSeconds(session.value.pauseStartTime);
    } else {
      endSeconds = tsToSeconds(getCurrentTimestamp());
    }

    // Handle midnight rollover: if end < start, session crossed midnight
    const rawDiff = endSeconds - start;
    const totalSeconds = rawDiff >= 0 ? rawDiff : rawDiff + 86400;
    return Math.max(0, totalSeconds - session.value.totalPausedSeconds);
  }

  const elapsed = computed(() => {
    // Depend on timerTick so this recomputes every second
    void timerTick.value;
    if (!session.value) return "—";
    return formatDuration(getActiveSeconds(), { alwaysShowSeconds: true });
  });

  const totalXpGained = computed(() => {
    if (!session.value) return 0;
    return Object.values(session.value.skillXp).reduce((sum, s) => sum + s.gained, 0);
  });

  const totalItemsGained = computed(() => {
    if (!session.value) return 0;
    const ignored = session.value.ignoredItems;
    return Object.entries(session.value.itemDeltas)
      .filter(([name]) => !ignored.has(name))
      .reduce((sum, [, qty]) => sum + Math.max(0, qty), 0);
  });

  const totalItemsLost = computed(() => {
    if (!session.value) return 0;
    const ignored = session.value.ignoredItems;
    return Object.entries(session.value.itemDeltas)
      .filter(([name]) => !ignored.has(name))
      .reduce((sum, [, qty]) => sum + Math.abs(Math.min(0, qty)), 0);
  });

  const totalKills = computed(() => {
    if (!session.value) return 0;
    return Object.values(session.value.kills).reduce((sum, k) => sum + k.count, 0);
  });

  const killSummary = computed(() => {
    void timerTick.value;
    if (!session.value) return [];
    const activeHours = Math.max(1, getActiveSeconds()) / 3600;
    return Object.entries(session.value.kills)
      .filter(([, v]) => v.count > 0)
      .map(([name, v]) => ({
        name,
        count: v.count,
        perHour: Math.round(v.count / activeHours),
        loot: Object.entries(v.loot)
          .filter(([, l]) => l.quantity > 0)
          .map(([itemName, l]) => ({ name: itemName, quantity: l.quantity }))
          .sort((a, b) => b.quantity - a.quantity),
      }))
      .sort((a, b) => b.count - a.count);
  });

  const totalFavorGained = computed(() => {
    if (!session.value) return 0;
    return Object.values(session.value.favorDeltas).reduce((sum, v) => sum + v.delta, 0);
  });

  const skillSummary = computed(() => {
    // Depend on timerTick for per-hour rate updates
    void timerTick.value;
    if (!session.value) return [];
    const activeHours = Math.max(1, getActiveSeconds()) / 3600;
    return Object.entries(session.value.skillXp)
      .filter(([, v]) => v.gained > 0 || v.levelsGained > 0)
      .map(([name, v]) => ({
        name,
        gained: v.gained,
        levelsGained: v.levelsGained,
        level: v.level,
        tnl: v.tnl,
        currentXp: v.baseline,
        xpProgress: v.tnl > 0 ? (v.baseline / v.tnl) * 100 : 0,
        perHour: Math.round(v.gained / activeHours),
      }))
      .sort((a, b) => b.gained - a.gained);
  });

  const itemSummary = computed(() => {
    void timerTick.value;
    if (!session.value) return [];
    const activeHours = Math.max(1, getActiveSeconds()) / 3600;
    const ignored = session.value.ignoredItems;
    return Object.entries(session.value.itemDeltas)
      .filter(([, qty]) => qty !== 0)
      .map(([name, qty]) => ({
        name,
        netQuantity: qty,
        perHour: Math.round(Math.abs(qty) / activeHours),
        isIgnored: ignored.has(name),
      }))
      .sort((a, b) => {
        // Ignored items go to the bottom
        if (a.isIgnored !== b.isIgnored) return a.isIgnored ? 1 : -1;
        return b.netQuantity - a.netQuantity;
      });
  });

  // Items looted from corpses this session, aggregated across all enemy types.
  // Drives the simplified "item — Looted N" list on the session tab.
  const lootedItems = computed(() => {
    void timerTick.value;
    if (!session.value) return [];
    const agg: Record<string, number> = {};
    for (const kill of Object.values(session.value.kills)) {
      for (const [itemName, l] of Object.entries(kill.loot)) {
        agg[itemName] = (agg[itemName] ?? 0) + l.quantity;
      }
    }
    return Object.entries(agg)
      .filter(([, qty]) => qty > 0)
      .map(([name, quantity]) => ({ name, quantity }))
      .sort((a, b) => b.quantity - a.quantity);
  });

  // Session enemies that dropped a given item, with this-session tallies.
  // Used by the hover popover; all-time figures are layered on via fetchEnemyStats.
  function sessionEnemiesForItem(itemName: string) {
    if (!session.value) return [];
    return Object.entries(session.value.kills)
      .map(([enemyName, kill]) => ({ enemyName, kill, loot: kill.loot[itemName] }))
      .filter((e) => e.loot && e.loot.quantity > 0)
      .map((e) => ({
        enemyName: e.enemyName,
        sessionQuantity: e.loot!.quantity,
        sessionDrops: e.loot!.drops,
        sessionKills: e.kill.count,
      }))
      .sort((a, b) => b.sessionQuantity - a.sessionQuantity);
  }

  // Items extracted via skinning/butchering this session, aggregated across
  // enemy types. Separate category from corpse loot.
  const extractedItems = computed(() => {
    void timerTick.value;
    if (!session.value) return [];
    const agg: Record<string, { quantity: number; skill: string }> = {};
    for (const byItem of Object.values(session.value.extracts ?? {})) {
      for (const [itemName, l] of Object.entries(byItem)) {
        const cur = agg[itemName] ?? { quantity: 0, skill: l.skill };
        cur.quantity += l.quantity;
        cur.skill = l.skill;
        agg[itemName] = cur;
      }
    }
    return Object.entries(agg)
      .filter(([, v]) => v.quantity > 0)
      .map(([name, v]) => ({ name, quantity: v.quantity, skill: v.skill }))
      .sort((a, b) => b.quantity - a.quantity);
  });

  // Session enemies a given item was extracted from, with this-session tallies.
  function sessionEnemiesForExtract(itemName: string) {
    if (!session.value) return [];
    const kills = session.value.kills;
    return Object.entries(session.value.extracts ?? {})
      .map(([enemyName, byItem]) => ({ enemyName, loot: byItem[itemName], kills }))
      .filter((e) => e.loot && e.loot.quantity > 0)
      .map((e) => ({
        enemyName: e.enemyName,
        sessionQuantity: e.loot!.quantity,
        sessionDrops: e.loot!.drops,
        sessionKills: e.kills[e.enemyName]?.count ?? 0,
      }))
      .sort((a, b) => b.sessionQuantity - a.sessionQuantity);
  }

  // Lazily fetch (and cache) all-time loot stats for an enemy from the DB.
  async function fetchEnemyStats(enemyName: string): Promise<EnemyKillStats | null> {
    const cached = enemyStatsCache.value[enemyName];
    if (cached) return cached;
    try {
      const stats = await invoke<EnemyKillStats>("get_enemy_kill_stats", { enemyName });
      enemyStatsCache.value[enemyName] = stats;
      return stats;
    } catch (e) {
      console.error("[farming] Failed to fetch enemy stats:", e);
      return null;
    }
  }

  function toggleIgnoreItem(name: string) {
    if (!session.value) return;
    if (session.value.ignoredItems.has(name)) {
      session.value.ignoredItems.delete(name);
    } else {
      session.value.ignoredItems.add(name);
    }
    // Trigger reactivity by reassigning the Set
    session.value.ignoredItems = new Set(session.value.ignoredItems);
  }

  const favorSummary = computed(() => {
    if (!session.value) return [];
    return Object.entries(session.value.favorDeltas)
      .filter(([, v]) => v.delta !== 0)
      .map(([name, v]) => ({
        name,
        delta: v.delta,
      }))
      .sort((a, b) => b.delta - a.delta);
  });

  function xpPerHour(skillName: string): number {
    if (!session.value?.skillXp[skillName]) return 0;
    const activeHours = Math.max(1, getActiveSeconds()) / 3600;
    return Math.round(session.value.skillXp[skillName].gained / activeHours);
  }

  // ── Helpers ─────────────────────────────────────────────────────────────

  function pushLog(kind: FarmingLogKind, timestamp: string, label: string, detail?: string) {
    log.value.unshift({ kind, timestamp, label, detail });
    // Cap log size
    if (log.value.length > 500) log.value.length = 500;
  }

  return {
    sessionActive,
    session,
    log,
    elapsed,
    totalXpGained,
    totalItemsGained,
    totalItemsLost,
    totalKills,
    totalFavorGained,
    skillSummary,
    itemSummary,
    killSummary,
    favorSummary,
    lootedItems,
    extractedItems,
    sessionEnemiesForItem,
    sessionEnemiesForExtract,
    fetchEnemyStats,
    xpPerHour,
    getActiveSeconds,
    handlePlayerEvent,
    handleSkillUpdate,
    handleEnemyKilled,
    startSession,
    endSession,
    togglePause,
    toggleIgnoreItem,
    updateName,
    updateNotes,
    reset,
  };
});

// ── Module helpers ────────────────────────────────────────────────────────

// Record a mining/survey gathered item into the session's "Gathered" category,
// keyed by its source (node name or survey map). Non-gathered provenances
// (corpse loot, vendor, storage, craft, unknown) are ignored here — corpse
// loot/extracts have their own paths.
function recordGathered(
  s: FarmingSession,
  provenance: ItemProvenance | undefined,
  itemName: string,
  quantity: number
): void {
  if (quantity <= 0 || !provenance || provenance.kind !== "Attributed") return;

  const src = provenance.source;
  let sourceName: string;
  let skill: string;
  if (src.kind === "Mining") {
    sourceName = src.node_name ?? "Mining (unknown node)";
    skill = "Mining";
  } else if (src.kind === "SurveyMapUse") {
    sourceName = src.survey_map_internal_name ?? "Survey";
    skill = "Survey";
  } else {
    return;
  }

  if (!s.extracts) s.extracts = {};
  if (!s.extracts[sourceName]) s.extracts[sourceName] = {};
  const tally = s.extracts[sourceName][itemName] ?? { quantity: 0, drops: 0, skill };
  tally.quantity += quantity;
  tally.drops += 1;
  tally.skill = skill;
  s.extracts[sourceName][itemName] = tally;
}

function tsToSeconds(ts: string): number {
  const [h, m, s] = ts.split(":").map(Number);
  return h * 3600 + m * 60 + s;
}

function getCurrentTimestamp(): string {
  return formatTimeFull(new Date().toISOString());
}
