<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { api, ApiError, formatClock, formatDate, formatSize, type SharedVideo, type Track } from "../api";

const props = defineProps<{ token: string }>();

const video = ref<SharedVideo | null>(null);
const notFound = ref(false);
const loadError = ref("");

const stage = ref<HTMLElement | null>(null);
const screenEl = ref<HTMLVideoElement | null>(null);
const camEl = ref<HTMLVideoElement | null>(null);
const micEl = ref<HTMLAudioElement | null>(null);
const sysEl = ref<HTMLAudioElement | null>(null);

const playing = ref(false);
const buffering = ref(false);
const current = ref(0);
const rate = ref(1);
const showCam = ref(true);
const micOn = ref(true);
const sysOn = ref(true);
const corner = ref(0);
const CORNERS = ["bl", "br", "tr", "tl"];

const track = (kind: Track["kind"]) => video.value?.tracks.find((t) => t.kind === kind && t.url) ?? null;
const screen = computed(() => track("screen"));
const camera = computed(() => track("camera"));
const mic = computed(() => track("mic"));
const system = computed(() => track("system"));
const ready = computed(() => video.value?.status === "ready");
// Sin pantalla, la cámara pasa a ser el video principal.
const camIsMain = computed(() => !screen.value && !!camera.value);
const duration = computed(() => (video.value?.duration_ms ?? 0) / 1000);

/** El elemento que marca el tiempo; el resto lo sigue. */
function master(): HTMLMediaElement | null {
  return screenEl.value ?? camEl.value ?? micEl.value ?? sysEl.value;
}

function followers(): HTMLMediaElement[] {
  const m = master();
  return [screenEl.value, camEl.value, micEl.value, sysEl.value].filter(
    (e): e is HTMLVideoElement | HTMLAudioElement => !!e && e !== m,
  );
}

function align(force = false) {
  const m = master();
  if (!m) return;
  for (const f of followers()) {
    // Más de 120 ms de diferencia se nota: se reacomoda la pista.
    if (force || Math.abs(f.currentTime - m.currentTime) > 0.12) f.currentTime = m.currentTime;
    if (!m.paused && f.paused && !f.ended) void f.play().catch(() => {});
    if (m.paused && !f.paused) f.pause();
  }
}

async function play() {
  const m = master();
  if (!m) return;
  if (m.ended || (duration.value && m.currentTime >= duration.value - 0.05)) seek(0);
  try {
    await m.play();
    align(true);
  } catch {
    /* el navegador bloqueó la reproducción: el usuario vuelve a tocar play */
  }
}

function pause() {
  master()?.pause();
  followers().forEach((f) => f.pause());
}

function toggle() {
  if (master()?.paused === false) pause();
  else void play();
}

function seek(seconds: number) {
  const m = master();
  if (!m) return;
  const t = Math.min(Math.max(0, seconds), duration.value || m.duration || 0);
  m.currentTime = t;
  current.value = t;
  followers().forEach((f) => (f.currentTime = t));
}

function onTime() {
  const m = master();
  if (!m) return;
  current.value = m.currentTime;
  align();
}

function onWaiting() {
  buffering.value = true;
  followers().forEach((f) => f.pause());
}

function onPlaying() {
  buffering.value = false;
  playing.value = true;
  align(true);
}

function fullscreen() {
  if (document.fullscreenElement) void document.exitFullscreen();
  else void stage.value?.requestFullscreen();
}

function onKey(e: KeyboardEvent) {
  if (!ready.value || (e.target as HTMLElement).closest("input, select, button, a")) return;
  if (e.key === " " || e.key === "k") toggle();
  else if (e.key === "ArrowRight") seek(current.value + 5);
  else if (e.key === "ArrowLeft") seek(current.value - 5);
  else if (e.key === "f") fullscreen();
  else return;
  e.preventDefault();
}

watch(rate, (r) => [master(), ...followers()].forEach((e) => e && (e.playbackRate = r)));
watch(micOn, (on) => micEl.value && (micEl.value.muted = !on));
watch(sysOn, (on) => sysEl.value && (sysEl.value.muted = !on));

let poll = 0;

async function load() {
  try {
    video.value = await api.shared(props.token);
    loadError.value = "";
    document.title = `${video.value.title} · Dualrec`;
    // Mientras sube o se procesa, se vuelve a consultar hasta que esté listo.
    if (video.value.status === "ready" || video.value.status === "failed") clearInterval(poll);
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) {
      notFound.value = true;
      clearInterval(poll);
    } else {
      loadError.value = e instanceof Error ? e.message : String(e);
    }
  }
}

onMounted(() => {
  void load();
  poll = window.setInterval(load, 3000);
  window.addEventListener("keydown", onKey);
});

onBeforeUnmount(() => {
  clearInterval(poll);
  window.removeEventListener("keydown", onKey);
});

const waitingText = computed(() => {
  switch (video.value?.status) {
    case "recording":
      return ["Se está grabando", "El video va a aparecer acá cuando termine la grabación."];
    case "uploading":
      return ["Se está subiendo", "Falta poco: el video se está terminando de subir."];
    case "processing":
      return ["Procesando el video", "En unos segundos va a estar listo para ver."];
    default:
      return ["No se pudo procesar este video", "Avisale a quien te lo compartió."];
  }
});

