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
import { lang, locale, native, t } from "./i18n";

const STORE_KEY = "dualrec.settings.v1";

/** La subida a la nube todavía no está publicada: se oculta y no se usa. */
const CLOUD_ENABLED = false;

const settings = reactive({
  /** "video": pantalla y cámara; "audio": solo micrófono y sonido de la compu. */
  mode: "video" as "video" | "audio",
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
/** Nombre para la próxima grabación (también nombra su carpeta). */
const recName = ref("");
/** Nombre de la grabación en curso. */
const currentName = ref("");

const recording = computed(() => status.value.recording);
const audioMode = computed(() => settings.mode === "audio");
const cameraLive = computed(() => !audioMode.value && settings.cameraOn && camera.value !== null);
const hasAudio = computed(() => (settings.micOn && micReady.value) || settings.systemOn);
const hasSource = computed(() =>
  audioMode.value ? hasAudio.value : settings.screenMode !== "none" || cameraLive.value || hasAudio.value,
);

/** Últimos niveles del micrófono, para dibujar la onda en el modo audio. */
const BARS = 36;
const levels = ref<number[]>(Array(BARS).fill(0));

const cloudConfig = computed<CloudConfig | null>(() =>
  CLOUD_ENABLED &&
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
      return t("cloudConnecting");
    case "uploading":
      return t("cloudUploading", { progress: recording.value ? formatSize(c.uploaded_bytes) : `${pct}%` });
    case "offline":
      return t("cloudOffline");
    case "finishing":
      return t("cloudFinishing");
    case "done":
      return t("cloudDone");
    default:
      return c.error ? native(c.error) : t("cloudFailed");
  }
});

async function testCloud() {
  if (!settings.apiUrl.trim() || !settings.apiToken.trim()) return;
  cloudTesting.value = true;
  try {
    const name = await api.cloudCheck({ api_url: settings.apiUrl.trim(), token: settings.apiToken.trim() });
    cloudTest.value = { ok: true, text: name };
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
    claudeNote.value = { ok: true, text: "" };
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
  if (audioMode.value) return null;
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
  if (!d) return { title: t("searchingDevices"), sub: "" };
  if (settings.screenMode === "monitor") {
    const m = d.monitors.find((x) => x.index === settings.monitorIndex);
    return m
      ? { title: native(m.name), sub: `${m.width} × ${m.height} · ${m.refresh_rate} Hz` }
      : { title: t("noScreen"), sub: "" };
  }
  if (settings.screenMode === "window") {
    const w = d.windows.find((x) => x.hwnd === settings.windowHwnd);
    return w ? { title: w.title, sub: w.process } : { title: t("pickWindow"), sub: "" };
  }
  return { title: t("noScreen"), sub: t("onlyCamAudio") };
});

// Los avisos se guardan en español y se traducen al mostrarlos (ver `native`).
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
    const wanted = !audioMode.value && settings.cameraOn && settings.cameraIndex >= 0 ? settings.cameraIndex : null;
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
  if (!hasSource.value) return fail(audioMode.value ? t("needAudio") : "Elegí al menos una fuente para grabar.");
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
        name: recName.value.trim() || null,
      },
      cloud: cloudConfig.value,
      title: recName.value.trim() || null,
    });
    currentName.value = recName.value.trim();
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
    recName.value = "";
    currentName.value = "";
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

function trackLabel(tr: Track): string {
  if (tr.width) return `${tr.width}×${tr.height} · ${tr.fps} fps`;
  return `${(tr.sample_rate ?? 0) / 1000} kHz · ${tr.channels === 1 ? "mono" : t("stereo")}`;
}

function recordedAt(m: Manifest): string {
  const d = new Date(m.recorded_at);
  return Number.isNaN(d.getTime())
    ? m.id
    : d.toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });
}

/** Aviso bajo el botón cuando no se puede grabar todavía. */
const recBlocker = computed(() =>
  !devices.value ? t("searchingDevices") : !hasSource.value ? t(audioMode.value ? "needAudio" : "needSource") : "",
);

function isAudioOnly(m: Manifest): boolean {
  return !m.tracks.some((tr) => tr.width);
}

/** Pedido listo para pegar en Claude con la grabación recién guardada. */
const claudePrompt = computed(() => {
  const m = last.value;
  if (!m) return "";
  const name = m.name || recordedAt(m);
  return isAudioOnly(m) ? t("promptAudio", { name }) : t("promptVideo", { name });
});
const promptCopied = ref(false);
async function copyPrompt() {
  promptCopied.value = await api.copyText(claudePrompt.value);
  setTimeout(() => (promptCopied.value = false), 1800);
}

