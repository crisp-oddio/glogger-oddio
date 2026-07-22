import { ref, computed, watch, type ComputedRef } from "vue";
import { useGameStateStore } from "../stores/gameStateStore";
import { useGameDataStore } from "../stores/gameDataStore";
import { useCharacterStore } from "../stores/characterStore";
import { useSettingsStore } from "../stores/settingsStore";
import { useViewPrefs } from "./useViewPrefs";

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
  /** Owning character of the source vault (for cross-alt moves). */
  fromCharacter: string;
  /** Owning character of the target vault (for cross-alt moves). */
  toCharacter: string;
  /** True when source and target belong to different characters. */
  crossCharacter: boolean;
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
  const characterStore = useCharacterStore();
  const settingsStore = useSettingsStore();

  const wizardActive = ref(false);
  const completedMoves = ref<Set<string>>(new Set());

  // ── Cross-alt options (persisted) ────────────────────────────────────────
  // When `includeAlts` is on, storage from every character on the active server
  // is pooled and duplicates are gathered onto `bankKey` (a designated character)
  // where possible. Each alt owns separate storage at every NPC, so a vault's
  // identity in this mode is (character, server, vault_key), not vault_key alone.

  const { prefs, update } = useViewPrefs("inventory.consolidate", {
    includeAlts: false as boolean,
    bankKey: "" as string,
    category: "" as string,
  });
  const includeAlts = computed<boolean>({
    get: () => prefs.value.includeAlts,
    set: (v) => update({ includeAlts: v }),
  });
  const bankKey = computed<string>({
    get: () => prefs.value.bankKey,
    set: (v) => update({ bankKey: v }),
  });
  /** Item-keyword filter (empty = all categories). Only applied in alt mode. */
  const category = computed<string>({
    get: () => prefs.value.category,
    set: (v) => update({ category: v }),
  });

  const activeCharacter = computed(() => settingsStore.settings.activeCharacterName ?? "");
  const activeServer = computed(() => settingsStore.settings.activeServerName ?? "");

  /** Separator for composite (character, server, vault) identities. */
  const SEP = "";
  /** Owner token for account-wide "transfer chest" storage shared by all characters. */
  const SHARED = "*Account*";
  /** Account-wide transfer chests (`*AccountStorage_*`) are shared across every
   *  character, so their contents must be counted once, not once per character. */
  function isAccountVault(vaultKey: string): boolean {
    return vaultKey.startsWith("*AccountStorage");
  }
  function ownerKey(character: string, server: string): string {
    return `${character}${SEP}${server}`;
  }
  /** Friendly owner label; the shared account token renders as "Account". */
  function ownerDisplay(character: string): string {
    return character === SHARED ? "Account" : character;
  }
  /** Composite vault id. Single-character mode uses the raw vault_key; in alt mode
   *  account/transfer chests get the shared owner so they collapse to one vault. */
  function makeVaultId(character: string, server: string, vaultKey: string): string {
    if (!includeAlts.value) return vaultKey;
    const owner = isAccountVault(vaultKey) ? SHARED : character;
    return `${owner}${SEP}${server}${SEP}${vaultKey}`;
  }
  function parseVaultId(id: string): { character: string; server: string; vaultKey: string } {
    const first = id.indexOf(SEP);
    if (first === -1) {
      return { character: activeCharacter.value, server: activeServer.value, vaultKey: id };
    }
    const last = id.lastIndexOf(SEP);
    return { character: id.slice(0, first), server: id.slice(first + 1, last), vaultKey: id.slice(last + 1) };
  }

  /** The character duplicates are gathered onto (defaults to the active character). */
  const bankOwnerKey = computed(() =>
    prefs.value.bankKey || ownerKey(activeCharacter.value, activeServer.value),
  );
  function isBankVault(id: string): boolean {
    const { character, server } = parseVaultId(id);
    return ownerKey(character, server) === bankOwnerKey.value;
  }

  /** Every item the bank character currently holds (storage or inventory).
   *  Only populated in alt mode; used to scope suggestions to what the bank has. */
  const bankHeldItems = computed<Set<string>>(() => {
    const set = new Set<string>();
    if (!includeAlts.value) return set;
    for (const it of characterStore.allCharacterItems) {
      if (ownerKey(it.character_name, it.server_name) === bankOwnerKey.value) set.add(it.item_name);
    }
    return set;
  });

  // Load every character's storage on demand when alt-inclusion turns on.
  watch(
    includeAlts,
    (on) => { if (on) characterStore.loadAllCharacterItems(); },
    { immediate: true },
  );

  // ── Unified storage dataset ──────────────────────────────────────────────
  interface OwnedItem { character: string; server: string; vaultKey: string; itemName: string; stackSize: number }

  /** One character whose account-storage rows represent the shared transfer chests
   *  (prefer the active character; else the first character that has any). Used to
   *  count account/transfer-chest contents once instead of once per character. */
  const canonicalAccountOwner = computed(() => {
    const activeK = ownerKey(activeCharacter.value, activeServer.value);
    const owners = new Set<string>();
    let first = "";
    for (const it of characterStore.allCharacterItems) {
      if (!it.is_in_inventory && isAccountVault(it.storage_vault)) {
        const k = ownerKey(it.character_name, it.server_name);
        owners.add(k);
        if (!first) first = k;
      }
    }
    return owners.has(activeK) ? activeK : first;
  });

  const ownedStorage = computed<OwnedItem[]>(() => {
    if (includeAlts.value) {
      const canonical = canonicalAccountOwner.value;
      return characterStore.allCharacterItems
        .filter((it) => !it.is_in_inventory && it.storage_vault)
        // account/transfer chests are shared — keep only one character's copy
        .filter((it) => !isAccountVault(it.storage_vault)
          || ownerKey(it.character_name, it.server_name) === canonical)
        .map((it) => ({
          character: it.character_name,
          server: it.server_name,
          vaultKey: it.storage_vault,
          itemName: it.item_name,
          stackSize: it.stack_size,
        }));
    }
    const ch = activeCharacter.value;
    const sv = activeServer.value;
    return gameState.storage.map((it) => ({
      character: ch, server: sv, vaultKey: it.vault_key, itemName: it.item_name, stackSize: it.stack_size,
    }));
  });

  /** Occupied slots per composite vault id (every stored stack counts as one slot). */
  const occupiedByVault = computed<Map<string, number>>(() => {
    const m = new Map<string, number>();
    for (const it of ownedStorage.value) {
      const id = makeVaultId(it.character, it.server, it.vaultKey);
      m.set(id, (m.get(id) ?? 0) + 1);
    }
    return m;
  });

  // ── Vault helpers ───────────────────────────────────────────────────────

  function vaultName(id: string): string {
    const { vaultKey } = parseVaultId(id);
    const detail = gameState.storageVaultsByKey[vaultKey];
    return detail?.npc_friendly_name ?? vaultKey;
  }

  function vaultArea(id: string): string | null {
    return gameState.storageVaultsByKey[parseVaultId(id).vaultKey]?.area ?? null;
  }

  /** Vault name, prefixed with its owning character when alts are pooled. */
  function vaultLabel(id: string): string {
    const name = vaultName(id);
    return includeAlts.value ? `[${ownerDisplay(parseVaultId(id).character)}] ${name}` : name;
  }

  // ── Capacity helpers ────────────────────────────────────────────────────

  /**
   * Slot capacity of a vault. Favor/attribute unlocks are only known for the
   * active character; for alt-owned vaults we fall back to the theoretical max
   * (accurate for fixed-slot vaults, an upper bound for favor-gated ones).
   * null = unknown (unconstrained).
   */
  function vaultCapacity(id: string): number | null {
    const { character, server, vaultKey } = parseVaultId(id);
    const vault = gameState.storageVaultsByKey[vaultKey];
    if (!vault) return null;
    if (character === activeCharacter.value && server === activeServer.value) {
      return gameState.getVaultUnlockedSlots(vault) ?? gameState.getVaultMaxPossibleSlots(vault);
    }
    return gameState.getVaultMaxPossibleSlots(vault);
  }

  /** Currently occupied slots in a vault (one storage entry == one slot). */
  function vaultOccupied(id: string): number {
    return occupiedByVault.value.get(id) ?? 0;
  }

  // ── Item metadata cache ────────────────────────────────────────────────
  // max_stack_size decides how many slots a consolidation consumes; equip_slot
  // marks gear (excluded from suggestions); keywords drive the category filter.
  // Fetched lazily from the CDN; unresolved names are treated as non-stackable
  // and non-equipment.

  interface ItemMeta { maxStack: number | null; equipSlot: string | null; keywords: string[] }
  const itemMetaByName = ref<Map<string, ItemMeta | null>>(new Map());

  watch(
    ownedStorage,
    async () => {
      const missing = new Set<string>();
      for (const item of ownedStorage.value) {
        if (!itemMetaByName.value.has(item.itemName)) missing.add(item.itemName);
      }
      if (missing.size === 0) return;
      const names = [...missing];
      const next = new Map(itemMetaByName.value);
      try {
        const infos = await gameData.resolveItemsBatch(names);
        for (const name of names) {
          const info = infos[name];
          next.set(name, info
            ? { maxStack: info.max_stack_size, equipSlot: info.equip_slot, keywords: info.keywords ?? [] }
            : null);
        }
      } catch {
        // On failure treat as unknown → non-stackable, non-equipment (conservative)
        for (const name of names) if (!next.has(name)) next.set(name, null);
      }
      itemMetaByName.value = next;
    },
    { immediate: true, deep: true },
  );

  /** Max stack size for an item: >1 stackable, 1 non-stackable, Infinity if not yet resolved. */
  function maxStack(itemName: string): number {
    if (!itemMetaByName.value.has(itemName)) return Infinity; // avoid false "full" during load
    const s = itemMetaByName.value.get(itemName)?.maxStack ?? null;
    return s != null && s > 1 ? s : 1;
  }

  /** True when the item is wearable gear (has an equip slot). */
  function isEquipment(itemName: string): boolean {
    return !!itemMetaByName.value.get(itemName)?.equipSlot;
  }

  function itemKeywords(itemName: string): string[] {
    return itemMetaByName.value.get(itemName)?.keywords ?? [];
  }

  /**
   * Whether an item should appear as a consolidation candidate. Gear is always
   * excluded. In alt mode, suggestions are further scoped to the selected
   * category and to items the bank character already holds.
   */
  function isCandidate(itemName: string): boolean {
    if (isEquipment(itemName)) return false;
    if (!includeAlts.value) return true;
    if (category.value && !itemKeywords(itemName).includes(category.value)) return false;
    return bankHeldItems.value.has(itemName);
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

    // Group: item_name → vaultId → { qty, stacks } (one storage entry == one slot).
    // vaultId is the composite (character, server, vault_key) so each alt's storage
    // at the same NPC stays distinct.
    interface Holding { vk: string; qty: number; stacks: number }
    const itemVaults = new Map<string, Map<string, Holding>>();
    for (const item of ownedStorage.value) {
      if (!isCandidate(item.itemName)) continue; // exclude gear / off-category / non-bank items
      const id = makeVaultId(item.character, item.server, item.vaultKey);
      if (!itemVaults.has(item.itemName)) itemVaults.set(item.itemName, new Map());
      const vm = itemVaults.get(item.itemName)!;
      const cur = vm.get(id) ?? { vk: id, qty: 0, stacks: 0 };
      cur.qty += item.stackSize;
      cur.stacks += 1;
      vm.set(id, cur);
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

    const bankRank = (vk: string) => (includeAlts.value && isBankVault(vk) ? 1 : 0);

    function pushMove(itemName: string, src: Holding, targetKey: string) {
      const moveKey = `${itemName}|${src.vk}|${targetKey}`;
      const from = parseVaultId(src.vk);
      const to = parseVaultId(targetKey);
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
        fromCharacter: ownerDisplay(from.character),
        toCharacter: ownerDisplay(to.character),
        // Account/transfer chests are reachable from any character, so a move that
        // touches one is NOT a character switch — only real-char↔real-char moves are.
        crossCharacter: from.character !== SHARED && to.character !== SHARED
          && ownerKey(from.character, from.server) !== ownerKey(to.character, to.server),
      });
    }

    // Process item groups in a stable order for deterministic plans.
    const itemNames = [...itemVaults.keys()].sort();
    for (const itemName of itemNames) {
      const vaultMap = itemVaults.get(itemName)!;
      if (vaultMap.size < 2) continue;

      // Candidate holders. With a designated bank, its vaults rank first so
      // duplicates gather onto the bank where capacity allows; otherwise
      // most-of-this-item first (fewest moves, most already there).
      const holders = [...vaultMap.values()].sort(
        (a, b) => bankRank(b.vk) - bankRank(a.vk) || b.qty - a.qty || b.stacks - a.stacks,
      );
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

      // No holder can take everything — reroute to the roomiest holder (bank
      // first when designated) and move as many stacks as fit, flagging the rest.
      const roomiest = [...holders].sort(
        (a, b) => bankRank(b.vk) - bankRank(a.vk) || budget(b.vk) - budget(a.vk) || b.qty - a.qty,
      )[0];
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
        leftoverVaultNames: left.map((h) => vaultLabel(h.vk)),
        targetVaultName: vaultLabel(roomiest.vk),
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

  // ── Cross-alt UI helpers ─────────────────────────────────────────────────

  /** Characters available to pool/target: the active character plus every alt
   *  seen in the cross-character data, de-duplicated by (character, server). */
  const availableCharacters = computed(() => {
    const map = new Map<string, { key: string; character: string; server: string }>();
    if (activeCharacter.value) {
      const k = ownerKey(activeCharacter.value, activeServer.value);
      map.set(k, { key: k, character: activeCharacter.value, server: activeServer.value });
    }
    for (const it of characterStore.allCharacterItems) {
      const k = ownerKey(it.character_name, it.server_name);
      if (!map.has(k)) map.set(k, { key: k, character: it.character_name, server: it.server_name });
    }
    return [...map.values()].sort((a, b) => a.character.localeCompare(b.character));
  });

  const altsLoading = computed(() => characterStore.allCharacterItemsLoading);
  /** Distinct characters contributing storage to the current plan. */
  const charactersInPlan = computed(() => {
    const set = new Set<string>();
    for (const it of ownedStorage.value) set.add(ownerKey(it.character, it.server));
    return set.size;
  });

  /** Item-keyword categories present among the bank's non-gear stored items,
   *  with a per-category distinct-item count. Drives the category dropdown. */
  const availableCategories = computed(() => {
    if (!includeAlts.value) return [] as { keyword: string; count: number }[];
    const counts = new Map<string, number>();
    const seen = new Set<string>();
    for (const it of ownedStorage.value) {
      if (seen.has(it.itemName)) continue;
      if (isEquipment(it.itemName)) continue;
      if (!bankHeldItems.value.has(it.itemName)) continue;
      seen.add(it.itemName);
      for (const kw of itemKeywords(it.itemName)) {
        if (kw.startsWith("Lint_")) continue; // internal/dev keywords
        counts.set(kw, (counts.get(kw) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .map(([keyword, count]) => ({ keyword, count }))
      .sort((a, b) => a.keyword.localeCompare(b.keyword));
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
    // Cross-alt options
    includeAlts,
    bankKey,
    bankOwnerKey,
    category,
    availableCharacters,
    availableCategories,
    altsLoading,
    charactersInPlan,
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
