import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { acceptHMRUpdate } from 'pinia'
import type { OcrStatus } from '../types/stallPrices'

/**
 * Tesseract OCR sidecar status for the stall price scanner (Phase B).
 *
 * The Rust side manages the binary in appdata (`tesseract/`); this store only
 * caches whether it's installed so the capture panel can label its button
 * "Install OCR engine…" until `ocr_check_status` says otherwise.
 */
export const useOcrStore = defineStore('ocr', () => {
  const installed = ref(false)
  const checked = ref(false)
  const downloading = ref(false)

  async function checkStatus(): Promise<void> {
    try {
      const status = await invoke<OcrStatus>('ocr_check_status')
      installed.value = status.installed
      checked.value = true
    } catch (e) {
      console.error('[ocrStore] Failed to check OCR status:', e)
      installed.value = false
      checked.value = true
    }
  }

  /** Download the portable Tesseract build into appdata. */
  async function download(): Promise<void> {
    if (downloading.value) return
    downloading.value = true
    try {
      await invoke<OcrStatus>('ocr_download')
      await checkStatus()
    } finally {
      downloading.value = false
    }
  }

  return { installed, checked, downloading, checkStatus, download }
})

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useOcrStore, import.meta.hot))
}
