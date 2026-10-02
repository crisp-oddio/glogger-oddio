// TypeScript shapes that mirror the Rust serde structs in
// src-tauri/src/db/stall_commands.rs.
//
// Naming convention: response types use snake_case (the Rust structs have no
// `rename_all` attribute — StallPriceObservation, StallScanResult, ...).

export interface StallPriceObservation {
  id: number
  character_name: string
  server_name: string
  item_name: string
  internal_name: string | null
  item_type_id: number | null
  quantity: number
  /** Councils per unit. `0` = unknown (purchase pending a manual price). */
  price_unit: number
  stall_npc_entity_id: number
  stall_label: string
  owner_name: string | null
  /** 'capture' (typed/OCR'd) or 'purchase' (auto-detected). */
  source: string
  observed_at: string
  notes: string | null
}

/** One quick-entry line from the capture panel (invoke args, camelCase). */
export interface StallPriceEntry {
  itemQuery: string
  quantity: number
  priceUnit: number
  stallNpcEntityId: number
  stallLabel: string | null
  ownerName: string | null
  notes: string | null
}

// ── Phase B: OCR stall scan ─────────────────────────────────────────────────
// Mirrors src-tauri/src/stall_ocr.rs StallScanResult / StallScanRow.

export interface StallScanRow {
  raw_text: string
  item_name: string | null
  price: number | null
  quantity: number | null
  confidence: number
}

export interface StallScanResult {
  stall_name: string | null
  stall_slot: string | null
  owner_name: string | null
  rows: StallScanRow[]
}

export interface OcrStatus {
  installed: boolean
  exe_path: string | null
  version: string | null
}
