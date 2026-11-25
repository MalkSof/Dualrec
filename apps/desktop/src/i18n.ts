import { ref, watch } from "vue";

export type Lang = "es" | "en";

const LANG_KEY = "dualrec.lang";

function stored(): Lang | null {
  try {
    const v = localStorage.getItem(LANG_KEY);
    return v === "es" || v === "en" ? v : null;
  } catch {
    return null;
  }
}

/** Idioma guardado; si no hay, inglés salvo que el sistema esté en español. */
function initial(): Lang {
  return stored() ?? (navigator.language.toLowerCase().startsWith("es") ? "es" : "en");
}

export const lang = ref<Lang>(initial());

watch(
  lang,
  (l) => {
    document.documentElement.lang = l;
    try {
      localStorage.setItem(LANG_KEY, l);
    } catch {
      /* sin almacenamiento: el idioma dura lo que dure la ventana */
    }
  },
  { immediate: true },
);

// La burbuja es otra ventana: se entera del cambio por el evento de almacenamiento.
window.addEventListener("storage", (e) => {
  if (e.key === LANG_KEY) lang.value = stored() ?? lang.value;
});

const es = {
  tagline: "Pantalla y cámara, en pistas separadas",
  screenPreviewAlt: "Vista previa de la pantalla",
  cameraPreviewAlt: "Vista previa de la cámara",
  paused: "PAUSA",
  saved: "Grabación guardada",
  savedIn: "Guardada en",
  namePlaceholder: "Nombre de la grabación (opcional)",
  nameLabel: "Nombre de la grabación y de su carpeta",
  needSource: "Activá al menos una fuente para grabar",
  openFolder: "Abrir carpeta",
  copied: "¡Copiado!",
  copyLink: "Copiar link",

  screen: "Pantalla",
  window: "Ventana",
  no: "No",
  screenToRecord: "Pantalla a grabar",
  windowToRecord: "Ventana a grabar",
  primary: " (principal)",
  noWindows: "No hay ventanas abiertas",
  camera: "Cámara",
  openingCamera: "Abriendo cámara…",
  recordCamera: "Grabar cámara",
  noCamera: "No se detectó ninguna cámara",
  mic: "Micrófono",
  recordMic: "Grabar micrófono",
  noMic: "No se detectó ningún micrófono",
  systemAudio: "Audio del sistema",
  recordSystem: "Grabar audio del sistema",

  settings: "Ajustes",
  settingsSummary: "{fps} fps · calidad {quality}",
  frames: "Cuadros",
  quality: "Calidad",
  qualityHigh: "Alta",
  qualityMedium: "Media",
  qualityLow: "Liviana",
  resolution: "Resolución",
  native: "Nativa",
  bubble: "Burbuja flotante de cámara",
  bubbleHint: "Te ves mientras grabás; se puede arrastrar",
  hideApp: "Ocultar Dualrec en la grabación",
  hideAppHint: "La app y la burbuja no salen en screen.mp4",
  cursor: "Mostrar el cursor",
  cursorHint: "Incluye el puntero del mouse en la pantalla",
  folder: "Carpeta de grabaciones",
  change: "Cambiar",
  language: "Idioma",

  cloud: "Nube",
  cloudOn: "activada",
  cloudNeedsSetup: "falta configurar",
  cloudOff: "desactivada",
  cloudUpload: "Subir a la nube mientras grabo",
  cloudUploadHint: "Al cortar, el link para compartir queda en el portapapeles",
  apiUrl: "Dirección de Dualrec",
  apiUrlPlaceholder: "https://api.tu-dominio.com",
  apiToken: "Token de acceso",
  testing: "Probando…",
  testConnection: "Probar conexión",
  connectedAs: "Conectado como {name}",
  cloudConnecting: "Conectando con la nube…",
  cloudUploading: "Subiendo · {progress}",
  cloudOffline: "Sin conexión · se retoma sola",
  cloudFinishing: "Terminando la subida…",
  cloudDone: "En la nube",
  cloudFailed: "No se pudo subir",

  claude: "Claude (IA)",
  claudeConnected: "conectado",
  claudeSummary: "armá videos y reels",
  claudeAbout:
    'Conectá Dualrec con Claude y pedile en palabras que arme el video: "hacé un reel vertical con la mejor parte de mi última grabación" o "sacá los silencios y dejá la cámara en una burbuja".',
  claudeReconnect: "Volver a conectar",
  claudeConnect: "Conectar con Claude Desktop",
  claudeCopyTitle: "Copia el comando para sumarlo en Claude Code",
  claudeCopy: "Copiar comando para Claude Code",
  claudeDone: "Listo. Cerrá Claude del todo (también desde la bandeja) y volvé a abrirlo.",
  claudeNeedsDesktop: "Necesitás la app de Claude para escritorio (claude.ai/download).",
  claudeExportsBefore: "Los videos que arma quedan en la subcarpeta",
  claudeExportsAfter: "de cada grabación.",

  recordings: "Grabaciones",
  noRecordings: "Todavía no hay grabaciones en esta carpeta.",

  preparing: "Preparando…",
  record: "Grabar",
  shortcutHint: "para grabar o cortar desde cualquier app",
  onPause: "en pausa",
  resume: "Reanudar",
  pause: "Pausar",
  stop: "Cortar",
  saving: "Guardando…",
  closeNotice: "Cerrar aviso",

  searchingDevices: "Buscando dispositivos…",
  noScreen: "Sin pantalla",
  pickWindow: "Elegí una ventana",
  onlyCamAudio: "Solo cámara y audio",
  stereo: "estéreo",
  noFfmpeg: "No se encontró FFmpeg. Reinstalá Dualrec o instalá ffmpeg y agregalo al PATH.",
  pickSource: "Elegí al menos una fuente para grabar.",
  bubbleTitle: "Arrastrá para mover · doble clic para cambiar el tamaño",
};

