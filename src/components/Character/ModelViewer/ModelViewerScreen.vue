<template>
  <div class="flex flex-col gap-3 h-full overflow-hidden">
    <!-- Loading -->
    <div v-if="store.loading" class="flex items-center justify-center h-full text-text-muted">
      Loading model viewer…
    </div>

    <!-- Game install not found -->
    <div
      v-else-if="!store.gameFound"
      class="flex flex-col items-center justify-center h-full gap-2 text-center px-6">
      <div class="text-lg font-medium text-text-secondary">Project: Gorgon install not found</div>
      <p class="text-sm text-text-muted max-w-md">
        The Model Viewer extracts 3D models from your local game install. We
        couldn't auto-detect it via Steam. Make sure Project: Gorgon is
        installed, then reload.
      </p>
      <button class="btn-secondary mt-2" @click="store.refreshStatus()">Re-check</button>
    </div>

    <!-- Cache not built yet -->
    <div
      v-else-if="!store.cacheReady"
      class="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
      <div class="text-lg font-medium text-text-secondary">Extract 3D models</div>
      <p class="text-sm text-text-muted max-w-md">
        One-time extraction of gear models and textures from your local install
        into glogger's cache. This takes about a minute.
      </p>
      <button class="btn-primary" :disabled="store.extracting" @click="store.runExtraction()">
        {{ store.extracting ? "Extracting…" : "Extract models" }}
      </button>
      <div v-if="store.extracting" class="text-xs text-text-muted">
        {{ store.extractionMessage }}
      </div>
      <div v-if="store.error" class="text-xs text-accent-red max-w-md break-words">{{ store.error }}</div>
    </div>

    <!-- Main viewer -->
    <PaneLayout
      v-else
      screen-key="model-viewer"
      :left-pane="{ title: 'Items', fixedWidth: 300 }"
      :right-pane="{ title: 'Customize', defaultWidth: 320, minWidth: 240, maxWidth: 520 }">
      <!-- Left: items for the active slot -->
      <template #left>
        <div class="flex flex-col gap-2 h-full min-h-0 p-2">
          <!-- Sex toggle -->
          <div class="flex gap-1">
            <button
              v-for="opt in [{ v: 'm', l: 'Male' }, { v: 'f', l: 'Female' }]"
              :key="opt.v"
              class="flex-1 px-2 py-1 text-xs rounded border cursor-pointer"
              :class="store.sex === opt.v
                ? 'bg-accent-gold/20 border-accent-gold/40 text-accent-gold'
                : 'border-white/10 text-text-muted hover:bg-white/5'"
              @click="store.setSex(opt.v as 'm' | 'f')">
              {{ opt.l }}
            </button>
          </div>

          <div class="flex items-center justify-between">
            <span class="text-[11px] text-text-dim uppercase tracking-wide">
              {{ activeSlotLabel }} items
            </span>
            <span class="text-[10px] text-text-dim">{{ modeledCount }} with model</span>
          </div>

          <label class="flex items-center gap-1.5 text-[11px] text-text-muted cursor-pointer select-none">
            <input v-model="onlyWithModel" type="checkbox" class="cursor-pointer" />
            Only items with a 3D model
          </label>

          <!-- Search -->
          <input
            v-model="search"
            type="text"
            :placeholder="`Search ${activeSlotLabel}…`"
            class="input text-xs" />

          <!-- Item list (windowed: renders in chunks, grows on scroll) -->
          <div
            ref="listEl"
            class="flex-1 min-h-0 overflow-y-auto flex flex-col gap-0.5"
            @scroll="onListScroll">
            <button
              v-for="item in displayedItems"
              :key="item.id"
              class="flex items-center gap-2 px-2 py-1 rounded text-left text-xs cursor-pointer hover:bg-white/5"
              :class="[
                { 'bg-accent-gold/15': isChosen(item.id) },
                item.has_model ? '' : 'opacity-40',
              ]"
              :title="item.has_model ? item.name : `${item.name} — no 3D model`"
              @click="store.chooseItem(item)">
              <GameIcon :icon-id="item.icon_id ?? undefined" size="sm" />
              <span class="truncate">{{ item.name }}</span>
              <span v-if="!item.has_model" class="ml-auto text-[9px] text-text-dim shrink-0">no model</span>
            </button>
            <div v-if="store.itemsLoading" class="text-xs text-text-muted px-2 py-1">Loading…</div>
            <div
              v-else-if="matchedItems.length === 0"
              class="text-xs text-text-muted px-2 py-1">
              No {{ activeSlotLabel.toLowerCase() }} items.
            </div>
            <div
              v-else-if="displayedItems.length < matchedItems.length"
              class="text-[10px] text-text-dim px-2 py-1">
              Showing {{ displayedItems.length }} of {{ matchedItems.length }} — scroll for more
            </div>
          </div>
        </div>
      </template>

      <!-- Center: slot column + viewport (Item / Character) -->
      <div class="flex flex-col h-full min-h-0">
        <!-- Item / Character toggle (top of the model pane) -->
        <div class="flex gap-1 p-2">
          <button
            v-for="m in ([['item', 'Item'], ['character', 'Character']] as const)"
            :key="m[0]"
            class="px-3 py-1 text-xs rounded border cursor-pointer"
            :class="store.viewMode === m[0]
              ? 'bg-accent-gold/20 border-accent-gold/40 text-accent-gold'
              : 'border-white/10 text-text-muted hover:bg-white/5'"
            @click="store.setViewMode(m[0])">
            {{ m[1] }}
          </button>
        </div>

        <!-- Slot column (nested on the left, dropped down to align with the
             item list) + viewport -->
        <div class="flex flex-1 min-h-0">
          <div class="flex flex-col gap-1.5 pl-2 pr-1 pt-12 shrink-0">
            <button
              v-for="s in VIEWER_SLOTS"
              :key="s.id"
              class="relative w-[60px] h-[60px] rounded border flex items-center justify-center cursor-pointer transition-colors"
              :class="store.activeSlot === s.id
                ? 'border-accent-gold bg-accent-gold/10'
                : 'border-border-default hover:bg-white/5'"
              :title="s.label"
              @click="store.selectSlot(s.id)">
              <GameIcon
                v-if="store.loadout[s.id]"
                :icon-id="store.loadout[s.id].icon_id ?? undefined"
                size="lg" />
              <span v-else class="text-[9px] text-text-dim leading-tight text-center px-0.5">
                {{ s.label }}
              </span>
              <span
                v-if="store.loadout[s.id]"
                class="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-black/70 border border-border-default
                       text-[10px] leading-none flex items-center justify-center text-text-muted hover:text-accent-red"
                title="Clear slot"
                @click.stop="store.clearSlot(s.id)">
                ×
              </span>
            </button>
          </div>

          <div class="flex-1 min-h-0 relative">
            <TurntableViewer
              v-show="store.viewMode === 'item'"
              ref="turntable"
              class="absolute inset-0" />
            <div
              v-show="store.viewMode === 'character'"
              class="absolute inset-0 flex flex-col items-center justify-center gap-2 text-center px-6">
              <div class="text-sm text-text-secondary">Paper doll — coming soon</div>
              <p class="text-xs text-text-dim max-w-xs">
                This will render your character wearing the whole loadout you build
                slot by slot.
              </p>
            </div>
          </div>
        </div>
      </div>

      <!-- Right: customize + drop sources -->
      <template #right>
        <div class="flex flex-col h-full min-h-0">
          <!-- Top half: customization -->
          <div class="flex-1 min-h-0 overflow-y-auto p-2 flex flex-col gap-4">
            <div v-if="store.resolved">
              <div class="text-sm font-medium text-text-primary">{{ store.resolved.item_name }}</div>
              <div class="text-[10px] text-text-muted mt-0.5">
                {{ store.resolved.source_field }}
              </div>
            </div>
            <div v-else class="text-xs text-text-dim italic">Select an item for this slot.</div>
            <DyeControls @dye="onDye" />
          </div>

          <!-- Bottom half: where it drops from -->
          <div class="flex-1 min-h-0 overflow-y-auto p-2 border-t border-border-default flex flex-col gap-2">
            <div class="flex items-center justify-between">
              <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
                Drop Sources
              </span>
              <select v-model="dropScope" class="input text-[10px] py-0.5">
                <option value="combined">All data</option>
                <option value="mine">My kills</option>
                <option value="imported">Imported</option>
              </select>
            </div>
            <ItemDropBreakdownTable
              v-if="store.resolved"
              :item-name="store.resolved.item_name"
              :scope="dropScope" />
            <div v-else class="text-xs text-text-dim italic">Select an item.</div>
            <p class="text-[10px] text-text-dim mt-1 leading-snug">
              Observed from your kills and imported data — Project&nbsp;Gorgon ships no
              master loot table, so this reflects what's been seen.
            </p>
          </div>
        </div>
      </template>
    </PaneLayout>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from "vue";
