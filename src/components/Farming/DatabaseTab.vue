<template>
  <div class="flex flex-col gap-4 h-full overflow-hidden">
    <!-- Scope + import/export toolbar -->
    <div class="flex items-center justify-between gap-3 flex-wrap">
      <div
        class="flex items-center gap-1 bg-surface-dark border border-border-default rounded-lg p-1"
        :class="scopeDisabled ? 'opacity-40' : ''"
        :title="scopeDisabled ? 'Harvested items are recorded locally only — no scope.' : ''">
        <button
          v-for="opt in scopeOptions"
          :key="opt.value"
          @click="scope = opt.value"
          :disabled="scopeDisabled"
          :class="[
            'px-3 py-1.5 text-xs rounded font-medium transition-colors',
            scopeDisabled ? 'cursor-not-allowed' : 'cursor-pointer',
            scope === opt.value
              ? 'bg-entity-item/20 text-entity-item'
              : 'text-text-secondary hover:text-text-primary'
          ]">
          {{ opt.label }}
        </button>
      </div>

      <div class="flex items-center gap-2">
        <button
          @click="doExport"
          :disabled="exporting"
          class="px-3 py-1.5 text-xs bg-[#2a2a3a]! border border-[#4a4a5a]! rounded text-text-secondary cursor-pointer transition-all font-medium hover:bg-[#3a3a4e] hover:text-text-primary disabled:opacity-50 disabled:cursor-not-allowed">
          {{ exporting ? "Exporting…" : "Export My Data" }}
        </button>
        <button
          @click="doImport"
          :disabled="importing"
          class="px-3 py-1.5 text-xs bg-[#2a3a2a]! border border-[#4a5a4a]! rounded text-value-positive! cursor-pointer transition-all font-medium hover:bg-[#3a4a3a] disabled:opacity-50 disabled:cursor-not-allowed">
          {{ importing ? "Importing…" : "Import Database" }}
        </button>
      </div>
    </div>

    <p v-if="importMessage" class="text-xs text-value-positive">{{ importMessage }}</p>
    <p v-if="errorMessage" class="text-xs text-value-negative">{{ errorMessage }}</p>

    <!-- Historical log backfill -->
    <div class="bg-surface-dark border border-border-default rounded-lg p-3 flex items-center justify-between gap-3 flex-wrap">
      <div class="flex flex-col gap-0.5 min-w-0">
        <span class="text-xs text-text-secondary font-medium">Backfill from a Player.log</span>
        <span class="text-[0.65rem] text-text-dim">
          Scan a kept Player.log to add its kills/loot. Player-prev.log is backed up automatically on game restart.
        </span>
        <span v-if="scanMessage" class="text-[0.65rem] text-value-positive mt-0.5">{{ scanMessage }}</span>
      </div>
      <button
        @click="doScan"
        :disabled="scanning"
        class="px-3 py-1.5 text-xs bg-[#2a3a3a]! border border-[#4a5a5a]! rounded text-[#7ea4c8]! cursor-pointer transition-all font-medium hover:bg-[#3a4a4a] disabled:opacity-50 disabled:cursor-not-allowed shrink-0">
        {{ scanning ? "Scanning…" : "Scan a log file…" }}
      </button>
    </div>

    <!-- Search / filter -->
    <div class="flex items-center gap-3">
      <div class="flex items-center gap-1 bg-surface-dark border border-border-default rounded-lg p-1 shrink-0">
        <button
          v-for="opt in searchTargetOptions"
          :key="opt.value"
          @click="searchTarget = opt.value"
          :class="[
            'px-3 py-1.5 text-xs rounded font-medium transition-colors cursor-pointer',
            searchTarget === opt.value
              ? 'bg-entity-item/20 text-entity-item'
              : 'text-text-secondary hover:text-text-primary'
          ]">
          {{ opt.label }}
        </button>
      </div>
      <div class="relative flex-1">
        <input
          v-model="query"
          type="text"
          :placeholder="filterPlaceholder"
          class="w-full px-3 py-2 pr-8 text-sm bg-surface-card border border-border-light rounded text-text-primary placeholder-text-dim outline-none focus:border-entity-item"
        />
        <button
          v-if="query"
          @click="query = ''"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-text-dim hover:text-text-primary cursor-pointer text-sm leading-none"
          title="Clear filter">
          ✕
        </button>
      </div>
    </div>

    <!-- Results -->
    <div class="flex-1 min-h-0 overflow-y-auto">
      <EmptyState
        v-if="loading && results.length === 0"
        variant="compact"
        primary="Loading…"
        secondary="Reading your drop-rate database." />
      <EmptyState
        v-else-if="results.length === 0 && query.trim().length > 0"
        variant="compact"
        primary="No matches"
        :secondary="`No ${entityNoun} contains “${query.trim()}”.`" />
      <EmptyState
        v-else-if="results.length === 0"
        variant="compact"
        :primary="emptyPrimary"
        :secondary="emptySecondary" />

      <div v-else class="flex flex-col gap-1">
        <div class="text-[0.65rem] text-text-dim px-1 pb-0.5">
          {{ results.length.toLocaleString() }}
          {{ entityNoun }}{{ results.length === 1 ? '' : 's' }}
          <template v-if="query.trim()">matching “{{ query.trim() }}”</template>
        </div>

        <!-- Enemy results -->
        <template v-if="searchTarget === 'enemies'">
          <div
            v-for="row in enemyResults"
            :key="row.enemy_name"
            class="rounded text-xs bg-black/20 border border-border-default">
            <div
              class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-black/30 transition-colors"
              @click="toggleEnemyExpanded(row.enemy_name)">
              <EnemyInline :reference="row.enemy_name" />
              <div class="flex items-center gap-3 shrink-0">
                <span class="text-text-secondary">{{ row.total_kills.toLocaleString() }} kills</span>
                <span class="text-text-dim">{{ row.distinct_loot_items }} loot items</span>
                <span class="text-text-dim">{{ expandedEnemies.has(row.enemy_name) ? '▲' : '▼' }}</span>
              </div>
            </div>
            <div v-if="expandedEnemies.has(row.enemy_name)" class="border-t border-border-default px-3 py-2">
              <EnemyDropTable :enemy-name="row.enemy_name" :scope="scope" />
            </div>
          </div>
        </template>

        <!-- Item results -->
        <template v-else-if="searchTarget === 'items'">
          <div
            v-for="row in itemResults"
            :key="row.item_name"
            class="rounded text-xs bg-black/20 border border-border-default">
            <div
              class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-black/30 transition-colors"
              @click="toggleItemExpanded(row.item_name)">
              <ItemInline :reference="row.item_name" />
              <div class="flex items-center gap-3 shrink-0">
                <span class="text-text-secondary">{{ row.total_quantity.toLocaleString() }} looted</span>
                <span class="text-text-dim">{{ row.distinct_enemies }} sources</span>
                <span class="text-text-dim">{{ expandedItems.has(row.item_name) ? '▲' : '▼' }}</span>
              </div>
            </div>
            <div v-if="expandedItems.has(row.item_name)" class="border-t border-border-default px-3 py-2">
              <ItemDropBreakdownTable :item-name="row.item_name" :scope="scope" />
            </div>
          </div>
        </template>

        <!-- Harvested results (skinning/butchering) -->
        <template v-else>
          <div
            v-for="row in harvestedResults"
            :key="row.item_name"
            class="rounded text-xs bg-black/20 border border-border-default">
            <div
              class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-black/30 transition-colors"
              @click="toggleHarvestedExpanded(row.item_name)">
              <ItemInline :reference="row.item_name" />
              <div class="flex items-center gap-3 shrink-0">
                <span class="text-text-secondary">{{ row.total_quantity.toLocaleString() }} harvested</span>
                <span class="text-text-dim">{{ row.distinct_corpses }} {{ row.distinct_corpses === 1 ? 'corpse' : 'corpses' }}</span>
                <span class="text-text-dim">{{ expandedHarvested.has(row.item_name) ? '▲' : '▼' }}</span>
              </div>
            </div>
            <div v-if="expandedHarvested.has(row.item_name)" class="border-t border-border-default px-3 py-2">
              <ExtractDetailTable :item-name="row.item_name" />
            </div>
          </div>
        </template>
      </div>
    </div>

    <!-- Imported sources management -->
    <div v-if="importedSources.length > 0" class="bg-surface-dark border border-border-default rounded-lg p-3 max-h-40 overflow-y-auto shrink-0">
      <div class="text-[0.65rem] uppercase tracking-widest text-text-dim font-bold mb-1">Imported Sources</div>
      <div class="text-[0.65rem] text-text-dim mb-2">Removing an entry only clears it from this list — the imported data stays merged into your database.</div>
      <div class="flex flex-col gap-1">
        <div
          v-for="src in importedSources"
          :key="src.source_label"
          class="flex items-center justify-between px-2 py-1.5 rounded text-xs bg-black/20 border border-border-default">
          <span class="text-text-secondary truncate">{{ src.display_name }}</span>
          <div class="flex items-center gap-3 shrink-0">
            <span class="text-text-dim">{{ src.enemy_count }} enemies</span>
            <span class="text-text-dim">{{ formatDate(src.imported_at) }}</span>
            <button
              @click="removeSource(src.source_label)"
              class="text-value-negative hover:text-[#e87e7e] cursor-pointer">
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import type {
  DatabaseScope,
  EnemySearchResult,
  ItemSearchResult,
  HarvestSearchResult,
  ImportSummary,
  ImportedSource,
  IngestResult,
} from "../../types/farming";
import EmptyState from "../Shared/EmptyState.vue";
import ItemInline from "../Shared/Item/ItemInline.vue";
import EnemyInline from "../Shared/Enemy/EnemyInline.vue";
import EnemyDropTable from "./EnemyDropTable.vue";
import ItemDropBreakdownTable from "./ItemDropBreakdownTable.vue";
import ExtractDetailTable from "./ExtractDetailTable.vue";
import { formatDateTimeShort } from "../../composables/useTimestamp";

