# Storage Consolidation Tool (v2)

An inventory optimization tool that helps players consolidate scattered items, utilize type-specific storage, and execute pickup/dropoff plans zone-by-zone.

## Core Goals

1. **Slot savings analysis**: Show how many storage slots can be freed by consolidating duplicate stacks
2. **Type-specific vault optimization**: Identify items in generic storage that could move to type-restricted vaults (potions vault, alchemy vault, etc.)
3. **Similar item grouping**: Group items by keyword/category (crystals, leather, bones) and suggest consolidation targets
4. **Zone-by-zone execution**: Checklist UI that knows where the player is and shows what to pick up / drop off at the current location

## Two Modes

### Plan Mode (overview)
Shows the full consolidation plan for review before starting:
- **Summary stats**: total slots saveable, items to move, zones to visit
- **Per-zone breakdown**: collapsible sections showing each zone's pickups and dropoffs
- **Optimization suggestions**: type-specific vault opportunities highlighted
- "Start" button enters Wizard Mode

### Wizard Mode (execution)
Step-by-step zone checklist driven by the player's current location:
- **Current zone panel**: shows what to pick up and drop off HERE, based on `current_area` from game state
- **Auto-detection**: as items enter/leave inventory (via ProcessAddItem/ProcessDeleteItem events), checklist items auto-check
- **Next zone indicator**: shows where to go next and what's waiting there
- **Progress bar**: X of Y zones completed, items moved

## Data Sources

### Consolidation Candidates
- `gameState.storage` — all items across all vaults
- `gameState.storageVaultsByKey` — vault metadata including `area`, `required_item_keywords`, slot counts
- Items in 2+ locations → consolidation candidate
- Items in generic vault when type-specific vault exists and has room → optimization candidate

### Type-Specific Vault Matching
- `StorageVaultDetail.required_item_keywords` — what items a vault accepts
- Cross-reference item keywords against vault restrictions
- Example: vault requires `["AlchemyRelated", "Potion", "BottledItem"]` — any matching items in generic storage could move there

### Player Location
- `gameState.world.area` — current zone for wizard mode
- `game-state-updated` events with domain `"area"` — react to zone changes

### Auto-Detection (Wizard Mode)
- Listen for `game-state-updated` with domain `"inventory"` or `"storage"` 
- Compare inventory/storage state changes against the plan's expected pickups/dropoffs
- Auto-check items as they move

## Plan Generation Algorithm

1. **Find duplicates**: items in 2+ vaults → consolidate to the vault with the most
2. **Find type-specific opportunities**: for each item, check if any type-restricted vault accepts it AND has capacity
3. **Build move list**: `{ item, quantity, from_vault, to_vault }`
4. **Group by zone**: organize moves by source/destination zone for the route
5. **Optimize route**: call `plan_trip` to determine zone visit order
6. **Build per-zone checklists**: for each zone in route order, list pickups then dropoffs

## UI Layout

### Plan Mode Layout
```
┌─────────────────────────────────────────────────────┐
│ [Summary bar: 47 slots saveable | 89 items | 12 zones | Start Wizard] │
├─────────────────────────────────────────────────────┤
│ ZONE: Serbule                              8 items  │
│ ┌─ PICK UP ────────────────────────────────────────┐│
│ │ □ Salt x30          from Ashk's Storage          ││
│ │ □ Iron Filament x20 from Marna                   ││
│ ├─ DROP OFF ───────────────────────────────────────┐│
│ │ □ Healing Potion x5 to Potions Vault (from inv)  ││
│ └──────────────────────────────────────────────────┘│
│ ZONE: Eltibule                             3 items  │
│ ┌─ PICK UP ────────────────────────────────────────┐│
│ │ □ Grass x7          from Kib the Unkillable      ││
│ ...                                                 │
└─────────────────────────────────────────────────────┘
```

### Wizard Mode Layout
```
┌─────────────────────────────────────────────────────┐
│ [Progress: ████████░░ 8/12 zones | 67 items moved]  │
├──────────────────────┬──────────────────────────────┤
│ CURRENT ZONE         │ NEXT STOP                    │
│ Serbule              │ Eltibule (3 items)           │
│                      │                              │
│ ── PICK UP ───────── │ After that:                  │
│ ✓ Salt x30           │ • Kur Mountains (2 items)    │
│ □ Iron Filament x20  │ • Rahu (5 items)             │
│                      │ • Casino (1 item)            │
│ ── DROP OFF ──────── │                              │
│ □ Healing Potion x5  │                              │
│   → Potions Vault    │                              │
│                      │                              │
└──────────────────────┴──────────────────────────────┘
```

