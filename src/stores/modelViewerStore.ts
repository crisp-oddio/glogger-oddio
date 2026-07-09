import { defineStore, acceptHMRUpdate } from "pinia";
import { ref, computed } from "vue";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ModelViewerStatus,
  ResolvedItemAppearance,
  ResolvedSlot,
  BrowsableItem,
  ExtractionProgress,
  LoadoutEntry,
} from "../types/modelViewer";

export const useModelViewerStore = defineStore("modelViewer", () => {
  const status = ref<ModelViewerStatus | null>(null);
  const cacheRoot = ref<string>("");
  const loading = ref(false);
  const error = ref<string | null>(null);

  const sex = ref<"m" | "f">("m");
  const items = ref<BrowsableItem[]>([]);
  const itemsLoading = ref(false);
  const selectedRef = ref<string | null>(null);
  const resolved = ref<ResolvedItemAppearance | null>(null);
  const resolving = ref(false);

  // Loadout: which equipment slot is being browsed, and the item chosen per
  // slot. The slot column fills in as items are chosen; the "Character" view
  // renders the whole loadout on the base body.
  const activeSlot = ref<string>("Chest");
  const loadout = ref<Record<string, LoadoutEntry>>({});
  const viewMode = ref<"item" | "character">("item");

  // Paper-doll data (Character mode): the naked base body for the current sex,
  // plus the resolved appearance of every equipped slot (so the whole loadout
  // can be assembled at once, not just the active item).
  const baseBody = ref<ResolvedSlot[]>([]);
  const resolvedLoadout = ref<Record<string, ResolvedItemAppearance>>({});

  // Extraction
  const extracting = ref(false);
  const extractionMessage = ref<string>("");
  let unlistenProgress: UnlistenFn | null = null;

  const cacheReady = computed(() => status.value?.cache_ready ?? false);
  const gameFound = computed(() => !!status.value?.game_bundles_dir);

  /** Build an asset:// URL for a cache-relative path (e.g. "meshes/x.glb"). */
  function assetUrl(relPath: string): string {
    const root = cacheRoot.value.replace(/[\\/]+$/, "");
    return convertFileSrc(`${root}/${relPath}`);
  }

  async function init() {
    loading.value = true;
    error.value = null;
    try {
      status.value = await invoke<ModelViewerStatus>("model_viewer_status", {
        gameDir: null,
      });
      cacheRoot.value = await invoke<string>("model_cache_root");
      if (!unlistenProgress) {
        unlistenProgress = await listen<ExtractionProgress>(
          "model-extraction-progress",
          (e) => {
            extractionMessage.value = e.payload.message;
            if (e.payload.done) {
              extracting.value = false;
              if (e.payload.ok) refreshStatus().then(fetchBaseBody);
            }
          },
        );
      }
      // Once the cache is ready, load the paper-doll base body for Character mode.
      await fetchBaseBody();
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function refreshStatus() {
    status.value = await invoke<ModelViewerStatus>("model_viewer_status", {
      gameDir: null,
    });
  }

  async function runExtraction() {
    extracting.value = true;
    extractionMessage.value = "Starting extraction…";
    error.value = null;
    try {
      await invoke<string>("start_model_extraction", { gameDir: null });
    } catch (e) {
      error.value = String(e);
      extracting.value = false;
    }
  }

  /** Fetch the naked base body (mesh + skin per region) for the current sex. */
  async function fetchBaseBody() {
    if (!cacheReady.value) return;
    try {
      baseBody.value = await invoke<ResolvedSlot[]>("resolve_base_body", {
        sex: sex.value,
      });
    } catch (e) {
      error.value = String(e);
      baseBody.value = [];
    }
  }

  /** Resolve a single item reference to its full appearance (no side effects). */
  async function resolveRef(reference: string): Promise<ResolvedItemAppearance | null> {
    return await invoke<ResolvedItemAppearance | null>("resolve_item_appearance", {
      reference,
      sex: sex.value,
    });
  }

  async function loadItems(slot?: string) {
    itemsLoading.value = true;
    try {
      items.value = await invoke<BrowsableItem[]>("list_appearance_items", {
        slot: slot ?? null,
      });
    } catch (e) {
      error.value = String(e);
    } finally {
      itemsLoading.value = false;
    }
  }

  async function selectItem(reference: string) {
    selectedRef.value = reference;
    await resolveCurrent();
  }

  /** Switch the browsed slot: load its items and view the item chosen for it. */
  async function selectSlot(slot: string) {
    activeSlot.value = slot;
    await loadItems(slot);
    const cur = loadout.value[slot];
    if (cur) {
      selectedRef.value = cur.ref;
      await resolveCurrent();
    } else {
      selectedRef.value = null;
      resolved.value = null;
    }
  }

  /** Choose an item for the active slot (records it in the loadout + views it). */
  async function chooseItem(item: BrowsableItem) {
    const slot = activeSlot.value;
    loadout.value[slot] = {
      ref: String(item.id),
      name: item.name,
      icon_id: item.icon_id,
    };
    selectedRef.value = String(item.id);
    await resolveCurrent();
    // Cache the resolved appearance so Character mode can assemble the whole
    // loadout without re-resolving each slot on every rebuild.
    if (resolved.value) resolvedLoadout.value[slot] = resolved.value;
    else delete resolvedLoadout.value[slot];
  }

  function clearSlot(slot: string) {
    delete loadout.value[slot];
    delete resolvedLoadout.value[slot];
    if (activeSlot.value === slot) {
      selectedRef.value = null;
      resolved.value = null;
    }
  }

  function setViewMode(mode: "item" | "character") {
    viewMode.value = mode;
  }

  async function setSex(next: "m" | "f") {
    if (sex.value === next) return;
    sex.value = next;
    // Every mesh key is sex-specific: re-resolve the base body and the whole
    // loadout (and the active item) so the paper doll rebuilds for the new sex.
    await fetchBaseBody();
    for (const [slot, entry] of Object.entries(loadout.value)) {
      const r = await resolveRef(entry.ref);
      if (r) resolvedLoadout.value[slot] = r;
      else delete resolvedLoadout.value[slot];
    }
    if (selectedRef.value) await resolveCurrent();
  }

  async function resolveCurrent() {
    if (!selectedRef.value) return;
    resolving.value = true;
    try {
      resolved.value = await resolveRef(selectedRef.value);
    } catch (e) {
      error.value = String(e);
      resolved.value = null;
    } finally {
      resolving.value = false;
    }
  }

  return {
    status,
    cacheRoot,
    loading,
    error,
    sex,
    items,
    itemsLoading,
    selectedRef,
    resolved,
    resolving,
    activeSlot,
    loadout,
    viewMode,
    baseBody,
    resolvedLoadout,
    extracting,
    extractionMessage,
    cacheReady,
    gameFound,
    assetUrl,
    init,
    refreshStatus,
    runExtraction,
    fetchBaseBody,
    loadItems,
    selectItem,
    selectSlot,
    chooseItem,
    clearSlot,
    setViewMode,
    setSex,
    resolveCurrent,
  };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useModelViewerStore, import.meta.hot));
}
