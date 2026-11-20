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
  screen_preview_url: string;
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
  cloud: CloudConfig | null;
  title: string | null;
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
export interface CloudConfig {
  api_url: string;
  token: string;
}
export interface CloudProgress {
  dir: string;
  state: "connecting" | "uploading" | "offline" | "finishing" | "done" | "failed";
  uploaded_bytes: number;
  total_bytes: number;
  share_url: string | null;
  error: string | null;
}
export interface Status {
  recording: boolean;
  paused: boolean;
  elapsed_ms: number;
  screen_frames: number;
  mic_peak: number;
  camera_active: boolean;
  cloud: CloudProgress | null;
  uploads_active: number;
}

export const api = {
  devices: () => invoke<Devices>("get_devices"),
  encoder: () => invoke<string>("get_encoder"),
  previewUrl: () => invoke<string>("get_preview_url"),
  setScreenPreview: (target: ScreenTarget | null) => invoke<void>("set_screen_preview", { target }),
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
  copyText: (text: string) => invoke<boolean>("copy_text", { text }),
  cloudDefaults: () => invoke<[string, string] | null>("cloud_defaults"),
  cloudCheck: (cloud: CloudConfig) => invoke<string>("cloud_check", { cloud }),
  resumeUploads: (cloud: CloudConfig, dir: string) =>
    invoke<number>("resume_uploads", { cloud, dir }),
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