type Key = keyof typeof es;

const en: Record<Key, string> = {
  tagline: "Screen and camera, on separate tracks",
  screenPreviewAlt: "Screen preview",
  cameraPreviewAlt: "Camera preview",
  paused: "PAUSED",
  saved: "Recording saved",
  savedIn: "Saved in",
  namePlaceholder: "Recording name (optional)",
  nameLabel: "Name of the recording and its folder",
  needSource: "Turn on at least one source to record",
  openFolder: "Open folder",
  copied: "Copied!",
  copyLink: "Copy link",

  screen: "Screen",
  window: "Window",
  no: "Off",
  screenToRecord: "Screen to record",
  windowToRecord: "Window to record",
  primary: " (primary)",
  noWindows: "No open windows",
  camera: "Camera",
  openingCamera: "Opening camera…",
  recordCamera: "Record camera",
  noCamera: "No camera detected",
  mic: "Microphone",
  recordMic: "Record microphone",
  noMic: "No microphone detected",
  systemAudio: "System audio",
  recordSystem: "Record system audio",

  settings: "Settings",
  settingsSummary: "{fps} fps · {quality} quality",
  frames: "Frame rate",
  quality: "Quality",
  qualityHigh: "High",
  qualityMedium: "Medium",
  qualityLow: "Light",
  resolution: "Resolution",
  native: "Native",
  bubble: "Floating camera bubble",
  bubbleHint: "See yourself while you record; drag it anywhere",
  hideApp: "Hide Dualrec from the recording",
  hideAppHint: "The app and the bubble stay out of screen.mp4",
  cursor: "Show the cursor",
  cursorHint: "Includes the mouse pointer in the screen video",
  folder: "Recordings folder",
  change: "Change",
  language: "Language",

  cloud: "Cloud",
  cloudOn: "on",
  cloudNeedsSetup: "needs setup",
  cloudOff: "off",
  cloudUpload: "Upload to the cloud while recording",
  cloudUploadHint: "When you stop, the share link is copied to the clipboard",
  apiUrl: "Dualrec address",
  apiUrlPlaceholder: "https://api.your-domain.com",
  apiToken: "Access token",
  testing: "Testing…",
  testConnection: "Test connection",
  connectedAs: "Connected as {name}",
  cloudConnecting: "Connecting to the cloud…",
  cloudUploading: "Uploading · {progress}",
  cloudOffline: "Offline · resumes on its own",
  cloudFinishing: "Finishing the upload…",
  cloudDone: "In the cloud",
  cloudFailed: "Upload failed",

  claude: "Claude (AI)",
  claudeConnected: "connected",
  claudeSummary: "make videos and Reels",
  claudeAbout:
    'Connect Dualrec to Claude and ask for the video in plain words: "make a vertical Reel from the best part of my latest recording" or "cut the silences and keep the camera in a bubble".',
  claudeReconnect: "Reconnect",
  claudeConnect: "Connect to Claude Desktop",
  claudeCopyTitle: "Copies the command that adds it to Claude Code",
  claudeCopy: "Copy Claude Code command",
  claudeDone: "Done. Quit Claude completely (from the tray too) and open it again.",
  claudeNeedsDesktop: "You need the Claude desktop app (claude.ai/download).",
  claudeExportsBefore: "The videos it makes are saved in the",
  claudeExportsAfter: "subfolder of each recording.",

  recordings: "Recordings",
  noRecordings: "No recordings in this folder yet.",

  preparing: "Getting ready…",
  record: "Record",
  shortcutHint: "to record or stop from any app",
  onPause: "paused",
  resume: "Resume",
  pause: "Pause",
  stop: "Stop",
  saving: "Saving…",
  closeNotice: "Dismiss",

  searchingDevices: "Looking for devices…",
  noScreen: "No screen",
  pickWindow: "Pick a window",
  onlyCamAudio: "Camera and audio only",
  stereo: "stereo",
  noFfmpeg: "FFmpeg was not found. Reinstall Dualrec, or install ffmpeg and add it to PATH.",
  pickSource: "Pick at least one source to record.",
  bubbleTitle: "Drag to move · double-click to resize",
};

