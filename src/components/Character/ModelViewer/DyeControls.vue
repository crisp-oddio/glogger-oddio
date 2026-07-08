<template>
  <div class="flex flex-col gap-3">
    <div
      v-for="slot in dyeableSlots"
      :key="slot.slot"
      class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-text-secondary">{{ slot.slot }}</span>
        <span class="text-[10px] text-text-muted uppercase tracking-wide">
          {{ channelLabel(slot.dye_channels) }} dye
        </span>
      </div>
      <div class="flex gap-2">
        <label
          v-for="ch in slot.dye_channels"
          :key="ch"
          class="flex flex-col items-center gap-1">
          <input
            type="color"
            class="w-9 h-9 rounded cursor-pointer border border-white/15 bg-transparent p-0"
            :value="hexFor(slot.slot, ch)"
            @input="onInput(slot.slot, ch, $event)" />
          <span class="text-[10px] text-text-muted">{{ ch }}</span>
        </label>
      </div>
    </div>

    <div v-if="dyeableSlots.length === 0" class="text-xs text-text-muted">
      {{ notDyeable ? "This item is not dyeable." : "No dye channels." }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import { useModelViewerStore } from "../../../stores/modelViewerStore";
import type { ResolvedSlot } from "../../../types/modelViewer";

const store = useModelViewerStore();

const emit = defineEmits<{
  (e: "dye", slot: string, channel: number, hex: string): void;
}>();

// Current hex per "slot:channel" (seeded from material defaults, then user edits).
const current = reactive<Record<string, string>>({});

const dyeableSlots = computed<ResolvedSlot[]>(() =>
  (store.resolved?.slots ?? []).filter((s) => s.dyeable && s.dye_channels > 0),
);

const notDyeable = computed(
  () => store.resolved?.not_dyeable_keyword ?? false,
);

function channelLabel(n: number): string {
  return n === 1 ? "Single" : n === 2 ? "Double" : n === 3 ? "Triple" : `${n}`;
}

function defaultHex(slot: ResolvedSlot, channel: number): string {
  const c = slot.default_colors?.[`_Color${channel}`];
  if (!c) return "#808080";
  const to = (v: number) =>
    Math.max(0, Math.min(255, Math.round(v * 255)))
      .toString(16)
      .padStart(2, "0");
  return `#${to(c[0])}${to(c[1])}${to(c[2])}`;
}

function hexFor(slot: string, channel: number): string {
  const key = `${slot}:${channel}`;
  return current[key] ?? "#808080";
}

function onInput(slot: string, channel: number, ev: Event) {
  const hex = (ev.target as HTMLInputElement).value;
  current[`${slot}:${channel}`] = hex;
  emit("dye", slot, channel, hex);
}

// Re-seed defaults whenever a new item resolves.
watch(
  dyeableSlots,
  (slots) => {
    for (const s of slots) {
      for (let ch = 1; ch <= s.dye_channels; ch++) {
        current[`${s.slot}:${ch}`] = defaultHex(s, ch);
      }
    }
  },
  { immediate: true },
);
</script>
