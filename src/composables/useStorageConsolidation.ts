import { ref, computed, watch, type ComputedRef } from "vue";
import { useGameStateStore } from "../stores/gameStateStore";
import { useGameDataStore } from "../stores/gameDataStore";

// ── Types ───────────────────────────────────────────────────────────────────

/** A single item that needs to move from one vault to another */
export interface PlannedMove {
  itemName: string;
  quantity: number;
  fromVaultKey: string;
  fromVaultName: string;
  fromAreaKey: string | null;
  toVaultKey: string;
  toVaultName: string;
  toAreaKey: string | null;
  /** Why this move was suggested */
  reason: "duplicate" | "type_specific";
  /** Has the player completed this move (auto-detected or manual check) */
  completed: boolean;
  /** Target vault's current occupied slot count (for capacity hints in the UI) */
  toVaultOccupied: number;
  /** Target vault's slot capacity, or null when unknown (unconstrained) */
  toVaultCapacity: number | null;
}

/**
 * An item that could not be fully consolidated because every candidate target
 * vault is at (or near) its slot capacity. Surfaced so the plan can warn the
 * player instead of silently suggesting an impossible move.
 */
export interface BlockedConsolidation {
  itemName: string;
  /** Total quantity of the item left un-consolidated (still scattered). */
  leftoverQuantity: number;
  /** Number of separate stacks left behind across the leftover vaults. */
  leftoverStacks: number;
  /** Friendly names of the vaults where the leftover stacks remain. */
  leftoverVaultNames: string[];
  /** Friendly name of the vault we consolidated into (best available). */
  targetVaultName: string;
  /** True when nothing could be moved at all (target had no free slots). */
  fullyBlocked: boolean;
}

/** All moves grouped by zone, with pickups and dropoffs separated */
export interface ZoneStop {
  areaKey: string;
  areaName: string;
  /** Items to pick up from vaults here and carry to another zone */
  pickups: PlannedMove[];
  /** Items arriving from another zone to deposit in vaults here */
  dropoffs: PlannedMove[];
  /** Items moving between two vaults in this same zone (no carrying needed) */
  localMoves: PlannedMove[];
  completed: boolean;
}

