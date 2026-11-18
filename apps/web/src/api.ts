const BASE = (import.meta.env.VITE_API_URL ?? "http://127.0.0.1:8000").replace(/\/$/, "");
const TOKEN_KEY = "dualrec.token";

export interface Track {
  id: string;
  kind: "screen" | "camera" | "mic" | "system" | "composite";
  codec: string;
  width: number | null;
  height: number | null;
  offset_ms: number;
  size_bytes: number;
  uploaded: boolean;
  url: string | null;
  download_url: string | null;
}

export interface SharedVideo {
  title: string;
  status: "recording" | "uploading" | "processing" | "ready" | "failed";
  duration_ms: number;
  recorded_at: string;
  author: string;
  tracks: Track[];
}

export interface Video extends Omit<SharedVideo, "author"> {
  id: string;
  error: string | null;
  visibility: "link" | "private";
  share_token: string;
  share_url: string;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export const token = {
  get: () => localStorage.getItem(TOKEN_KEY) ?? "",
  set: (value: string) => localStorage.setItem(TOKEN_KEY, value.trim()),
  clear: () => localStorage.removeItem(TOKEN_KEY),
};

async function request<T>(path: string, init: RequestInit = {}, auth = true): Promise<T> {
  const headers = new Headers(init.headers);
  if (auth) headers.set("Authorization", `Bearer ${token.get()}`);
  if (init.body) headers.set("Content-Type", "application/json");
  let response: Response;
  try {
    response = await fetch(BASE + path, { ...init, headers });
  } catch {
    throw new ApiError(0, "No se pudo conectar con Dualrec. Revisá tu conexión.");
  }
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const detail = typeof body?.detail === "string" ? body.detail : `Error ${response.status}`;
    throw new ApiError(response.status, detail);
  }
  return response.status === 204 ? (undefined as T) : ((await response.json()) as T);
}

export const api = {
  shared: (shareToken: string) =>
    request<SharedVideo>(`/share/${encodeURIComponent(shareToken)}`, {}, false),
  me: () => request<{ name: string; email: string }>("/me"),
  videos: () => request<Video[]>("/videos"),
  update: (id: string, patch: { title?: string; visibility?: "link" | "private" }) =>
    request<Video>(`/videos/${id}`, { method: "PATCH", body: JSON.stringify(patch) }),
  remove: (id: string) => request<void>(`/videos/${id}`, { method: "DELETE" }),
};

export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function formatDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleString("es-AR", { dateStyle: "long", timeStyle: "short" });
}

export const STATUS_LABEL: Record<SharedVideo["status"], string> = {
  recording: "Grabando",
  uploading: "Subiendo",
  processing: "Procesando",
  ready: "Listo",
  failed: "Falló",
};
