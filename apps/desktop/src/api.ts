import { invoke } from "@tauri-apps/api/core";

export interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  refresh_rate: number;
  primary: boolean;
}
export interface WindowInfo {
  hwnd: number;
  title: string;
  process: string;
}
export interface CameraInfo {
  index: number;
  name: string;
}
export interface Devices {
  monitors: MonitorInfo[];
  windows: WindowInfo[];
  cameras: CameraInfo[];
  mics: string[];
  default_mic: string | null;
  system_output: string | null;
  preview_url: string;
  default_dir: string;
  ffmpeg_ok: boolean;
}
export interface CameraDesc {
  index: number;
  name: string;
  width: number;
  height: number;
  fps: number;
}
export interface MicDesc {
  name: string;
  rate: number;
  channels: number;
}
export type ScreenTarget = { kind: "monitor"; index: number } | { kind: "window"; hwnd: number };

export interface StartRequest {
  screen: ScreenTarget | null;
  camera: boolean;
  mic: boolean;
  system_audio: boolean;
  options: {
    out_dir: string;
    fps: number;
    quality: "alta" | "media" | "baja";
    max_height: number | null;
    cursor: boolean;
  };
}
export interface Track {
  kind: "screen" | "camera" | "mic" | "system";
  file: string;
  codec: string;
  source: string;
  width?: number;
  height?: number;
  fps?: number;
  sample_rate?: number;
  channels?: number;
  offset_ms: number;
  duration_ms: number;
  size_bytes: number;
  error?: string;
}
export interface Manifest {
  id: string;
  status: string;
  recorded_at: string;
  duration_ms: number;
  encoder: string;
  dir: string;
  tracks: Track[];
}
export interface Status {
  recording: boolean;
  paused: boolean;
  elapsed_ms: number;
  screen_frames: number;
  mic_peak: number;
  camera_active: boolean;
}

export const api = {
  devices: () => invoke<Devices>("get_devices"),
  encoder: () => invoke<string>("get_encoder"),
  previewUrl: () => invoke<string>("get_preview_url"),
  setCamera: (index: number | null) => invoke<CameraDesc | null>("set_camera", { index }),
  setMic: (enabled: boolean, name: string | null) =>
    invoke<MicDesc | null>("set_mic", { enabled, name }),
  start: (request: StartRequest) => invoke<Manifest>("start_recording", { request }),
  pause: () => invoke<void>("pause_recording"),
  resume: () => invoke<void>("resume_recording"),
  stop: () => invoke<Manifest>("stop_recording"),
  status: () => invoke<Status>("get_status"),
  recordings: (dir: string) => invoke<Manifest[]>("list_recordings", { dir }),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  setBubble: (visible: boolean) => invoke<void>("set_bubble", { visible }),
  setCaptureHidden: (hidden: boolean) => invoke<void>("set_capture_hidden", { hidden }),
};

export function formatClock(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