export interface ConsolidationPlan {
  moves: PlannedMove[];
  zoneStops: ZoneStop[];
  slotsSaved: number;
  itemsToMove: number;
  zonesInvolved: number;
  typeSpecificSuggestions: number;
  /** Items that couldn't be fully consolidated due to full target vaults. */
  blockedItems: BlockedConsolidation[];
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function isRoutableZone(zone: string | null): zone is string {
  if (!zone) return false;
  if (zone === "*") return false;
  if (!zone.startsWith("Area")) return false;
  return true;
}

// ── Composable ──────────────────────────────────────────────────────────────

export function useStorageConsolidation() {
  const gameState = useGameStateStore();
  const gameData = useGameDataStore();

  const wizardActive = ref(false);
  const completedMoves = ref<Set<string>>(new Set());

  // ── Vault helpers ───────────────────────────────────────────────────────

  function vaultName(key: string): string {
    const detail = gameState.storageVaultsByKey[key];
    if (detail?.npc_friendly_name) return detail.npc_friendly_name;
    return key;
  }

  function vaultArea(key: string): string | null {
    return gameState.storageVaultsByKey[key]?.area ?? null;
  }

  // ── Capacity helpers ────────────────────────────────────────────────────

  /** Slot capacity of a vault (unlocked slots, else theoretical max). null = unknown. */
  function vaultCapacity(key: string): number | null {
    const vault = gameState.storageVaultsByKey[key];
    if (!vault) return null;
    return gameState.getVaultUnlockedSlots(vault) ?? gameState.getVaultMaxPossibleSlots(vault);
  }

  /** Currently occupied slots in a vault (one storage entry == one slot). */
  function vaultOccupied(key: string): number {
    return gameState.storageByVault[key]?.length ?? 0;
  }

  // ── Item stack-size cache ─────────────────────────────────────────────
  // Whether an item merges (stackable) or needs one slot per stack (gear)
  // decides how many slots a consolidation actually consumes at the target.
  // Fetched lazily from the CDN; unresolved names are treated as non-stackable.

  const stackSizeByItem = ref<Map<string, number | null>>(new Map());

  watch(
    () => gameState.storage,
    async () => {
      const missing = new Set<string>();
      for (const item of gameState.storage) {
        if (!stackSizeByItem.value.has(item.item_name)) missing.add(item.item_name);
      }
      if (missing.size === 0) return;
      const names = [...missing];
      const next = new Map(stackSizeByItem.value);
      try {
        const infos = await gameData.resolveItemsBatch(names);
        for (const name of names) next.set(name, infos[name]?.max_stack_size ?? null);
      } catch {
        // On failure treat as unknown → non-stackable (conservative for capacity)
        for (const name of names) if (!next.has(name)) next.set(name, null);
      }
      stackSizeByItem.value = next;
    },
    { immediate: true, deep: true },
  );

  /** Max stack size for an item: >1 stackable, 1 non-stackable, Infinity if not yet resolved. */
  function maxStack(itemName: string): number {
    if (!stackSizeByItem.value.has(itemName)) return Infinity; // avoid false "full" during load
    const s = stackSizeByItem.value.get(itemName);
    return s != null && s > 1 ? s : 1;
  }

  /**
   * Slots a target vault must gain to absorb `incomingStacks`/`incomingQty` of an
   * item it already holds (`targetStacks` slots, `targetQty` count). Stackable
   * items merge into existing stacks (often 0 new slots); non-stackable items
   * need one slot per incoming stack.
   */
  function newSlotsNeeded(
    itemName: string,
    targetStacks: number,
    targetQty: number,
    incomingStacks: number,
    incomingQty: number,
  ): number {
    const ms = maxStack(itemName);
    if (ms <= 1) return incomingStacks; // non-stackable: every stack keeps a slot
    const slotsAfter = Math.ceil((targetQty + incomingQty) / ms);
    return Math.max(0, slotsAfter - targetStacks);
  }

  // ── Plan generation ───────────────────────────────────────────────────

  const plan: ComputedRef<ConsolidationPlan> = computed(() => {
    const moves: PlannedMove[] = [];
    const blockedItems: BlockedConsolidation[] = [];
    let slotsSaved = 0;

    // ── Step 1: Find duplicates (capacity-aware) ─────────────────────

    // Group: item_name → vault_key → { qty, stacks } (one storage entry == one slot)
    interface Holding { vk: string; qty: number; stacks: number }
    const itemVaults = new Map<string, Map<string, Holding>>();
    for (const item of gameState.storage) {
      if (!itemVaults.has(item.item_name)) itemVaults.set(item.item_name, new Map());
      const vm = itemVaults.get(item.item_name)!;
      const cur = vm.get(item.vault_key) ?? { vk: item.vault_key, qty: 0, stacks: 0 };
      cur.qty += item.stack_size;
      cur.stacks += 1;
      vm.set(item.vault_key, cur);
    }

    // Running free-slot budget per vault. Sources free slots as items leave them,
    // which can make room for another item's consolidation processed later.
    const freeBudget = new Map<string, number>();
    function budget(key: string): number {
      if (!freeBudget.has(key)) {
        const cap = vaultCapacity(key);
        freeBudget.set(key, cap == null ? Infinity : cap - vaultOccupied(key));
      }
      return freeBudget.get(key)!;
    }

    function pushMove(itemName: string, src: Holding, targetKey: string) {
      const moveKey = `${itemName}|${src.vk}|${targetKey}`;
      moves.push({
        itemName,
        quantity: src.qty,
        fromVaultKey: src.vk,
        fromVaultName: vaultName(src.vk),
        fromAreaKey: vaultArea(src.vk),
        toVaultKey: targetKey,
        toVaultName: vaultName(targetKey),
        toAreaKey: vaultArea(targetKey),
        reason: "duplicate",
        completed: completedMoves.value.has(moveKey),
        toVaultOccupied: vaultOccupied(targetKey),
        toVaultCapacity: vaultCapacity(targetKey),
      });
    }

    // Process item groups in a stable order for deterministic plans.
    const itemNames = [...itemVaults.keys()].sort();
    for (const itemName of itemNames) {
      const vaultMap = itemVaults.get(itemName)!;
      if (vaultMap.size < 2) continue;

      // Candidate holders, most-of-this-item first (fewest moves, most already there).
      const holders = [...vaultMap.values()].sort((a, b) => b.qty - a.qty || b.stacks - a.stacks);
      const totalQty = holders.reduce((s, h) => s + h.qty, 0);
      const totalStacks = holders.reduce((s, h) => s + h.stacks, 0);

      // Prefer a single target that can hold ALL the duplicates outright.
      let target: Holding | null = null;
      for (const cand of holders) {
        const incomingStacks = totalStacks - cand.stacks;
        const incomingQty = totalQty - cand.qty;
        const need = newSlotsNeeded(itemName, cand.stacks, cand.qty, incomingStacks, incomingQty);
        if (need <= budget(cand.vk)) { target = cand; break; }
      }

      if (target) {
        // Full consolidation into `target`.
        const incomingStacks = totalStacks - target.stacks;
        const incomingQty = totalQty - target.qty;
        const need = newSlotsNeeded(itemName, target.stacks, target.qty, incomingStacks, incomingQty);
        freeBudget.set(target.vk, budget(target.vk) - need);
        for (const h of holders) {
          if (h.vk === target.vk) continue;
          freeBudget.set(h.vk, budget(h.vk) + h.stacks); // source empties, frees slots
          pushMove(itemName, h, target.vk);
        }
        slotsSaved += incomingStacks - need;
        continue;
      }

      // No holder can take everything — reroute to the roomiest holder and move
      // as many stacks as fit, flagging the leftover.
      const roomiest = [...holders].sort((a, b) => budget(b.vk) - budget(a.vk) || b.qty - a.qty)[0];
      const ms = maxStack(itemName);
      let runQty = roomiest.qty;
      let runSlots = roomiest.stacks;
      let runBudget = budget(roomiest.vk);
      // Move smallest sources first so we empty as many vaults as possible.
      const sources = holders
        .filter((h) => h.vk !== roomiest.vk)
        .sort((a, b) => a.stacks - b.stacks || a.qty - b.qty);
      const left: Holding[] = [];
      let movedStacks = 0;
      let addedSlots = 0;
      for (const src of sources) {
        const cost = ms <= 1
          ? src.stacks
          : Math.max(0, Math.ceil((runQty + src.qty) / ms) - runSlots);
        if (cost <= runBudget) {
          runBudget -= cost;
          runQty += src.qty;
          runSlots = ms <= 1 ? runSlots + src.stacks : Math.ceil(runQty / ms);
          addedSlots += cost;
          movedStacks += src.stacks;
          freeBudget.set(src.vk, budget(src.vk) + src.stacks);
          pushMove(itemName, src, roomiest.vk);
        } else {
          left.push(src);
        }
      }
      freeBudget.set(roomiest.vk, runBudget);
      slotsSaved += movedStacks - addedSlots;

      blockedItems.push({
        itemName,
        leftoverQuantity: left.reduce((s, h) => s + h.qty, 0),
        leftoverStacks: left.reduce((s, h) => s + h.stacks, 0),
        leftoverVaultNames: left.map((h) => vaultName(h.vk)),
        targetVaultName: vaultName(roomiest.vk),
        fullyBlocked: movedStacks === 0,
      });
    }

    // ── Step 2: Type-specific vault opportunities ────────────────────
    // (Future: check items in generic vaults that could go to type-specific ones)

    // ── Build zone stops ─────────────────────────────────────────────
    // Classify each move:
    //   - localMoves: source and target in same zone (no carrying)
    //   - cross-zone: pickup at source zone, dropoff at target zone
    //
    // Key insight: dropoffs should only appear at a zone AFTER the
    // corresponding pickup zone has been visited. We order zones so
    // pickup-only zones come first, then zones that are both pickup
    // and dropoff, then dropoff-only zones.

    const perZonePickups = new Map<string, PlannedMove[]>();
    const perZoneDropoffs = new Map<string, PlannedMove[]>();
    const perZoneLocal = new Map<string, PlannedMove[]>();

    for (const move of moves) {
      const from = move.fromAreaKey;
      const to = move.toAreaKey;
      const sameZone = from && to && from === to;

      if (sameZone && isRoutableZone(from)) {
        if (!perZoneLocal.has(from)) perZoneLocal.set(from, []);
        perZoneLocal.get(from)!.push(move);
      } else {
        if (isRoutableZone(from)) {
          if (!perZonePickups.has(from)) perZonePickups.set(from, []);
          perZonePickups.get(from)!.push(move);
        }
        if (isRoutableZone(to)) {
          if (!perZoneDropoffs.has(to)) perZoneDropoffs.set(to, []);
          perZoneDropoffs.get(to)!.push(move);
        }
      }
    }

    // Collect all zones and classify them for ordering
    const allZones = new Set([
      ...perZonePickups.keys(),
      ...perZoneDropoffs.keys(),
      ...perZoneLocal.keys(),
    ]);

    // Helper: resolve friendly area name
    function zoneFriendlyName(zone: string): string {
      for (const v of gameState.storageVaults) {
        if (v.area === zone && v.area_name) return v.area_name;
      }
      return zone;
    }

    const zoneStops: ZoneStop[] = [];
    for (const zone of allZones) {
      const pickups = perZonePickups.get(zone) ?? [];
      const dropoffs = perZoneDropoffs.get(zone) ?? [];
      const localMoves = perZoneLocal.get(zone) ?? [];
      const allMovesHere = [...pickups, ...dropoffs, ...localMoves];

      zoneStops.push({
        areaKey: zone,
        areaName: zoneFriendlyName(zone),
        pickups,
        dropoffs,
        localMoves,
        completed: allMovesHere.length > 0 && allMovesHere.every((m) => m.completed),
      });
    }

    // Order zones so the route makes sense:
    // 1. Zones with pickups but no dropoffs (pure sources — visit first)
    // 2. Zones with both pickups and dropoffs (swap stops)
    // 3. Zones with dropoffs but no pickups (pure destinations — visit last)
    // 4. Zones with only local moves (can be done anytime)
    // Within each group, sort by action count descending.
    function zoneOrder(zs: ZoneStop): number {
      const hasPickup = zs.pickups.length > 0;
      const hasDropoff = zs.dropoffs.length > 0;
      const hasLocal = zs.localMoves.length > 0;
      if (hasPickup && !hasDropoff) return 0;  // pure source
      if (hasPickup && hasDropoff) return 1;    // swap
      if (!hasPickup && hasDropoff) return 2;   // pure destination
      if (hasLocal) return 3;                   // local only
      return 4;
    }

    zoneStops.sort((a, b) => {
      const orderDiff = zoneOrder(a) - zoneOrder(b);
      if (orderDiff !== 0) return orderDiff;
      const aCount = a.pickups.length + a.dropoffs.length + a.localMoves.length;
      const bCount = b.pickups.length + b.dropoffs.length + b.localMoves.length;
      return bCount - aCount;
    });

    // ── Stats ────────────────────────────────────────────────────────
    // slotsSaved is accumulated during plan generation as the net slots freed
    // (source stacks removed minus new slots consumed at the target). Merging
    // stackable duplicates saves slots; co-locating non-stackable gear does not.

    return {
      moves,
      zoneStops,
      slotsSaved: Math.max(0, slotsSaved),
      itemsToMove: moves.filter((m) => !m.completed).length,
      zonesInvolved: zoneStops.length,
      typeSpecificSuggestions: moves.filter((m) => m.reason === "type_specific").length,
      blockedItems,
    };
  });

  // ── Action completion ───────────────────────────────────────────────
  // Each cross-zone move has two actions: pickup and dropoff.
  // Local moves have one action (the rearrangement).
  // We track completion of each action separately.

  /** Completed actions, keyed as "pickup|item|vault" or "dropoff|item|vault" or "local|item|from|to" */
  const completedActions = ref<Set<string>>(new Set());

  function pickupKey(move: PlannedMove): string {
    return `pickup|${move.itemName}|${move.fromVaultKey}`;
  }

  function dropoffKey(move: PlannedMove): string {
    return `dropoff|${move.itemName}|${move.toVaultKey}`;
  }

  function localKey(move: PlannedMove): string {
    return `local|${move.itemName}|${move.fromVaultKey}|${move.toVaultKey}`;
  }

  function isPickupDone(move: PlannedMove): boolean {
    return completedActions.value.has(pickupKey(move));
  }

  function isDropoffDone(move: PlannedMove): boolean {
    return completedActions.value.has(dropoffKey(move));
  }

  function isLocalDone(move: PlannedMove): boolean {
    return completedActions.value.has(localKey(move));
  }

  function togglePickup(move: PlannedMove) {
    const key = pickupKey(move);
    const next = new Set(completedActions.value);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    completedActions.value = next;
  }

  function toggleDropoff(move: PlannedMove) {
    const key = dropoffKey(move);
    const next = new Set(completedActions.value);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    completedActions.value = next;
  }

  function toggleLocal(move: PlannedMove) {
    const key = localKey(move);
    const next = new Set(completedActions.value);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    completedActions.value = next;
  }

  // Legacy compat for plan.moves[].completed
  function isMoveCompleted(move: PlannedMove): boolean {
    if (move.fromAreaKey === move.toAreaKey) return isLocalDone(move);
    return isPickupDone(move) && isDropoffDone(move);
  }

  function resetCompletion() {
    completedActions.value = new Set();
  }

  // ── Wizard mode ─────────────────────────────────────────────────────

  function startWizard() {
    wizardActive.value = true;
    resetCompletion();
  }

  function stopWizard() {
    wizardActive.value = false;
  }

  /** Current zone from game state */
  const currentZone = computed(() => {
    const area = gameState.world?.area as { area_name?: string } | null;
    return area?.area_name ?? null;
  });

  /**
   * Items currently in the player's carry bag: pickup was checked done
   * but the corresponding dropoff has NOT been checked done yet.
   */
  const carryBag = computed<Set<string>>(() => {
    const bag = new Set<string>();
    for (const move of plan.value.moves) {
      if (move.fromAreaKey === move.toAreaKey) continue; // local moves don't carry
      if (isPickupDone(move) && !isDropoffDone(move)) {
        bag.add(`${move.itemName}|${move.fromVaultKey}`);
      }
    }
    return bag;
  });

  /** Is this dropoff item currently in the player's carry bag? */
  function isInCarryBag(move: PlannedMove): boolean {
    return carryBag.value.has(`${move.itemName}|${move.fromVaultKey}`);
  }

  /** The zone stop for the player's current location, filtered for actionability */
  const currentZoneStop = computed<ZoneStop | null>(() => {
    if (!currentZone.value) return null;
    const raw = plan.value.zoneStops.find((zs) => zs.areaKey === currentZone.value);
    if (!raw) return null;

    // Filter dropoffs to only items actually in the carry bag
    return {
      ...raw,
      dropoffs: raw.dropoffs.filter((d) => isInCarryBag(d)),
    };
  });

  /** Zone stops not yet completed, excluding current zone */
  const remainingZoneStops = computed(() =>
    plan.value.zoneStops.filter((zs) => !zs.completed && zs.areaKey !== currentZone.value)
  );

  const completedCount = computed(() => plan.value.moves.filter((m) => m.completed).length);
  const totalCount = computed(() => plan.value.moves.length);

  /** Next zone the player should travel to (first incomplete zone that isn't the current zone) */
  const nextZoneStop = computed<ZoneStop | null>(() => {
    for (const zs of plan.value.zoneStops) {
      if (zs.completed) continue;
      if (zs.areaKey === currentZone.value) continue;
      // Only suggest zones where the player has actionable work:
      // - pickups are always actionable
      // - dropoffs only if the item is in carry bag
      // - local moves are always actionable
      const hasPickups = zs.pickups.some((p) => !isPickupDone(p));
      const hasDropoffs = zs.dropoffs.some((d) => !isDropoffDone(d) && isInCarryBag(d));
      const hasLocal = zs.localMoves.some((l) => !isLocalDone(l));
      if (hasPickups || hasDropoffs || hasLocal) return zs;
    }
    return null;
  });

  // ── Auto-detection ──────────────────────────────────────────────────
  // Watch storage changes and auto-check moves when items appear/disappear

  /** Snapshot of storage state for change detection */
  const prevStorageSnapshot = ref<Map<string, Map<string, number>>>(new Map());

  function buildStorageSnapshot(): Map<string, Map<string, number>> {
    const snap = new Map<string, Map<string, number>>();
    for (const item of gameState.storage) {
      if (!snap.has(item.vault_key)) snap.set(item.vault_key, new Map());
      const vaultMap = snap.get(item.vault_key)!;
      vaultMap.set(item.item_name, (vaultMap.get(item.item_name) ?? 0) + item.stack_size);
    }
    return snap;
  }

  function snapshotVaultQty(snap: Map<string, Map<string, number>>, vaultKey: string, itemName: string): number {
    return snap.get(vaultKey)?.get(itemName) ?? 0;
  }

  // Watch for storage changes and auto-check matching moves
  watch(() => gameState.storage, () => {
    if (!wizardActive.value) return;

    const newSnap = buildStorageSnapshot();
    const oldSnap = prevStorageSnapshot.value;

    // Only auto-detect if we have a previous snapshot to compare against
    if (oldSnap.size > 0) {
      for (const move of plan.value.moves) {
        if (move.fromAreaKey === move.toAreaKey) continue; // skip local

        // Auto-check pickup: item quantity decreased at source vault
        if (!isPickupDone(move)) {
          const oldQty = snapshotVaultQty(oldSnap, move.fromVaultKey, move.itemName);
          const newQty = snapshotVaultQty(newSnap, move.fromVaultKey, move.itemName);
          if (oldQty > 0 && newQty < oldQty) {
            togglePickup(move);
          }
        }

        // Auto-check dropoff: item quantity increased at target vault
        if (isPickupDone(move) && !isDropoffDone(move)) {
          const oldQty = snapshotVaultQty(oldSnap, move.toVaultKey, move.itemName);
          const newQty = snapshotVaultQty(newSnap, move.toVaultKey, move.itemName);
          if (newQty > oldQty) {
            toggleDropoff(move);
          }
        }
      }

      // Auto-check local moves too
      for (const move of plan.value.moves) {
        if (move.fromAreaKey !== move.toAreaKey) continue;
        if (isLocalDone(move)) continue;

        const oldFrom = snapshotVaultQty(oldSnap, move.fromVaultKey, move.itemName);
        const newFrom = snapshotVaultQty(newSnap, move.fromVaultKey, move.itemName);
        const oldTo = snapshotVaultQty(oldSnap, move.toVaultKey, move.itemName);
        const newTo = snapshotVaultQty(newSnap, move.toVaultKey, move.itemName);

        if (oldFrom > 0 && newFrom < oldFrom && newTo > oldTo) {
          toggleLocal(move);
        }
      }
    }

    prevStorageSnapshot.value = newSnap;
  }, { deep: true });

  // Initialize snapshot on wizard start
  watch(wizardActive, (active) => {
    if (active) {
      prevStorageSnapshot.value = buildStorageSnapshot();
    }
  });

  // ── Route stops for trip planner ────────────────────────────────────

  const routeStops = computed(() => {
    const stops: { zone: string; purpose: string; details: string }[] = [];
    for (const zs of plan.value.zoneStops) {
      if (zs.completed) continue;
      for (const p of zs.pickups) {
        if (p.completed) continue;
        stops.push({
          zone: zs.areaKey,
          purpose: "pickup",
          details: `Pick up ${p.itemName} x${p.quantity} from ${p.fromVaultName}`,
        });
      }
      for (const d of zs.dropoffs) {
        if (d.completed) continue;
        stops.push({
          zone: zs.areaKey,
          purpose: "deposit",
          details: `Deposit ${d.itemName} at ${d.toVaultName}`,
        });
      }
    }
    return stops;
  });

  return {
    plan,
    wizardActive,
    startWizard,
    stopWizard,
    currentZone,
    currentZoneStop,
    nextZoneStop,
    carryBag,
    remainingZoneStops,
    completedCount,
    totalCount,
    // Action tracking (separate pickup/dropoff/local checkboxes)
    isPickupDone,
    isDropoffDone,
    isLocalDone,
    togglePickup,
    toggleDropoff,
    toggleLocal,
    isInCarryBag,
    isMoveCompleted,
    resetCompletion,
    routeStops,
  };
}