const scope = ref<DatabaseScope>("combined");
const scopeOptions: Array<{ value: DatabaseScope; label: string }> = [
  { value: "mine", label: "My Data" },
  { value: "imported", label: "Imported" },
  { value: "combined", label: "Combined" },
];

type SearchTarget = "enemies" | "items" | "harvested";
const searchTarget = ref<SearchTarget>("enemies");
const searchTargetOptions: Array<{ value: SearchTarget; label: string }> = [
  { value: "enemies", label: "Monsters" },
  { value: "items", label: "Items" },
  { value: "harvested", label: "Harvested" },
];

// Scope only applies to kill/loot data (mine vs imported); harvested extracts
// are recorded locally only.
const scopeDisabled = computed(() => searchTarget.value === "harvested");

// Full lists for the current target + scope, loaded up front. The text box
// filters these client-side so results isolate instantly as you type.
const query = ref("");
const loading = ref(false);
const allEnemies = ref<EnemySearchResult[]>([]);
const allItems = ref<ItemSearchResult[]>([]);
const allHarvested = ref<HarvestSearchResult[]>([]);

const enemyResults = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return allEnemies.value;
  return allEnemies.value.filter((r) => r.enemy_name.toLowerCase().includes(q));
});
const itemResults = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return allItems.value;
  return allItems.value.filter((r) => r.item_name.toLowerCase().includes(q));
});
const harvestedResults = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return allHarvested.value;
  return allHarvested.value.filter((r) => r.item_name.toLowerCase().includes(q));
});
const results = computed(() => {
  if (searchTarget.value === "enemies") return enemyResults.value;
  if (searchTarget.value === "items") return itemResults.value;
  return harvestedResults.value;
});

