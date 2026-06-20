<template>
  <div class="flex flex-col gap-1.5">
    <div v-if="loading" class="text-xs text-text-dim italic">Loading…</div>
    <div v-else-if="sources.length === 0" class="text-xs text-text-dim italic">
      No drop sources for this scope.
    </div>
    <template v-else>
      <div v-for="row in sources" :key="row.enemy_name" class="flex flex-col gap-0.5">
        <div class="flex items-center justify-between text-xs">
          <EnemyInline :reference="row.enemy_name" />
          <span class="text-text-dim shrink-0">
            <span class="text-value-positive font-mono font-bold">x{{ row.total_quantity }}</span>
            <span class="text-[0.6rem]"> · {{ row.times_dropped }}/{{ row.total_kills }} kills</span>
          </span>
        </div>
        <div class="relative h-3 rounded bg-black/40 border border-border-default overflow-hidden">
          <div
            class="absolute inset-y-0 left-0 bg-[#3a5a3a] transition-[width] duration-300"
            :style="{ width: `${Math.min(100, Math.max(2, row.drop_rate * 100))}%` }" />
          <div class="relative z-10 flex items-center px-1.5 h-full text-[0.6rem] font-mono text-text-secondary">
            {{ fmtPct(row.drop_rate) }} drop rate
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DatabaseScope, ItemDropSource } from "../../types/farming";
import EnemyInline from "../Shared/Enemy/EnemyInline.vue";

const props = defineProps<{
  itemName: string;
  scope: DatabaseScope;
}>();

const loading = ref(false);
const sources = ref<ItemDropSource[]>([]);

async function load() {
  loading.value = true;
  try {
    sources.value = await invoke<ItemDropSource[]>("get_item_drop_sources", {
      itemName: props.itemName,
      internalName: null,
      scope: props.scope,
    });
  } catch (e) {
    console.error("[item-drop-breakdown-table] Failed to load sources:", e);
    sources.value = [];
  } finally {
    loading.value = false;
  }
}

function fmtPct(rate: number): string {
  const pct = rate * 100;
  return pct >= 10 ? `${pct.toFixed(0)}%` : `${pct.toFixed(1)}%`;
}

watch(() => [props.itemName, props.scope], load, { immediate: true });
</script>
