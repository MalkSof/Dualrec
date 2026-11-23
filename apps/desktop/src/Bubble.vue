<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { api } from "./api";
import { t } from "./i18n";

// Burbuja flotante: se arrastra con el mouse y cambia de tamaño con doble clic.
const SIZES = [160, 220, 300];
const url = ref("");
let size = 1;

onMounted(async () => {
  url.value = await api.previewUrl();
});

function drag(e: MouseEvent) {
  if (e.button === 0 && e.detail === 1) void getCurrentWindow().startDragging();
}

function cycleSize() {
  size = (size + 1) % SIZES.length;
  void getCurrentWindow().setSize(new LogicalSize(SIZES[size], SIZES[size]));
}
</script>

<template>
  <div class="bubble-wrap">
    <div class="bubble-cam" :title="t('bubbleTitle')">
      <img v-if="url" :src="url" alt="" />
      <div class="drag" @mousedown="drag" @dblclick="cycleSize" />
    </div>
  </div>
</template>