const dicts: Record<Lang, Record<Key, string>> = { es, en };

/** Texto de la interfaz en el idioma elegido. Es reactivo: depende de `lang`. */
export function t(key: Key, params?: Record<string, string | number>): string {
  const text = dicts[lang.value][key];
  return params ? text.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m)) : text;
}

/** Código de región para fechas. */
export function locale(): string {
  return lang.value === "es" ? "es-AR" : "en-US";
}

// Los mensajes que arma el lado nativo salen en español. Acá se pasan al inglés
// los que puede ver el usuario; uno que no esté en la lista se muestra tal cual.
const NATIVE: [RegExp, string][] = [
  [/No se puede cambiar la cámara mientras se graba\./, "The camera cannot be changed while recording."],
  [/No se puede cambiar el micrófono mientras se graba\./, "The microphone cannot be changed while recording."],
  [/Ya hay una grabación en curso\./, "A recording is already in progress."],
  [/No hay una grabación en curso\./, "There is no recording in progress."],
  [/Elegí una cámara antes de grabar\./, "Pick a camera before recording."],
  [/Elegí un micrófono antes de grabar\./, "Pick a microphone before recording."],
  [/Elegí al menos una fuente para grabar\./, "Pick at least one source to record."],
  [/No se encontró FFmpeg\. Reinstalá Dualrec o instalá ffmpeg y agregalo al PATH\./, en.noFfmpeg],
  [/No se pudo iniciar FFmpeg \((.*)\)\. Verificá que ffmpeg esté instalado junto a Dualrec\./, "FFmpeg could not start ($1). Check that ffmpeg is installed next to Dualrec."],
  [/No se pudo ejecutar FFmpeg: /, "FFmpeg could not run: "],
  [/No se pudo crear la carpeta /, "Could not create the folder "],
  [/La cámara todavía no entregó imagen\./, "The camera has not delivered an image yet."],
  [
    /La cámara no está entregando imagen\. Suele pasar cuando otra app la está usando\s+\(OBS, Teams, el navegador\), cuando la tapa de la notebook está cerrada o cuando\s+el obturador de privacidad está activado\./,
    "The camera is not delivering an image. This usually happens when another app is using it (OBS, Teams, the browser), when the laptop lid is closed or when the privacy shutter is on.",
  ],
  [
    /No se pudo abrir la cámara \((.*)\)\. Revisá que no la esté usando otra app y que Windows\s+permita el acceso en Configuración → Privacidad y seguridad → Cámara\./,
    "Could not open the camera ($1). Check that no other app is using it and that Windows allows access in Settings → Privacy & security → Camera.",
  ],
  [/La cámara no respondió a tiempo\./, "The camera did not respond in time."],
  [/El dispositivo de audio no respondió a tiempo\./, "The audio device did not respond in time."],
  [/No se pudieron listar los micrófonos: /, "Could not list the microphones: "],
  [/No hay ningún micrófono conectado\./, "No microphone is connected."],
  [
    /No se pudo abrir el micrófono \((.*)\)\. Revisá Configuración → Privacidad y\s+seguridad → Micrófono\./,
    "Could not open the microphone ($1). Check Settings → Privacy & security → Microphone.",
  ],
  [/No hay un dispositivo de salida de audio\./, "There is no audio output device."],
  [/No se pudo leer el audio del sistema: /, "Could not read the system audio: "],
  [/No se pudo abrir el audio de «(.*?)»: /, "Could not open the audio of “$1”: "],
  [/No se pudo iniciar el audio de «(.*?)»: /, "Could not start the audio of “$1”: "],
  [/No se pudo iniciar la captura de pantalla: /, "Could not start the screen capture: "],
  [/No se pudieron listar las pantallas: /, "Could not list the screens: "],
  [/La pantalla elegida ya no está conectada\./, "The chosen screen is no longer connected."],
  [/La ventana se cerró antes de empezar a grabar\./, "The window was closed before recording started."],
  [
    /La pantalla no entregó imagen\. Si elegiste una ventana, fijate que no esté minimizada\./,
    "The screen did not deliver an image. If you picked a window, check that it is not minimized.",
  ],
  [/ffmpeg terminó con error: /, "ffmpeg ended with an error: "],
  [/se cortó la escritura de video \(/, "the video write was interrupted ("],
  [/se cortó la escritura de audio \(/, "the audio write was interrupted ("],
  [/el hilo de grabación terminó de forma inesperada/, "the recording thread ended unexpectedly"],
  [/no se generó el archivo/, "the file was not created"],
  [/El token de acceso no es válido\. Revisalo en Ajustes → Nube\./, "The access token is not valid. Check it in Settings → Cloud."],
  [/la API respondió /, "the API answered "],
  [/sin conexión \(/, "no connection ("],
  [/la API no devolvió el id del video/, "the API did not return the video id"],
  [/la API no devolvió la URL de subida/, "the API did not return the upload URL"],
  [/el almacenamiento respondió /, "the storage answered "],
  [/el almacenamiento no devolvió ETag/, "the storage did not return an ETag"],
  [
    /No encontré Claude Desktop en esta computadora\. Instalalo desde claude\.ai\/download, abrilo una vez y volvé a intentar\./,
    "Claude Desktop was not found on this computer. Install it from claude.ai/download, open it once and try again.",
  ],
  [/El archivo (.*) tiene un formato que no pude leer; no lo modifiqué\./, "The file $1 has a format that could not be read; it was not modified."],
  [/El archivo (.*) tiene un formato inesperado; no lo modifiqué\./, "The file $1 has an unexpected format; it was not modified."],
  [/No se pudo guardar (.*): /, "Could not save $1: "],
  [/^Pantalla (\d+)$/, "Screen $1"],
  [/^Ventana$/, "Window"],
  [/^Cámara de prueba$/, "Test camera"],
];

/** Pasa al idioma elegido un mensaje o nombre que viene del lado nativo. */
export function native(text: string): string {
  if (lang.value === "es") return text;
  return NATIVE.reduce((s, [re, to]) => s.replace(re, to), text);
}
