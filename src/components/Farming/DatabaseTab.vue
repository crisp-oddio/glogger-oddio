<template>
  <div class="flex flex-col gap-4 h-full overflow-y-auto">
    <!-- Scope + import/export toolbar -->
    <div class="flex items-center justify-between gap-3 flex-wrap">
      <div class="flex items-center gap-1 bg-surface-dark border border-border-default rounded-lg p-1">
        <button
          v-for="opt in scopeOptions"
          :key="opt.value"
          @click="scope = opt.value"
          :class="[
            'px-3 py-1.5 text-xs rounded font-medium transition-colors cursor-pointer',
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

    <!-- Search -->
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
      <input
        v-model="query"
        type="text"
        :placeholder="searchTarget === 'enemies' ? 'Search monsters…' : 'Search items…'"
        class="flex-1 px-3 py-2 text-sm bg-surface-card border border-border-light rounded text-text-primary placeholder-text-dim outline-none focus:border-entity-item"
      />
    </div>

    <!-- Results -->
    <div class="flex-1 min-h-0 overflow-y-auto">
      <EmptyState
        v-if="!searching && results.length === 0 && query.trim().length > 0"
        variant="compact"
        primary="No matches"
        secondary="Try a different search term or scope." />
      <EmptyState
        v-else-if="query.trim().length === 0"
        variant="compact"
        primary="Search for a monster or item"
        secondary="Results show lifetime kill/loot counts and drop rates for the selected scope." />

      <div v-else class="flex flex-col gap-1">
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
        <template v-else>
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
      </div>
    </div>

    <!-- Imported sources management -->
    <div v-if="importedSources.length > 0" class="bg-surface-dark border border-border-default rounded-lg p-3 max-h-40 overflow-y-auto shrink-0">
      <div class="text-[0.65rem] uppercase tracking-widest text-text-dim font-bold mb-2">Imported Sources</div>
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
  ImportSummary,
  ImportedSource,
} from "../../types/farming";
import EmptyState from "../Shared/EmptyState.vue";
import ItemInline from "../Shared/Item/ItemInline.vue";
import EnemyInline from "../Shared/Enemy/EnemyInline.vue";
import EnemyDropTable from "./EnemyDropTable.vue";
import ItemDropBreakdownTable from "./ItemDropBreakdownTable.vue";
import { formatDateTimeShort } from "../../composables/useTimestamp";

const scope = ref<DatabaseScope>("combined");
const scopeOptions: Array<{ value: DatabaseScope; label: string }> = [
  { value: "mine", label: "My Data" },
  { value: "imported", label: "Imported" },
  { value: "combined", label: "Combined" },
];

const searchTarget = ref<"enemies" | "items">("enemies");
const searchTargetOptions: Array<{ value: "enemies" | "items"; label: string }> = [
  { value: "enemies", label: "Monsters" },
  { value: "items", label: "Items" },
];

const query = ref("");
const searching = ref(false);
const enemyResults = ref<EnemySearchResult[]>([]);
const itemResults = ref<ItemSearchResult[]>([]);
const results = computed(() => (searchTarget.value === "enemies" ? enemyResults.value : itemResults.value));

const expandedEnemies = ref<Set<string>>(new Set());
const expandedItems = ref<Set<string>>(new Set());

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

let searchDebounce: ReturnType<typeof setTimeout> | null = null;
async function runSearch() {
  const q = query.value.trim();
  if (!q) {
    enemyResults.value = [];
    itemResults.value = [];
    return;
  }
  searching.value = true;
  try {
    if (searchTarget.value === "enemies") {
      enemyResults.value = await invoke<EnemySearchResult[]>("search_database_enemies", {
        query: q,
        scope: scope.value,
        limit: 50,
      });
    } else {
      itemResults.value = await invoke<ItemSearchResult[]>("search_database_items", {
        query: q,
        scope: scope.value,
        limit: 50,
      });
    }
  } catch (e) {
    console.error("[database-tab] Search failed:", e);
  } finally {
    searching.value = false;
  }
}

watch([query, searchTarget, scope], () => {
  if (searchDebounce) clearTimeout(searchDebounce);
  searchDebounce = setTimeout(runSearch, 250);
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
    importMessage.value = `Imported ${summary.enemies_imported} enemies (${summary.loot_rows_imported} loot rows) from "${summary.source_label}".`;
    await loadImportedSources();
    if (query.value.trim()) await runSearch();
  } catch (e) {
    errorMessage.value = `Import failed: ${e}`;
  } finally {
    importing.value = false;
  }
}

async function removeSource(sourceLabel: string) {
  try {
    await invoke("delete_imported_source", { sourceLabel });
    await loadImportedSources();
    if (query.value.trim()) await runSearch();
  } catch (e) {
    console.error("[database-tab] Failed to remove import source:", e);
  }
}

function formatDate(isoStr: string): string {
  return formatDateTimeShort(isoStr);
}

onMounted(loadImportedSources);
</script>
