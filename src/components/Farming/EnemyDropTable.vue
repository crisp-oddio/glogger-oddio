<template>
  <div class="flex flex-col gap-1.5">
    <div v-if="loading" class="text-xs text-text-dim italic">Loading…</div>
    <div v-else-if="stats === null || stats.loot.length === 0" class="text-xs text-text-dim italic">
      No loot data for this scope.
    </div>
    <template v-else>
      <div v-for="row in stats.loot" :key="row.item_name" class="flex flex-col gap-0.5">
        <div class="flex items-center justify-between text-xs">
          <ItemInline :reference="row.item_name" />
          <span class="text-text-dim shrink-0">
            <span class="text-value-positive font-mono font-bold">x{{ row.total_quantity }}</span>
            <span class="text-[0.6rem]"> · {{ row.times_dropped }}/{{ stats!.total_kills }} kills</span>
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
import type { DatabaseScope, EnemyKillStats } from "../../types/farming";
import ItemInline from "../Shared/Item/ItemInline.vue";

const props = defineProps<{
  enemyName: string;
  scope: DatabaseScope;
  // Area key to scope stats to. null = the unknown-zone bucket; passed to the
  // backend as "" (absent/undefined there would mean "all zones").
  zone?: string | null;
  // Equipped combat-skill loadout to filter by (null = all loadouts).
  combatSkills?: string | null;
}>();

const loading = ref(false);
const stats = ref<EnemyKillStats | null>(null);

async function load() {
  loading.value = true;
  try {
    stats.value = await invoke<EnemyKillStats>("get_enemy_kill_stats", {
      enemyName: props.enemyName,
      scope: props.scope,
      zone: props.zone ?? "",
      combatSkills: props.combatSkills ?? null,
    });
  } catch (e) {
    console.error("[enemy-drop-table] Failed to load stats:", e);
    stats.value = null;
  } finally {
    loading.value = false;
  }
}

function fmtPct(rate: number): string {
  const pct = rate * 100;
  return pct >= 10 ? `${pct.toFixed(0)}%` : `${pct.toFixed(1)}%`;
}

watch(() => [props.enemyName, props.scope, props.zone, props.combatSkills], load, { immediate: true });
</script>
