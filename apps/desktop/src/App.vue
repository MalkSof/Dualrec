<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  formatClock,
  formatSize,
  type CameraDesc,
  type ClaudeStatus,
  type CloudConfig,
  type Devices,
  type Manifest,
  type ScreenTarget,
  type Status,
  type Track,
} from "./api";

const STORE_KEY = "dualrec.settings.v1";

const settings = reactive({
  screenMode: "monitor" as "monitor" | "window" | "none",
  monitorIndex: 0,
  windowHwnd: 0,
  cameraOn: true,
  cameraIndex: -1,
  micOn: true,
  micName: "",
  systemOn: true,
  fps: 30,
  quality: "media" as "alta" | "media" | "baja",
  maxHeight: 1080,
  cursor: true,
  bubble: true,
  hideFromCapture: true,
  outDir: "",
  cloudOn: false,
  apiUrl: "",
  apiToken: "",
});

try {
  Object.assign(settings, JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}"));
} catch {
  /* ajustes guardados inválidos: se usan los predeterminados */
}
watch(settings, (s) => localStorage.setItem(STORE_KEY, JSON.stringify(s)), { deep: true });

const devices = ref<Devices | null>(null);
const encoder = ref("");
const camera = ref<CameraDesc | null>(null);
const cameraBusy = ref(false);
const micReady = ref(false);
const status = ref<Status>({
  recording: false,
  paused: false,
  elapsed_ms: 0,
  screen_frames: 0,
  mic_peak: 0,
  camera_active: false,
  cloud: null,
  uploads_active: 0,
});
const cloudTest = ref<{ ok: boolean; text: string } | null>(null);
const cloudTesting = ref(false);
const linkCopied = ref(false);
const claude = ref<ClaudeStatus | null>(null);
const claudeBusy = ref(false);
const claudeNote = ref<{ ok: boolean; text: string } | null>(null);
const claudeCopied = ref(false);
const level = ref(0);
const busy = ref<"" | "starting" | "stopping">("");
const error = ref("");
const last = ref<Manifest | null>(null);
const recent = ref<Manifest[]>([]);
const scroller = ref<HTMLElement | null>(null);

const recording = computed(() => status.value.recording);
const cameraLive = computed(() => settings.cameraOn && camera.value !== null);
const hasSource = computed(
  () =>
    settings.screenMode !== "none" ||
    cameraLive.value ||
    (settings.micOn && micReady.value) ||
    settings.systemOn,
);

const cloudConfig = computed<CloudConfig | null>(() =>
  settings.cloudOn && settings.apiUrl.trim() && settings.apiToken.trim()
    ? { api_url: settings.apiUrl.trim(), token: settings.apiToken.trim() }
    : null,
);

/** Subida de la grabación que se está mostrando (la última o la que está en curso). */
const cloud = computed(() => {
  const c = status.value.cloud;
  if (!c) return null;
  if (recording.value) return c;
  return last.value && c.dir === last.value.dir ? c : null;
});

const cloudLabel = computed(() => {
  const c = cloud.value;
  if (!c) return "";
  const pct = c.total_bytes ? Math.round((c.uploaded_bytes / c.total_bytes) * 100) : 0;
  switch (c.state) {
    case "connecting":
      return "Conectando con la nube…";
    case "uploading":
      return recording.value ? `Subiendo · ${formatSize(c.uploaded_bytes)}` : `Subiendo · ${pct}%`;
    case "offline":
      return "Sin conexión · se retoma sola";
    case "finishing":
      return "Terminando la subida…";
    case "done":
      return "En la nube";
    default:
      return c.error ?? "No se pudo subir";
  }
});

async function testCloud() {
  if (!settings.apiUrl.trim() || !settings.apiToken.trim()) return;
  cloudTesting.value = true;
  try {
    const name = await api.cloudCheck({ api_url: settings.apiUrl.trim(), token: settings.apiToken.trim() });
    cloudTest.value = { ok: true, text: `Conectado como ${name}` };
  } catch (e) {
    cloudTest.value = { ok: false, text: String(e) };
  } finally {
    cloudTesting.value = false;
  }
}