const entityNoun = computed(() =>
  searchTarget.value === "enemies" ? "monster" : searchTarget.value === "items" ? "item" : "harvested item",
);
const filterPlaceholder = computed(() =>
  searchTarget.value === "enemies"
    ? "Filter monsters…"
    : searchTarget.value === "items"
      ? "Filter items…"
      : "Filter harvested items…",
);
const emptyPrimary = computed(() =>
  searchTarget.value === "enemies"
    ? "No monsters recorded yet"
    : searchTarget.value === "items"
      ? "No items recorded yet"
      : "No harvested items yet",
);
const emptySecondary = computed(() =>
  searchTarget.value === "harvested"
    ? "Skin or butcher corpses while a Player.log is being read to fill this list."
    : "Kill and loot enemies, scan a Player.log, or import a database to fill this list.",
);

const expandedEnemies = ref<Set<string>>(new Set());
const expandedItems = ref<Set<string>>(new Set());
const expandedHarvested = ref<Set<string>>(new Set());

function toggleEnemyExpanded(name: string) {
  if (expandedEnemies.value.has(name)) expandedEnemies.value.delete(name);
  else expandedEnemies.value.add(name);
  expandedEnemies.value = new Set(expandedEnemies.value);
}

function toggleItemExpanded(name: string) {
  if (expandedItems.value.has(name)) expandedItems.value.delete(name);
  else expandedItems.value.add(name);
  expandedItems.value = new Set(expandedItems.value);
}

