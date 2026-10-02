import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { acceptHMRUpdate } from 'pinia'
import type { StallPriceObservation, StallPriceEntry } from '../types/stallPrices'

/**
 * Stall price capture state — pricing observations for OTHER players' stalls.
 *
 * The store owns the observation list plus the live capture flow: when the
 * coordinator detects a `ProcessPlayerVendorScreen(...)` for someone else's
 * stall it emits `stall-browse-started`, which raises `pendingCapture` and
 * the Market tab's capture panel. Rows refresh on `stall-prices-updated`
 * (debounced 500ms — the coordinator emits once per batch).
 */
export const useStallPriceStore = defineStore('stallPrice', () => {
  const observations = ref<StallPriceObservation[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  /** Active stall-browse session awaiting capture, or null. */
  const pendingCapture = ref<{ npcEntityId: number; startedAt: string } | null>(null)

  /** Observations indexed by item_type_id for O(1) tooltip lookup. */
  const observationsByItemId = computed(() => {
    const map: Record<number, StallPriceObservation> = {}
    for (const o of observations.value) {
      if (o.item_type_id != null && map[o.item_type_id] == null) {
        // Newest first (query is ordered DESC) — keep the most recent sighting.
        map[o.item_type_id] = o
      }
    }
    return map
  })

  async function loadObservations(): Promise<void> {
    loading.value = true
    error.value = null
    try {
      observations.value = await invoke<StallPriceObservation[]>(
        'get_stall_price_observations',
        {},
      )
    } catch (e) {
      error.value = String(e)
      console.error('[stallPriceStore] Failed to load observations:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * Save capture-panel lines. Returns the number of rows actually inserted
   * (duplicates are swallowed server-side by INSERT OR IGNORE).
   */
  async function recordPrices(entries: StallPriceEntry[]): Promise<number> {
    const inserted = await invoke<number>('record_stall_prices', { entries })
    await loadObservations()
    return inserted
  }

  /** Fill in the price of a purchase-sentinel row (`price_unit = 0`). */
  async function updateObservation(
    id: number,
    priceUnit: number,
    stallLabel?: string,
    ownerName?: string,
  ): Promise<void> {
    await invoke('update_stall_price_observation', {
      id,
      priceUnit,
      stallLabel: stallLabel ?? null,
      ownerName: ownerName ?? null,
    })
    await loadObservations()
  }

  async function deleteObservation(id: number): Promise<void> {
    await invoke('delete_stall_price_observation', { id })
    observations.value = observations.value.filter((o) => o.id !== id)
  }

  /** Clear a dismissed capture prompt (frontend-local; no backend state). */
  function dismissCapture(): void {
    pendingCapture.value = null
  }

  // Coordinator side-channel: browsing someone else's stall raises the
  // capture panel. Only fires when the coordinator sees the vendor screen
  // event, so the panel appears in-context.
  void listen<{ npc_entity_id: number; timestamp: string }>('stall-browse-started', (event) => {
    pendingCapture.value = {
      npcEntityId: event.payload.npc_entity_id,
      startedAt: event.payload.timestamp,
    }
  })

  let pricesTimer: number | undefined = undefined
  void listen<number>('stall-prices-updated', () => {
    clearTimeout(pricesTimer)
    pricesTimer = setTimeout(() => {
      void loadObservations()
    }, 500)
  })

  return {
    observations,
    loading,
    error,
    pendingCapture,
    observationsByItemId,
    loadObservations,
    recordPrices,
    updateObservation,
    deleteObservation,
    dismissCapture,
  }
})

// Enable proper Pinia hot-module replacement: without this, editing a store
// action during `tauri dev` leaves the already-instantiated singleton running
// the old code.
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useStallPriceStore, import.meta.hot))
}