## Phases

### Phase 1: Smart Plan Generation
- Composable: duplicate detection + type-specific vault matching + move list generation
- Slot savings calculation
- Per-zone grouping with route ordering

### Phase 2: Plan Mode UI
- Summary stats bar
- Zone-by-zone collapsible sections with pickup/dropoff checklists
- Manual checkbox toggling
- Route planning integration

### Phase 3: Wizard Mode
- Current-zone detection from game state
- Active zone panel with pickup/dropoff checklist
- Next zone indicator + remaining zones list
- Progress tracking

### Phase 4: Auto-Detection
- Listen for inventory/storage change events
- Match against plan expectations
- Auto-check completed moves
- Toast notifications on auto-detected moves

### Cross-alt consolidation (DONE)

An **"Include all alts' storage"** toggle pools storage from *every* character on the
active server (via `characterStore.loadAllCharacterItems()` → `get_all_character_items`,
which backfills each character's Storage report and returns items tagged with their owner).

- **Per-character storage:** each alt owns separate storage at every NPC, so a vault's
  identity in this mode is the composite `(character, server, vault_key)`, not `vault_key`
  alone. In single-character mode the id collapses back to the raw `vault_key` (no behavior
  change).
- **Bank targeting:** a **"Gather onto ⟨character⟩"** selector designates a bank/mule
  character (defaults to the active character). Its vaults rank first in target selection,
  so duplicates gather onto the bank where capacity allows; if the bank doesn't hold an
  item, selection falls back to the vault holding the most.
- **Capacity for alts:** favor/attribute unlocks are only known for the active character,
  so alt vaults use the theoretical max (`getVaultMaxPossibleSlots` — exact for fixed-slot
  vaults, an upper bound for favor-gated ones). Occupancy is always exact (row counts).
- **Cross-character moves** are labeled with the owning character and a `⇄` badge; they
  require switching characters (withdraw → mail/trade → deposit) and are not auto-detected
  by the wizard (which tails only the active character's live storage).
- **Noise reduction (`isCandidate`):** worn/gear items (`ItemInfo.equip_slot` set) are always
  excluded from suggestions (both modes). In alt mode a **category dropdown** (populated from
  the item keywords present on the bank's non-gear items, `Lint_*` filtered out) scopes the
  plan to one keyword, and suggestions are further limited to items the **bank character
  already holds** (`bankHeldItems`, storage or inventory) — so you top up the bank's existing
  collections instead of importing arbitrary items. Occupancy/capacity still counts *all*
  stored items (including gear), so the filter only narrows candidates, never capacity.
  `ItemInfo.equip_slot` + `keywords` are cached alongside `max_stack_size` in one
  `resolveItemsBatch` pass (`itemMetaByName`).

### Phase 5: Polish
- Type-specific vault suggestions highlighted with special styling
- "Similar items" grouping suggestions
- Persist plan across navigation (useViewPrefs)
- Handle edge cases (vault full, item moved by other means)
  - **Vault-full handling: DONE.** Plan generation is capacity-aware. Each vault
    gets a running free-slot budget (`getVaultUnlockedSlots` ?? `getVaultMaxPossibleSlots`,
    minus current occupancy; `null` capacity = unconstrained). A consolidation target
    is only chosen if it can receive the incoming stacks. Slot cost depends on item
    `max_stack_size`: stackable items merge (often 0 new slots), non-stackable gear
    needs one slot per stack. If the natural best holder can't fit everything, the
    planner reroutes to the roomiest holder that can, else does a partial fill and
    flags the leftover via `ConsolidationPlan.blockedItems` (surfaced as a "Storage
    full" warning banner). `slotsSaved` is now the accurate net (source stacks freed
    minus new target slots) rather than a raw move count.

## Key Files

- [useStorageConsolidation.ts](../../src/composables/useStorageConsolidation.ts) — composable (needs major expansion)
- [ConsolidateTab.vue](../../src/components/Inventory/ConsolidateTab.vue) — UI component
- [gameStateStore.ts](../../src/stores/gameStateStore.ts) — storage data, vault metadata, current area
- [trip_router.rs](../../src-tauri/src/trip_router.rs) — route planning
- [zone_graph.rs](../../src-tauri/src/zone_graph.rs) — zone connectivity