const QUALITY = { alta: "qualityHigh", media: "qualityMedium", baja: "qualityLow" } as const;

function totalSize(m: Manifest): number {
  return m.tracks.reduce((a, tr) => a + tr.size_bytes, 0);
}

watch(() => [settings.cameraOn, settings.cameraIndex, settings.mode], applyCamera);
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
  const defaults = CLOUD_ENABLED ? await api.cloudDefaults().catch(() => null) : null;
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
      if (audioMode.value) {
        const on = settings.micOn && micReady.value && !(s.recording && s.paused);
        levels.value = [...levels.value.slice(1), on ? level.value : 0];
      }
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
        <small>{{ t("tagline") }}</small>
      </div>
      <span v-if="encoder" class="chip" :class="{ hw: encoder.includes('hardware') }" :title="`Encoder H.264: ${encoder}`">
        {{ encoder.replace(" (hardware)", "") }}
      </span>
    </header>

    <main ref="scroller" class="scroll">
      <div class="mode" role="radiogroup" :aria-label="t('modeLabel')">
        <button role="radio" :aria-checked="!audioMode" :disabled="recording || !!busy" @click="settings.mode = 'video'">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="6" width="13.5" height="12" rx="2.4" /><path d="m16 10.5 5.5-3v9l-5.5-3" /></svg>
          {{ t("modeVideo") }}
        </button>
        <button role="radio" :aria-checked="audioMode" :disabled="recording || !!busy" @click="settings.mode = 'audio'">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="2.5" width="6" height="11.5" rx="3" /><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3.5" /></svg>
          {{ t("modeAudio") }}
        </button>
      </div>

      <section v-if="audioMode" class="stage audio-stage" :class="{ live: recording && !status.paused }">
        <div class="wave" aria-hidden="true">
          <i v-for="(l, i) in levels" :key="i" :style="{ transform: `scaleY(${Math.max(0.06, l)})` }" />
        </div>
        <div class="audio-label">
          <b>{{ settings.micOn && micReady ? settings.micName : t("noMicOn") }}</b>
          <span>{{ settings.systemOn ? t("audioPlusSystem") : t("audioOnlyMic") }}</span>
        </div>
        <div v-if="recording" class="rec-pill">
          <span class="dot" :class="{ paused: status.paused }" />
          {{ status.paused ? t("paused") : "REC" }} {{ formatClock(status.elapsed_ms) }}
          <span v-if="currentName" class="rec-name">· {{ currentName }}</span>
        </div>
      </section>

      <section v-else class="stage" :class="{ live: recording && !status.paused }">
        <img
          v-if="screenTarget && devices"
          v-show="screenLive"
          :key="screenKey"
          class="stage-shot"
          :src="`${devices.screen_preview_url}?k=${screenKey}`"
          :alt="t('screenPreviewAlt')"
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
          <img :src="devices.preview_url" :alt="t('cameraPreviewAlt')" />
        </div>
        <div v-if="recording" class="rec-pill">
          <span class="dot" :class="{ paused: status.paused }" />
          {{ status.paused ? t("paused") : "REC" }} {{ formatClock(status.elapsed_ms) }}
          <span v-if="currentName" class="rec-name">· {{ currentName }}</span>
        </div>
        <div v-if="recording && cloud" class="cloud-pill" :class="cloud.state">{{ cloudLabel }}</div>
      </section>

      <section v-if="last" class="card">
        <div class="result-head">
          <b><span class="ok">✓</span> {{ last.name || (isAudioOnly(last) ? t("savedAudio") : t("saved")) }} · {{ formatClock(last.duration_ms) }}</b>
          <button class="ghost" @click="api.openFolder(last.dir)">{{ t("openFolder") }}</button>
        </div>
        <div v-for="tr in last.tracks" :key="tr.file" class="track" :class="{ bad: tr.error }">
          <svg v-if="tr.width" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2.5" /><path d="m10 9.5 4.5 2.5-4.5 2.5v-5Z" fill="currentColor" /></svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3" /></svg>
          <code>{{ tr.file }}</code>
          <small>{{ trackLabel(tr) }} · {{ formatSize(tr.size_bytes) }}</small>
          <div v-if="tr.error" class="err">{{ native(tr.error) }}</div>
        </div>
        <div v-if="cloud" class="cloud-row" :class="cloud.state">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 18a4.5 4.5 0 0 1-.6-8.96 6 6 0 0 1 11.66 1.5A3.75 3.75 0 0 1 17.5 18H7Z" /></svg>
          <span>
            {{ cloudLabel }}
            <small v-if="cloud.share_url" :title="cloud.share_url">{{ cloud.share_url }}</small>
          </span>
          <button v-if="cloud.share_url" class="ghost" @click="copyLink(cloud.share_url)">{{ linkCopied ? t("copied") : t("copyLink") }}</button>
        </div>
        <div class="saved-in" :title="last.dir">{{ t("savedIn") }} <code>{{ last.dir }}</code></div>
        <div class="ask-claude">
          <div>
            <b>{{ isAudioOnly(last) ? t("askAudioTitle") : t("askVideoTitle") }}</b>
            <q>{{ claudePrompt }}</q>
            <small v-if="!claude?.connected">{{ t("askConnectFirst") }}</small>
          </div>
          <button class="ghost" @click="copyPrompt">{{ promptCopied ? t("copied") : t("copyPrompt") }}</button>
        </div>
      </section>

      <section class="card">
        <div v-if="!audioMode" class="row" :class="{ on: settings.screenMode !== 'none' }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2.5" y="4" width="19" height="13" rx="2.2" />
              <path d="M8.5 20.5h7M12 17v3.5" />
            </svg>
          </div>
          <div class="label">{{ t("screen") }} <small>screen.mp4</small></div>
          <div class="seg" role="tablist">
            <button :class="{ active: settings.screenMode === 'monitor' }" :disabled="recording" @click="settings.screenMode = 'monitor'">{{ t("screen") }}</button>
            <button :class="{ active: settings.screenMode === 'window' }" :disabled="recording" @click="settings.screenMode = 'window'; refreshDevices()">{{ t("window") }}</button>
            <button :class="{ active: settings.screenMode === 'none' }" :disabled="recording" @click="settings.screenMode = 'none'">{{ t("no") }}</button>
          </div>
          <div v-if="settings.screenMode === 'monitor'" class="wide">
            <select v-model.number="settings.monitorIndex" :disabled="recording" :aria-label="t('screenToRecord')">
              <option v-for="m in devices?.monitors ?? []" :key="m.index" :value="m.index">
                {{ native(m.name) }} — {{ m.width }}×{{ m.height }}{{ m.primary ? t("primary") : "" }}
              </option>
            </select>
          </div>
          <div v-else-if="settings.screenMode === 'window'" class="wide">
            <select v-model.number="settings.windowHwnd" :disabled="recording" :aria-label="t('windowToRecord')" @focus="refreshDevices">
              <option v-if="!devices?.windows.length" :value="0">{{ t("noWindows") }}</option>
              <option v-for="w in devices?.windows ?? []" :key="w.hwnd" :value="w.hwnd">
                {{ w.title }}{{ w.process ? ` — ${w.process}` : "" }}
              </option>
            </select>
          </div>
        </div>

        <div v-if="!audioMode" class="row" :class="{ on: cameraLive }">
          <div class="ico">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2.5" y="6" width="13.5" height="12" rx="2.4" />
              <path d="m16 10.5 5.5-3v9l-5.5-3" />
            </svg>
          </div>
          <div class="label">
            {{ t("camera") }}
            <small>{{ cameraBusy ? t("openingCamera") : camera ? `camera.mp4 · ${camera.width}×${camera.height}` : "camera.mp4" }}</small>
          </div>
          <button class="switch" role="switch" :aria-checked="settings.cameraOn && !!devices?.cameras.length" :disabled="recording || cameraBusy || !devices?.cameras.length" :aria-label="t('recordCamera')" @click="settings.cameraOn = !settings.cameraOn" />
          <div class="wide">
            <select v-model.number="settings.cameraIndex" :disabled="recording || cameraBusy || !settings.cameraOn" :aria-label="t('camera')">
              <option v-if="!devices?.cameras.length" :value="-1">{{ t("noCamera") }}</option>
              <option v-for="c in devices?.cameras ?? []" :key="c.index" :value="c.index">{{ native(c.name) }}</option>
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
          <div class="label">{{ t("mic") }} <small>mic.m4a</small></div>
          <button class="switch" role="switch" :aria-checked="settings.micOn && !!devices?.mics.length" :disabled="recording || !devices?.mics.length" :aria-label="t('recordMic')" @click="settings.micOn = !settings.micOn" />
          <div class="wide">
            <select v-model="settings.micName" :disabled="recording || !settings.micOn" :aria-label="t('mic')">
              <option v-if="!devices?.mics.length" value="">{{ t("noMic") }}</option>
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
            {{ t("systemAudio") }}
            <small>system.m4a{{ devices?.system_output ? ` · ${devices.system_output}` : "" }}</small>
          </div>
          <button class="switch" role="switch" :aria-checked="settings.systemOn" :disabled="recording" :aria-label="t('recordSystem')" @click="settings.systemOn = !settings.systemOn" />
        </div>
      </section>

      <details class="card">
        <summary>{{ t("settings") }} <small>{{ audioMode ? t("settingsAudioSummary") : t("settingsSummary", { fps: settings.fps, quality: t(QUALITY[settings.quality]).toLowerCase() }) }}</small></summary>
        <div v-if="!audioMode" class="grid">
          <div class="field">
            <label for="fps">{{ t("frames") }}</label>
            <select id="fps" v-model.number="settings.fps" :disabled="recording">
              <option :value="30">30 fps</option>
              <option :value="60">60 fps</option>
            </select>
          </div>
          <div class="field">
            <label for="quality">{{ t("quality") }}</label>
            <select id="quality" v-model="settings.quality" :disabled="recording">
              <option value="alta">{{ t("qualityHigh") }}</option>
              <option value="media">{{ t("qualityMedium") }}</option>
              <option value="baja">{{ t("qualityLow") }}</option>
            </select>
          </div>
          <div class="field">
            <label for="res">{{ t("resolution") }}</label>
            <select id="res" v-model.number="settings.maxHeight" :disabled="recording">
              <option :value="0">{{ t("native") }}</option>
              <option :value="1440">1440p</option>
              <option :value="1080">1080p</option>
              <option :value="720">720p</option>
            </select>
          </div>
        </div>
        <label v-if="!audioMode" class="opt">
          <span>{{ t("bubble") }}<small>{{ t("bubbleHint") }}</small></span>
          <button class="switch" role="switch" :aria-checked="settings.bubble" @click="settings.bubble = !settings.bubble" />
        </label>
        <label v-if="!audioMode" class="opt">
          <span>{{ t("hideApp") }}<small>{{ t("hideAppHint") }}</small></span>
          <button class="switch" role="switch" :aria-checked="settings.hideFromCapture" @click="settings.hideFromCapture = !settings.hideFromCapture" />
        </label>
        <label v-if="!audioMode" class="opt">
          <span>{{ t("cursor") }}<small>{{ t("cursorHint") }}</small></span>
          <button class="switch" role="switch" :aria-checked="settings.cursor" :disabled="recording" @click="settings.cursor = !settings.cursor" />
        </label>
        <div class="opt">
          <span>{{ t("folder") }}<small :title="settings.outDir">{{ settings.outDir }}</small></span>
          <button class="ghost" :disabled="recording" @click="pickFolder">{{ t("change") }}</button>
        </div>
        <div class="opt">
          <span>{{ t("language") }}</span>
          <div class="seg">
            <button :class="{ active: lang === 'es' }" @click="lang = 'es'">Español</button>
            <button :class="{ active: lang === 'en' }" @click="lang = 'en'">English</button>
          </div>
        </div>
      </details>

      <details v-if="CLOUD_ENABLED" class="card">
        <summary>{{ t("cloud") }} <small>{{ settings.cloudOn ? (cloudConfig ? t("cloudOn") : t("cloudNeedsSetup")) : t("cloudOff") }}</small></summary>
        <label class="opt first">
          <span>{{ t("cloudUpload") }}<small>{{ t("cloudUploadHint") }}</small></span>
          <button class="switch" role="switch" :aria-checked="settings.cloudOn" :disabled="recording" @click="settings.cloudOn = !settings.cloudOn" />
        </label>
        <div class="fields">
          <label for="api-url">{{ t("apiUrl") }}</label>
          <input id="api-url" v-model="settings.apiUrl" class="text" type="url" :placeholder="t('apiUrlPlaceholder')" spellcheck="false" :disabled="recording" />
          <label for="api-token">{{ t("apiToken") }}</label>
          <input id="api-token" v-model="settings.apiToken" class="text" type="password" placeholder="dr_…" autocomplete="off" spellcheck="false" :disabled="recording" />
          <div class="test">
            <button class="ghost" :disabled="cloudTesting || !settings.apiUrl.trim() || !settings.apiToken.trim()" @click="testCloud">{{ cloudTesting ? t("testing") : t("testConnection") }}</button>
            <small v-if="cloudTest" :class="cloudTest.ok ? 'good' : 'bad'">{{ cloudTest.ok ? t("connectedAs", { name: cloudTest.text }) : native(cloudTest.text) }}</small>
          </div>
        </div>
      </details>

      <details class="card">
        <summary>{{ t("claude") }} <small>{{ claude?.connected ? t("claudeConnected") : t("claudeSummary") }}</small></summary>
        <div class="fields">
          <p class="about">{{ t("claudeAbout") }}</p>
          <div class="test">
            <button class="ghost" :disabled="claudeBusy || !settings.outDir" @click="connectClaude">{{ claude?.connected ? t("claudeReconnect") : t("claudeConnect") }}</button>
            <button class="ghost" :disabled="!claude" :title="t('claudeCopyTitle')" @click="copyClaudeCommand">{{ claudeCopied ? t("copied") : t("claudeCopy") }}</button>
          </div>
          <small v-if="claudeNote" :class="claudeNote.ok ? 'good' : 'bad'">{{ claudeNote.ok ? t("claudeDone") : native(claudeNote.text) }}</small>
          <small v-else-if="claude && !claude.desktop_found" class="muted">{{ t("claudeNeedsDesktop") }}</small>
          <small class="muted">{{ t("claudeExportsBefore") }} <code>exports</code> {{ t("claudeExportsAfter") }}</small>
        </div>
      </details>

      <details class="card">
        <summary>{{ t("recordings") }} <small>{{ recent.length || "" }}</small></summary>
        <div v-if="!recent.length" class="empty">{{ t("noRecordings") }}</div>
        <button v-for="m in recent" :key="m.id" class="recent" :title="m.dir" @click="api.openFolder(m.dir)">
          <svg v-if="isAudioOnly(m)" class="kind" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" :aria-label="t('modeAudio')"><rect x="9" y="2.5" width="6" height="11.5" rx="3" /><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3.5" /></svg>
          <svg v-else class="kind" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" :aria-label="t('modeVideo')"><rect x="2.5" y="6" width="13.5" height="12" rx="2.4" /><path d="m16 10.5 5.5-3v9l-5.5-3" /></svg>
          <span class="recent-main">{{ m.name || recordedAt(m) }}<br /><small>{{ m.name ? `${recordedAt(m)} · ` : "" }}{{ m.tracks.map((tr) => tr.file).join(" · ") }}</small></span>
          <small>{{ formatClock(m.duration_ms) }} · {{ formatSize(totalSize(m)) }}</small>
        </button>
      </details>
    </main>

    <footer class="bottom">
      <template v-if="!recording">
        <label class="name-field">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2.2h7a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-10Z" /></svg>
          <input
            v-model="recName"
            class="name-input"
            type="text"
            maxlength="80"
            spellcheck="false"
            :placeholder="t('namePlaceholder')"
            :aria-label="t('nameLabel')"
            :disabled="!!busy"
            @keydown.enter="start"
          />
        </label>
        <button class="rec-btn" :disabled="!!busy || !hasSource || !devices" @click="start">
          <span v-if="busy === 'starting'" class="spinner" />
          <i v-else />
          {{ busy === "starting" ? t("preparing") : audioMode ? t("recordAudio") : t("record") }}
        </button>
        <div v-if="recBlocker" class="hint warn">{{ recBlocker }}</div>
        <div v-else class="hint"><kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>R</kbd> {{ t("shortcutHint") }}</div>
      </template>
      <template v-else>
        <div class="live-bar">
          <div class="timer">
            <span class="dot" :class="{ paused: status.paused }" />
            {{ formatClock(status.elapsed_ms) }}
            <small v-if="status.paused">{{ t("onPause") }}</small>
            <small v-else-if="currentName" class="rec-name">{{ currentName }}</small>
          </div>
          <button class="round" :disabled="!!busy" :title="`${status.paused ? t('resume') : t('pause')} (Ctrl+Alt+P)`" @click="togglePause">
            <svg v-if="status.paused" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l12-7.5-12-7.5Z" /></svg>
            <svg v-else viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4.5" width="4.2" height="15" rx="1.2" /><rect x="13.8" y="4.5" width="4.2" height="15" rx="1.2" /></svg>
            {{ status.paused ? t("resume") : t("pause") }}
          </button>
          <button class="round stop" :disabled="!!busy" :title="`${t('stop')} (Ctrl+Alt+R)`" @click="stop">
            <span v-if="busy === 'stopping'" class="spinner" />
            <svg v-else viewBox="0 0 24 24" fill="currentColor"><rect x="5.5" y="5.5" width="13" height="13" rx="2.6" /></svg>
            {{ busy === "stopping" ? t("saving") : t("stop") }}
          </button>
        </div>
      </template>
    </footer>

    <div v-if="error" class="toast" role="alert">
      <span>{{ native(error) }}</span>
      <button :aria-label="t('closeNotice')" @click="error = ''">×</button>
    </div>
  </div>
</template>