async function loadClaude() {
  if (settings.outDir) claude.value = await api.claudeStatus(settings.outDir).catch(() => null);
}

watch(() => settings.outDir, loadClaude);

async function connectClaude() {
  claudeBusy.value = true;
  try {
    claude.value = await api.claudeConnect(settings.outDir);
    claudeNote.value = { ok: true, text: "Listo. Cerrá Claude del todo (también desde la bandeja) y volvé a abrirlo." };
  } catch (e) {
    claudeNote.value = { ok: false, text: String(e) };
  } finally {
    claudeBusy.value = false;
  }
}

async function copyClaudeCommand() {
  if (!claude.value) return;
  claudeCopied.value = await api.copyText(claude.value.code_command);
  setTimeout(() => (claudeCopied.value = false), 1800);
}

async function copyLink(link: string) {
  linkCopied.value = await api.copyText(link);
  setTimeout(() => (linkCopied.value = false), 1800);
}

/** Retoma subidas que quedaron a medias (sin internet o app cerrada). */
async function resumePending() {
  if (cloudConfig.value && settings.outDir) {
    try {
      await api.resumeUploads(cloudConfig.value, settings.outDir);
    } catch {
      /* se vuelve a intentar al abrir la app */
    }
  }
}

const screenTarget = computed<ScreenTarget | null>(() => {
  if (settings.screenMode === "monitor") return { kind: "monitor", index: settings.monitorIndex };
  if (settings.screenMode === "window" && settings.windowHwnd)
    return { kind: "window", hwnd: settings.windowHwnd };
  return null;
});

// Vista previa en vivo de la pantalla o ventana elegida.
const screenLive = ref(false);
const screenKey = ref(0);
watch(
  screenTarget,
  (t) => {
    screenLive.value = false;
    screenKey.value++;
    void api.setScreenPreview(t).catch(() => {});
  },
  { immediate: true },
);

const stageTitle = computed(() => {
  const d = devices.value;
  if (!d) return { title: "Buscando dispositivos…", sub: "" };
  if (settings.screenMode === "monitor") {
    const m = d.monitors.find((x) => x.index === settings.monitorIndex);
    return m
      ? { title: m.name, sub: `${m.width} × ${m.height} · ${m.refresh_rate} Hz` }
      : { title: "Sin pantalla", sub: "" };
  }
  if (settings.screenMode === "window") {
    const w = d.windows.find((x) => x.hwnd === settings.windowHwnd);
    return w ? { title: w.title, sub: w.process } : { title: "Elegí una ventana", sub: "" };
  }
  return { title: "Sin pantalla", sub: "Solo cámara y audio" };
});