const TRACK_NAME: Record<string, string> = {
  screen: "Pantalla",
  camera: "Cámara",
  mic: "Micrófono",
  system: "Audio del sistema",
  composite: "Video completo",
};
</script>

<template>
  <main class="watch">
    <section v-if="notFound" class="notice">
      <h1>Este video no existe o ya no está compartido</h1>
      <p>Pedile un link nuevo a quien te lo envió.</p>
    </section>

    <section v-else-if="!video" class="notice">
      <div v-if="!loadError" class="spinner" aria-label="Cargando" />
      <template v-else>
        <h1>No se pudo cargar el video</h1>
        <p>{{ loadError }}</p>
      </template>
    </section>

    <template v-else>
      <div ref="stage" class="player" :class="{ idle: !playing }">
        <div v-if="!ready" class="notice inside">
          <div v-if="video.status !== 'failed'" class="spinner" />
          <h1>{{ waitingText[0] }}</h1>
          <p>{{ waitingText[1] }}</p>
        </div>

        <template v-else>
          <video
            v-if="screen"
            ref="screenEl"
            class="main"
            :src="screen.url!"
            muted
            playsinline
            preload="auto"
            @click="toggle"
            @timeupdate="onTime"
            @waiting="onWaiting"
            @playing="onPlaying"
            @pause="playing = false"
            @ended="pause"
          />
          <video
            v-if="camera"
            v-show="camIsMain || showCam"
            ref="camEl"
            :class="camIsMain ? 'main' : ['cam', CORNERS[corner]]"
            :src="camera.url!"
            muted
            playsinline
            preload="auto"
            :title="camIsMain ? undefined : 'Clic para mover la cámara de esquina'"
            @click="camIsMain ? toggle() : (corner = (corner + 1) % 4)"
            @timeupdate="camIsMain && onTime()"
            @waiting="camIsMain && onWaiting()"
            @playing="camIsMain && onPlaying()"
            @pause="camIsMain && (playing = false)"
            @ended="camIsMain && pause()"
          />
          <div v-if="!screen && !camera" class="audio-only" @click="toggle">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3" /></svg>
            Grabación de audio
          </div>
          <audio
            v-if="mic"
            ref="micEl"
            :src="mic.url!"
            preload="auto"
            @timeupdate="!screen && !camera && onTime()"
            @playing="!screen && !camera && onPlaying()"
            @pause="!screen && !camera && (playing = false)"
            @ended="!screen && !camera && pause()"
          />
          <audio
            v-if="system"
            ref="sysEl"
            :src="system.url!"
            preload="auto"
            @timeupdate="!screen && !camera && !mic && onTime()"
            @playing="!screen && !camera && !mic && onPlaying()"
            @pause="!screen && !camera && !mic && (playing = false)"
            @ended="!screen && !camera && !mic && pause()"
          />

          <button v-if="!playing && !buffering" class="big-play" aria-label="Reproducir" @click="toggle">
            <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5Z" /></svg>
          </button>
          <div v-if="buffering" class="spinner over" />

          <div class="controls">
            <input
              class="seek"
              type="range"
              min="0"
              :max="duration"
              step="0.05"
              :value="current"
              aria-label="Posición"
              :style="{ '--p': `${duration ? (current / duration) * 100 : 0}%` }"
              @input="seek(Number(($event.target as HTMLInputElement).value))"
            />
            <div class="bar">
              <button class="ctl" :aria-label="playing ? 'Pausar' : 'Reproducir'" @click="toggle">
                <svg v-if="playing" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1" /><rect x="14" y="5" width="4" height="14" rx="1" /></svg>
                <svg v-else viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7L8 5Z" /></svg>
              </button>
              <span class="time">{{ formatClock(current * 1000) }} / {{ formatClock(video.duration_ms) }}</span>
              <span class="grow" />
              <button v-if="camera && screen" class="chip" :class="{ on: showCam }" :aria-pressed="showCam" @click="showCam = !showCam">Cámara</button>
              <button v-if="mic" class="chip" :class="{ on: micOn }" :aria-pressed="micOn" @click="micOn = !micOn">Micrófono</button>
              <button v-if="system" class="chip" :class="{ on: sysOn }" :aria-pressed="sysOn" @click="sysOn = !sysOn">Sistema</button>
              <select v-model.number="rate" class="rate" aria-label="Velocidad">
                <option :value="1">1×</option>
                <option :value="1.25">1,25×</option>
                <option :value="1.5">1,5×</option>
                <option :value="2">2×</option>
              </select>
              <button class="ctl" aria-label="Pantalla completa" @click="fullscreen">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" /></svg>
              </button>
            </div>
          </div>
        </template>
      </div>

      <div class="meta">
        <h1>{{ video.title }}</h1>
        <p>
          <template v-if="video.author">{{ video.author }} · </template>{{ formatDate(video.recorded_at) }}
          <template v-if="video.duration_ms"> · {{ formatClock(video.duration_ms) }}</template>
        </p>
      </div>

      <section v-if="ready" class="downloads">
        <h2>Descargar pistas</h2>
        <div class="tracks">
          <a v-for="t in video.tracks" :key="t.id" class="dl" :href="t.download_url!" download>
            <b>{{ TRACK_NAME[t.kind] }}</b>
            <small>{{ t.width ? `${t.width}×${t.height} · ` : "" }}{{ formatSize(t.size_bytes) }}</small>
          </a>
        </div>
      </section>
    </template>
  </main>
</template>