function toggleHarvestedExpanded(name: string) {
  if (expandedHarvested.value.has(name)) expandedHarvested.value.delete(name);
  else expandedHarvested.value.add(name);
  expandedHarvested.value = new Set(expandedHarvested.value);
}

// Load the full list for the active target + scope (empty query, no limit).
async function loadDatabase() {
  loading.value = true;
  try {
    if (searchTarget.value === "enemies") {
      allEnemies.value = await invoke<EnemySearchResult[]>("search_database_enemies", {
        query: "",
        scope: scope.value,
        limit: null,
      });
    } else if (searchTarget.value === "items") {
      allItems.value = await invoke<ItemSearchResult[]>("search_database_items", {
        query: "",
        scope: scope.value,
        limit: null,
      });
    } else {
      allHarvested.value = await invoke<HarvestSearchResult[]>("search_database_harvested", {
        query: "",
        limit: null,
      });
    }
  } catch (e) {
    console.error("[database-tab] Failed to load database:", e);
  } finally {
    loading.value = false;
  }
}

// Reload whenever the box (Monsters/Items/Harvested) or scope changes; clear stale expansions.
watch([searchTarget, scope], () => {
  expandedEnemies.value = new Set();
  expandedItems.value = new Set();
  expandedHarvested.value = new Set();
  loadDatabase();
});

// ── Import / Export ─────────────────────────────────────────────────────

const exporting = ref(false);
const importing = ref(false);
const importMessage = ref("");
const errorMessage = ref("");
const importedSources = ref<ImportedSource[]>([]);

async function loadImportedSources() {
  try {
    importedSources.value = await invoke<ImportedSource[]>("list_imported_sources");
  } catch (e) {
    console.error("[database-tab] Failed to load imported sources:", e);
  }
}

async function doExport() {
  errorMessage.value = "";
  importMessage.value = "";
  exporting.value = true;
  try {
    const filePath = await save({
      filters: [{ name: "JSON", extensions: ["json"] }],
      defaultPath: `glogger-drop-rates-${new Date().toISOString().slice(0, 10)}.json`,
    });
    if (!filePath) return;
    const count = await invoke<number>("export_kill_loot_database", { path: filePath });
    importMessage.value = `Exported ${count} enemies' drop data to ${filePath}.`;
  } catch (e) {
    errorMessage.value = `Export failed: ${e}`;
  } finally {
    exporting.value = false;
  }
}

async function doImport() {
  errorMessage.value = "";
  importMessage.value = "";
  importing.value = true;
  try {
    const filePath = await open({
      filters: [{ name: "JSON", extensions: ["json"] }],
      multiple: false,
    });
    if (!filePath) return;
    const summary = await invoke<ImportSummary>("import_kill_loot_database", { path: filePath as string });
    importMessage.value = `Merged ${summary.enemies_imported} enemies (${summary.loot_rows_imported} loot rows) from "${summary.source_label}" into your database.`;
    await loadImportedSources();
    await loadDatabase();
  } catch (e) {
    errorMessage.value = `Import failed: ${e}`;
  } finally {
    importing.value = false;
  }
}

const scanning = ref(false);
const scanMessage = ref("");

async function doScan() {
  errorMessage.value = "";
  scanMessage.value = "";
  scanning.value = true;
  try {
    const filePath = await open({
      filters: [{ name: "Log", extensions: ["log"] }],
      multiple: false,
    });
    if (!filePath) return;
    const r = await invoke<IngestResult>("ingest_player_log", {
      playerLogPath: filePath as string,
    });
    if (r.already_ingested) {
      scanMessage.value = "Already scanned — no new data added.";
    } else {
      scanMessage.value = `Added ${r.kills_added} lootable kills and ${r.loot_added} loot entries.`;
    }
    await loadDatabase();
  } catch (e) {
    errorMessage.value = `Scan failed: ${e}`;
  } finally {
    scanning.value = false;
  }
}

async function removeSource(sourceLabel: string) {
  try {
    await invoke("delete_imported_source", { sourceLabel });
    await loadImportedSources();
  } catch (e) {
    console.error("[database-tab] Failed to remove import source:", e);
  }
}

function formatDate(isoStr: string): string {
  return formatDateTimeShort(isoStr);
}

onMounted(() => {
  loadDatabase();
  loadImportedSources();
});
</script>