function fail(e: unknown) {
  error.value = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

async function refreshDevices() {
  try {
    const d = await api.devices();
    devices.value = d;
    if (!settings.outDir) settings.outDir = d.default_dir;
    if (!d.monitors.some((m) => m.index === settings.monitorIndex)) {
      settings.monitorIndex = d.monitors.find((m) => m.primary)?.index ?? 0;
    }
    if (!d.windows.some((w) => w.hwnd === settings.windowHwnd)) {
      settings.windowHwnd = d.windows[0]?.hwnd ?? 0;
    }
    if (!d.cameras.some((c) => c.index === settings.cameraIndex)) {
      settings.cameraIndex = d.cameras[0]?.index ?? -1;
    }
    if (!d.mics.includes(settings.micName)) {
      settings.micName = d.default_mic ?? d.mics[0] ?? "";
    }
    if (!d.ffmpeg_ok) {
      fail("No se encontró FFmpeg. Reinstalá Dualrec o instalá ffmpeg y agregalo al PATH.");
    }
  } catch (e) {
    fail(e);
  }
}

async function applyCamera() {
  if (recording.value) return;
  cameraBusy.value = true;
  try {
    const wanted = settings.cameraOn && settings.cameraIndex >= 0 ? settings.cameraIndex : null;
    camera.value = await api.setCamera(wanted);
  } catch (e) {
    camera.value = null;
    settings.cameraOn = false;
    fail(e);
  } finally {
    cameraBusy.value = false;
    void syncBubble();
  }
}

async function applyMic() {
  if (recording.value) return;
  try {
    const desc = await api.setMic(settings.micOn && !!settings.micName, settings.micName || null);
    micReady.value = desc !== null;
  } catch (e) {
    micReady.value = false;
    settings.micOn = false;
    fail(e);
  }
}

async function syncBubble() {
  try {
    await api.setBubble(settings.bubble && cameraLive.value);
  } catch {
    /* la burbuja es opcional */
  }
}

async function loadRecent() {
  if (!settings.outDir) return;
  try {
    recent.value = await api.recordings(settings.outDir);
  } catch {
    recent.value = [];
  }
}

async function start() {
  if (busy.value || recording.value) return;
  if (!hasSource.value) return fail("Elegí al menos una fuente para grabar.");
  busy.value = "starting";
  error.value = "";
  try {
    await api.start({
      screen: screenTarget.value,
      camera: cameraLive.value,
      mic: settings.micOn && micReady.value,
      system_audio: settings.systemOn,
      options: {
        out_dir: settings.outDir,
        fps: settings.fps,
        quality: settings.quality,
        max_height: settings.maxHeight || null,
        cursor: settings.cursor,
      },
      cloud: cloudConfig.value,
      title: null,
    });
    last.value = null;
    status.value = { ...status.value, recording: true, paused: false, elapsed_ms: 0 };
  } catch (e) {
    fail(e);
  } finally {
    busy.value = "";
  }
}

async function stop() {
  if (busy.value || !recording.value) return;
  busy.value = "stopping";
  try {
    last.value = await api.stop();
    status.value = { ...status.value, recording: false, paused: false };
    scroller.value?.scrollTo({ top: 0, behavior: "smooth" });
    await loadRecent();
  } catch (e) {
    fail(e);
  } finally {
    busy.value = "";
  }
}

async function togglePause() {
  if (!recording.value || busy.value) return;
  try {
    if (status.value.paused) await api.resume();
    else await api.pause();
    status.value = { ...status.value, paused: !status.value.paused };
  } catch (e) {
    fail(e);
  }
}

async function pickFolder() {
  const dir = await open({ directory: true, defaultPath: settings.outDir || undefined });
  if (typeof dir === "string") settings.outDir = dir;
}

function trackLabel(t: Track): string {
  if (t.width) return `${t.width}×${t.height} · ${t.fps} fps`;
  return `${(t.sample_rate ?? 0) / 1000} kHz · ${t.channels === 1 ? "mono" : "estéreo"}`;
}

function recentTitle(m: Manifest): string {
  const d = new Date(m.recorded_at);
  return Number.isNaN(d.getTime())
    ? m.id
    : d.toLocaleString("es-AR", { dateStyle: "medium", timeStyle: "short" });
}

function totalSize(m: Manifest): number {
  return m.tracks.reduce((a, t) => a + t.size_bytes, 0);
}

watch(() => [settings.cameraOn, settings.cameraIndex], applyCamera);
watch(() => [settings.micOn, settings.micName], applyMic);
watch(() => settings.bubble, syncBubble);
watch(() => settings.hideFromCapture, (v) => void api.setCaptureHidden(v));
watch(() => settings.outDir, loadRecent);
watch(cloudConfig, resumePending);

let timer = 0;
let unlisten: UnlistenFn | undefined;

onMounted(async () => {
  await refreshDevices();
  void api.setCaptureHidden(settings.hideFromCapture);
  void api.encoder().then((e) => (encoder.value = e));
  await Promise.all([applyCamera(), applyMic(), loadRecent()]);
  const defaults = await api.cloudDefaults().catch(() => null);
  if (defaults && !settings.apiUrl && !settings.apiToken) {
    [settings.apiUrl, settings.apiToken] = defaults;
    settings.cloudOn = true;
  }
  void resumePending();
  void loadClaude();

  timer = window.setInterval(async () => {
    try {
      const s = await api.status();
      // Durante el arranque o el corte manda el estado local.
      status.value = busy.value ? { ...s, recording: status.value.recording } : s;
      level.value = Math.max(Math.min(1, s.mic_peak * 1.6), level.value * 0.72);
    } catch {
      /* se reintenta en el próximo tick */
    }
  }, 120);

  unlisten = await listen<string>("shortcut", ({ payload }) => {
    if (payload === "toggle-record") void (recording.value ? stop() : start());
    else if (payload === "toggle-pause") void togglePause();
  });
});

onBeforeUnmount(() => {
  clearInterval(timer);
  unlisten?.();
});
</script>

<template>
  <div class="app">
    <header class="top">
      <svg class="logo" viewBox="0 0 64 64" aria-hidden="true">
        <defs>
          <linearGradient id="lg-s" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="#8b7cff" />
            <stop offset="1" stop-color="#4da3ff" />
          </linearGradient>
          <linearGradient id="lg-c" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="#ff6b81" />
            <stop offset="1" stop-color="#ff3d57" />
          </linearGradient>
        </defs>
        <rect x="4" y="10" width="44" height="32" rx="7" fill="url(#lg-s)" />
        <circle cx="44" cy="42" r="16" fill="#0b0d12" />
        <circle cx="44" cy="42" r="12.5" fill="url(#lg-c)" />
        <circle cx="44" cy="42" r="4.5" fill="#fff" />
      </svg>
      <div class="brand">
        Dualrec
        <small>Pantalla y cámara, en pistas separadas</small>
      </div>
      <span v-if="encoder" class="chip" :class="{ hw: encoder.includes('hardware') }" :title="`Encoder H.264: ${encoder}`">
        {{ encoder.replace(" (hardware)", "") }}
      </span>
    </header>

    <main ref="scroller" class="scroll">
      <section class="stage" :class="{ live: recording && !status.paused }">
        <img
          v-if="screenTarget && devices"
          v-show="screenLive"
          :key="screenKey"
          class="stage-shot"
          :src="`${devices.screen_preview_url}?k=${screenKey}`"
          alt="Vista previa de la pantalla"
          @load="screenLive = true"
          @error="screenLive = false"
        />
        <div v-if="screenTarget && screenLive" class="stage-tag">{{ stageTitle.title }}</div>
        <div v-else-if="settings.screenMode !== 'none' || !cameraLive" class="stage-screen">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <rect x="2.5" y="4" width="19" height="13" rx="2.2" />
            <path d="M8.5 20.5h7M12 17v3.5" />
          </svg>
          <b>{{ stageTitle.title }}</b>
          <span v-if="stageTitle.sub">{{ stageTitle.sub }}</span>
        </div>
        <div v-if="cameraLive && devices" class="stage-cam" :class="{ solo: settings.screenMode === 'none' }">
          <img :src="devices.preview_url" alt="Vista previa de la cámara" />
        </div>
        <div v-if="recording" class="rec-pill">
          <span class="dot" :class="{ paused: status.paused }" />
          {{ status.paused ? "PAUSA" : "REC" }} {{ formatClock(status.elapsed_ms) }}
        </div>
        <div v-if="recording && cloud" class="cloud-pill" :class="cloud.state">{{ cloudLabel }}</div>
      </section>

      <section v-if="last" class="card">
        <div class="result-head">
          <b><span class="ok">✓</span> Grabación guardada · {{ formatClock(last.duration_ms) }}</b>
          <button class="ghost" @click="api.openFolder(last.dir)">Abrir carpeta</button>
        </div>
        <div v-for="t in last.tracks" :key="t.file" class="track" :class="{ bad: t.error }">
          <svg v-if="t.width" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2.5" /><path d="m10 9.5 4.5 2.5-4.5 2.5v-5Z" fill="currentColor" /></svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3" /></svg>
          <code>{{ t.file }}</code>
          <small>{{ trackLabel(t) }} · {{ formatSize(t.size_bytes) }}</small>
          <div v-if="t.error" class="err">{{ t.error }}</div>
        </div>
        <div v-if="cloud" class="cloud-row" :class="cloud.state">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 18a4.5 4.5 0 0 1-.6-8.96 6 6 0 0 1 11.66 1.5A3.75 3.75 0 0 1 17.5 18H7Z" /></svg>
          <span>
            {{ cloudLabel }}
            <small v-if="cloud.share_url" :title="cloud.share_url">{{ cloud.share_url }}</small>
          </span>
          <button v-if="cloud.share_url" class="ghost" @click="copyLink(cloud.share_url)">{{ linkCopied ? "¡Copiado!" : "Copiar link" }}</button>
        </div>
        <div class="card-foot" />
      </section>

      <section class="card">
        <div class="row" :class="{ on: settings.screenMode !== 'none' }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2.5" y="4" width="19" height="13" rx="2.2" />
              <path d="M8.5 20.5h7M12 17v3.5" />
            </svg>
          </div>
          <div class="label">Pantalla <small>screen.mp4</small></div>
          <div class="seg" role="tablist">
            <button :class="{ active: settings.screenMode === 'monitor' }" :disabled="recording" @click="settings.screenMode = 'monitor'">Pantalla</button>
            <button :class="{ active: settings.screenMode === 'window' }" :disabled="recording" @click="settings.screenMode = 'window'; refreshDevices()">Ventana</button>
            <button :class="{ active: settings.screenMode === 'none' }" :disabled="recording" @click="settings.screenMode = 'none'">No</button>
          </div>
          <div v-if="settings.screenMode === 'monitor'" class="wide">
            <select v-model.number="settings.monitorIndex" :disabled="recording" aria-label="Pantalla a grabar">
              <option v-for="m in devices?.monitors ?? []" :key="m.index" :value="m.index">
                {{ m.name }} — {{ m.width }}×{{ m.height }}{{ m.primary ? " (principal)" : "" }}
              </option>
            </select>
          </div>
          <div v-else-if="settings.screenMode === 'window'" class="wide">
            <select v-model.number="settings.windowHwnd" :disabled="recording" aria-label="Ventana a grabar" @focus="refreshDevices">
              <option v-if="!devices?.windows.length" :value="0">No hay ventanas abiertas</option>
              <option v-for="w in devices?.windows ?? []" :key="w.hwnd" :value="w.hwnd">
                {{ w.title }}{{ w.process ? ` — ${w.process}` : "" }}
              </option>
            </select>
          </div>
        </div>

        <div class="row" :class="{ on: cameraLive }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2.5" y="6" width="13.5" height="12" rx="2.4" />
              <path d="m16 10.5 5.5-3v9l-5.5-3" />
            </svg>
          </div>
          <div class="label">
            Cámara
            <small>{{ cameraBusy ? "Abriendo cámara…" : camera ? `camera.mp4 · ${camera.width}×${camera.height}` : "camera.mp4" }}</small>
          </div>
          <button class="switch" role="switch" :aria-checked="settings.cameraOn" :disabled="recording || cameraBusy || !devices?.cameras.length" aria-label="Grabar cámara" @click="settings.cameraOn = !settings.cameraOn" />
          <div class="wide">
            <select v-model.number="settings.cameraIndex" :disabled="recording || cameraBusy || !settings.cameraOn" aria-label="Cámara">
              <option v-if="!devices?.cameras.length" :value="-1">No se detectó ninguna cámara</option>
              <option v-for="c in devices?.cameras ?? []" :key="c.index" :value="c.index">{{ c.name }}</option>
            </select>
          </div>
        </div>

        <div class="row" :class="{ on: settings.micOn && micReady }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="9" y="2.5" width="6" height="11.5" rx="3" />
              <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3.5" />
            </svg>
          </div>
          <div class="label">Micrófono <small>mic.m4a</small></div>
          <button class="switch" role="switch" :aria-checked="settings.micOn" :disabled="recording || !devices?.mics.length" aria-label="Grabar micrófono" @click="settings.micOn = !settings.micOn" />
          <div class="wide">
            <select v-model="settings.micName" :disabled="recording || !settings.micOn" aria-label="Micrófono">
              <option v-if="!devices?.mics.length" value="">No se detectó ningún micrófono</option>
              <option v-for="m in devices?.mics ?? []" :key="m" :value="m">{{ m }}</option>
            </select>
            <div class="meter" aria-hidden="true"><i :style="{ width: `${settings.micOn && micReady ? level * 100 : 0}%` }" /></div>
          </div>
        </div>

        <div class="row" :class="{ on: settings.systemOn }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path d="M4 9.5v5h3.5L12 18.5v-13L7.5 9.5H4Z" />
              <path d="M15.5 9a4.2 4.2 0 0 1 0 6M18 6.5a7.8 7.8 0 0 1 0 11" />
            </svg>
          </div>
          <div class="label">
            Audio del sistema
            <small>system.m4a{{ devices?.system_output ? ` · ${devices.system_output}` : "" }}</small>
          </div>
          <button class="switch" role="switch" :aria-checked="settings.systemOn" :disabled="recording" aria-label="Grabar audio del sistema" @click="settings.systemOn = !settings.systemOn" />
        </div>
      </section>

      <details class="card">
        <summary>Ajustes <small>{{ settings.fps }} fps · calidad {{ settings.quality }}</small></summary>
        <div class="grid">
          <div class="field">
            <label for="fps">Cuadros</label>
            <select id="fps" v-model.number="settings.fps" :disabled="recording">
              <option :value="30">30 fps</option>
              <option :value="60">60 fps</option>
            </select>
          </div>
          <div class="field">
            <label for="quality">Calidad</label>
            <select id="quality" v-model="settings.quality" :disabled="recording">
              <option value="alta">Alta</option>
              <option value="media">Media</option>
              <option value="baja">Liviana</option>
            </select>
          </div>
          <div class="field">
            <label for="res">Resolución</label>
            <select id="res" v-model.number="settings.maxHeight" :disabled="recording">
              <option :value="0">Nativa</option>
              <option :value="1440">1440p</option>
              <option :value="1080">1080p</option>
              <option :value="720">720p</option>
            </select>
          </div>
        </div>
        <label class="opt">
          <span>Burbuja flotante de cámara<small>Te ves mientras grabás; se puede arrastrar</small></span>
          <button class="switch" role="switch" :aria-checked="settings.bubble" @click="settings.bubble = !settings.bubble" />
        </label>
        <label class="opt">
          <span>Ocultar Dualrec en la grabación<small>La app y la burbuja no salen en screen.mp4</small></span>
          <button class="switch" role="switch" :aria-checked="settings.hideFromCapture" @click="settings.hideFromCapture = !settings.hideFromCapture" />
        </label>
        <label class="opt">
          <span>Mostrar el cursor<small>Incluye el puntero del mouse en la pantalla</small></span>
          <button class="switch" role="switch" :aria-checked="settings.cursor" :disabled="recording" @click="settings.cursor = !settings.cursor" />
        </label>
        <div class="opt">
          <span>Carpeta de grabaciones<small :title="settings.outDir">{{ settings.outDir }}</small></span>
          <button class="ghost" :disabled="recording" @click="pickFolder">Cambiar</button>
        </div>
      </details>

      <details class="card">
        <summary>Nube <small>{{ settings.cloudOn ? (cloudConfig ? "activada" : "falta configurar") : "desactivada" }}</small></summary>
        <label class="opt first">
          <span>Subir a la nube mientras grabo<small>Al cortar, el link para compartir queda en el portapapeles</small></span>
          <button class="switch" role="switch" :aria-checked="settings.cloudOn" :disabled="recording" @click="settings.cloudOn = !settings.cloudOn" />
        </label>
        <div class="fields">
          <label for="api-url">Dirección de Dualrec</label>
          <input id="api-url" v-model="settings.apiUrl" class="text" type="url" placeholder="https://api.tu-dominio.com" spellcheck="false" :disabled="recording" />
          <label for="api-token">Token de acceso</label>
          <input id="api-token" v-model="settings.apiToken" class="text" type="password" placeholder="dr_…" autocomplete="off" spellcheck="false" :disabled="recording" />
          <div class="test">
            <button class="ghost" :disabled="cloudTesting || !settings.apiUrl.trim() || !settings.apiToken.trim()" @click="testCloud">{{ cloudTesting ? "Probando…" : "Probar conexión" }}</button>
            <small v-if="cloudTest" :class="cloudTest.ok ? 'good' : 'bad'">{{ cloudTest.text }}</small>
          </div>
        </div>
      </details>

      <details class="card">
        <summary>Claude (IA) <small>{{ claude?.connected ? "conectado" : "armá videos y reels" }}</small></summary>
        <div class="fields">
          <p class="about">Conectá Dualrec con Claude y pedile en palabras que arme el video: "hacé un reel vertical con la mejor parte de mi última grabación" o "sacá los silencios y dejá la cámara en una burbuja".</p>
          <div class="test">
            <button class="ghost" :disabled="claudeBusy || !settings.outDir" @click="connectClaude">{{ claude?.connected ? "Volver a conectar" : "Conectar con Claude Desktop" }}</button>
            <button class="ghost" :disabled="!claude" title="Copia el comando para sumarlo en Claude Code" @click="copyClaudeCommand">{{ claudeCopied ? "¡Copiado!" : "Copiar comando para Claude Code" }}</button>
          </div>
          <small v-if="claudeNote" :class="claudeNote.ok ? 'good' : 'bad'">{{ claudeNote.text }}</small>
          <small v-else-if="claude && !claude.desktop_found" class="muted">Necesitás la app de Claude para escritorio (claude.ai/download).</small>
          <small class="muted">Los videos que arma quedan en la subcarpeta <code>exports</code> de cada grabación.</small>
        </div>
      </details>

      <details class="card">
        <summary>Grabaciones <small>{{ recent.length || "" }}</small></summary>
        <div v-if="!recent.length" class="empty">Todavía no hay grabaciones en esta carpeta.</div>
        <button v-for="m in recent" :key="m.id" class="recent" :title="m.dir" @click="api.openFolder(m.dir)">
          <span>{{ recentTitle(m) }}<br /><small>{{ m.tracks.map((t) => t.file).join(" · ") }}</small></span>
          <small>{{ formatClock(m.duration_ms) }} · {{ formatSize(totalSize(m)) }}</small>
        </button>
      </details>
    </main>

    <footer class="bottom">
      <template v-if="!recording">
        <button class="rec-btn" :disabled="!!busy || !hasSource || !devices" @click="start">
          <span v-if="busy === 'starting'" class="spinner" />
          <i v-else />
          {{ busy === "starting" ? "Preparando…" : "Grabar" }}
        </button>
        <div class="hint"><kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>R</kbd> para grabar o cortar desde cualquier app</div>
      </template>
      <template v-else>
        <div class="live-bar">
          <div class="timer">
            <span class="dot" :class="{ paused: status.paused }" />
            {{ formatClock(status.elapsed_ms) }}
            <small v-if="status.paused">en pausa</small>
          </div>
          <button class="round" :disabled="!!busy" :title="status.paused ? 'Reanudar (Ctrl+Alt+P)' : 'Pausar (Ctrl+Alt+P)'" @click="togglePause">
            <svg v-if="status.paused" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l12-7.5-12-7.5Z" /></svg>
            <svg v-else viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4.5" width="4.2" height="15" rx="1.2" /><rect x="13.8" y="4.5" width="4.2" height="15" rx="1.2" /></svg>
            {{ status.paused ? "Reanudar" : "Pausar" }}
          </button>
          <button class="round stop" :disabled="!!busy" title="Cortar (Ctrl+Alt+R)" @click="stop">
            <span v-if="busy === 'stopping'" class="spinner" />
            <svg v-else viewBox="0 0 24 24" fill="currentColor"><rect x="5.5" y="5.5" width="13" height="13" rx="2.6" /></svg>
            {{ busy === "stopping" ? "Guardando…" : "Cortar" }}
          </button>
        </div>
      </template>
    </footer>

    <div v-if="error" class="toast" role="alert">
      <span>{{ error }}</span>
      <button aria-label="Cerrar aviso" @click="error = ''">×</button>
    </div>
  </div>
</template>