import { useModelViewerStore } from "../../../stores/modelViewerStore";
import { VIEWER_SLOTS } from "../../../types/modelViewer";
import type { DatabaseScope } from "../../../types/farming";
import PaneLayout from "../../Shared/PaneLayout.vue";
import GameIcon from "../../Shared/GameIcon.vue";
import TurntableViewer from "./TurntableViewer.vue";
import DyeControls from "./DyeControls.vue";
import ItemDropBreakdownTable from "../../Farming/ItemDropBreakdownTable.vue";

const store = useModelViewerStore();
const turntable = ref<InstanceType<typeof TurntableViewer> | null>(null);
const search = ref("");
const dropScope = ref<DatabaseScope>("combined");
// Many cosmetics (holiday hats, masks) have no 3D mesh in the game; hide them
// by default so the list only shows items that actually render.
const onlyWithModel = ref(true);
const modeledCount = computed(() => store.items.filter((i) => i.has_model).length);

const activeSlotLabel = computed(
  () => VIEWER_SLOTS.find((s) => s.id === store.activeSlot)?.label ?? store.activeSlot,
);

function isChosen(id: number): boolean {
  return store.loadout[store.activeSlot]?.ref === String(id);
}

// Windowed list: render `visibleCount` rows and grow on scroll, so a large
// slot's items are browsable without rendering every row (and icon) up front.
const listEl = ref<HTMLElement | null>(null);
const CHUNK = 200;
const visibleCount = ref(CHUNK);

const matchedItems = computed(() => {
  const q = search.value.trim().toLowerCase();
  return store.items.filter(
    (i) =>
      (!onlyWithModel.value || i.has_model) &&
      (!q || i.name.toLowerCase().includes(q)),
  );
});
const displayedItems = computed(() => matchedItems.value.slice(0, visibleCount.value));

// Reset the window when the underlying list, query, or filter changes.
watch([search, () => store.items, onlyWithModel], () => {
  visibleCount.value = CHUNK;
  if (listEl.value) listEl.value.scrollTop = 0;
});

function onListScroll() {
  const el = listEl.value;
  if (!el) return;
  if (
    el.scrollTop + el.clientHeight >= el.scrollHeight - 240 &&
    visibleCount.value < matchedItems.value.length
  ) {
    visibleCount.value = Math.min(visibleCount.value + CHUNK, matchedItems.value.length);
  }
}

function onDye(slot: string, channel: number, hex: string) {
  turntable.value?.setDye(slot, channel, hex);
}

onMounted(async () => {
  await store.init();
  if (store.cacheReady) await store.selectSlot(store.activeSlot);
});
</script>
