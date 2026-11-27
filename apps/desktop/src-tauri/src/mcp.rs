//! Servidor MCP (Model Context Protocol) por stdio: `dualrec.exe --mcp`.
//!
//! Le da a un asistente (Claude Desktop, Claude Code u otro cliente MCP) acceso
//! a las grabaciones de Dualrec para armar videos terminados: lista las
//! grabaciones, detecta dónde se habla, mira cuadros de la pantalla y la cámara
//! y exporta videos horizontales o reels verticales combinando las pistas.
//!
//! El protocolo es JSON-RPC 2.0, un mensaje por línea. Por stdout solo salen
//! respuestas del protocolo.

use crate::ffmpeg;
use crate::recorder::{self, Manifest};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FPS: u32 = 30;
/// Tiempo que `render` espera antes de devolver "sigue en curso".
const RENDER_WAIT: Duration = Duration::from_secs(40);

type Seg = (f64, f64);

fn ff_cmd() -> Command {
    let mut cmd = Command::new(ffmpeg::locate());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdin(Stdio::null());
    cmd
}

fn ff_output(args: &[String], cwd: Option<&Path>) -> Result<Output, String> {
    let mut cmd = ff_cmd();
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.output().map_err(|e| format!("No se pudo ejecutar FFmpeg: {e}"))
}

// ---------- Utilidades puras ----------

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Ordena, recorta a `[0, dur]` y une los tramos que se tocan.
pub fn normalize(mut segs: Vec<Seg>, dur: f64) -> Vec<Seg> {
    segs.retain(|s| s.0.is_finite() && s.1.is_finite());
    for s in &mut segs {
        s.0 = s.0.clamp(0.0, dur);
        s.1 = s.1.clamp(0.0, dur);
    }
    segs.retain(|s| s.1 - s.0 > 0.02);
    segs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Seg> = Vec::new();
    for s in segs {
        match out.last_mut() {
            Some(last) if s.0 <= last.1 + 1e-6 => last.1 = last.1.max(s.1),
            _ => out.push(s),
        }
    }
    out
}

pub fn intersect(a: &[Seg], b: &[Seg]) -> Vec<Seg> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let (lo, hi) = (x.0.max(y.0), x.1.min(y.1));
            if hi - lo > 0.02 {
                out.push((lo, hi));
            }
        }
    }
    out
}

/// Tramos con voz a partir de los silencios detectados: agrega un margen,
/// une pausas cortas y descarta ruidos sueltos.
pub fn speech_from_silences(silences: &[Seg], dur: f64) -> Vec<Seg> {
    let sil = normalize(silences.to_vec(), dur);
    let mut speech = Vec::new();
    let mut t = 0.0;
    for s in &sil {
        if s.0 > t {
            speech.push((t, s.0));
        }
        t = s.1;
    }
    if dur > t {
        speech.push((t, dur));
    }
    speech.retain(|s| s.1 - s.0 >= 0.2);
    let padded: Vec<Seg> = speech.iter().map(|s| (s.0 - 0.12, s.1 + 0.15)).collect();
    let mut out: Vec<Seg> = Vec::new();
    for s in normalize(padded, dur) {
        match out.last_mut() {
            Some(last) if s.0 - last.1 < 0.3 => last.1 = s.1,
            _ => out.push(s),
        }
    }
    out
}

/// Pasa un intervalo de la grabación original a la línea de tiempo del video
/// exportado (después de los cortes).
pub fn map_interval(kept: &[Seg], iv: Seg) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut acc = 0.0;
    for s in kept {
        let (lo, hi) = (iv.0.max(s.0), iv.1.min(s.1));
        if hi - lo > 0.05 {
            out.push((acc + lo - s.0, acc + hi - s.0));
        }
        acc += s.1 - s.0;
    }
    out
}

/// Máscara en formato PGM (grises): 255 adentro de la forma, 0 afuera.
pub fn mask_pgm(d: u32, shape: &str) -> Vec<u8> {
    let mut out = format!("P5\n{d} {d}\n255\n").into_bytes();
    let half = d as f64 / 2.0;
    let rr = if shape == "rounded" { d as f64 * 0.2 } else { 0.0 };
    for y in 0..d {
        for x in 0..d {
            let (px, py) = (x as f64 + 0.5 - half, y as f64 + 0.5 - half);
            let dist = match shape {
                "circle" => (px * px + py * py).sqrt() - half,
                "rounded" => {
                    let (qx, qy) = (px.abs() - (half - rr), py.abs() - (half - rr));
                    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rr
                }
                _ => -1.0,
            };
            out.push(((0.5 - dist).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

fn ass_time(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

fn ass_text(s: &str) -> String {
    s.replace(['{', '}'], "").replace("\r\n", "\n").replace('\n', "\\N").trim().to_string()
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32).max(1) * 2
}

fn safe_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    n.trim_matches('-').chars().take(60).collect()
}

// ---------- Grabaciones ----------

struct Rec {
    manifest: Manifest,
    dir: PathBuf,
}

impl Rec {
    fn track(&self, kind: &str) -> Option<PathBuf> {
        let t = self.manifest.tracks.iter().find(|t| t.kind == kind && t.error.is_none())?;
        let p = self.dir.join(&t.file);
        p.is_file().then_some(p)
    }
    fn duration(&self) -> f64 {
        self.manifest.duration_ms as f64 / 1000.0
    }
}

fn folder_arg(args: &Value, default_dir: &Path) -> PathBuf {
    args.get("folder")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_dir.to_path_buf())
}

fn find_rec(args: &Value, default_dir: &Path) -> Result<Rec, String> {
    let folder = folder_arg(args, default_dir);
    let all = recorder::list_recordings(&folder);
    let id = args.get("id").and_then(Value::as_str).unwrap_or("latest");
    let manifest = if id == "latest" || id.is_empty() {
        all.into_iter().find(|m| m.status != "recording")
    } else {
        all.into_iter().find(|m| m.id == id)
    }
    .ok_or_else(|| format!("No hay ninguna grabación \"{id}\" en {}. Usá list_recordings para ver las disponibles.", folder.display()))?;
    if manifest.status == "recording" {
        return Err("Esa grabación todavía está en curso. Cortala en Dualrec y volvé a intentar.".into());
    }
    let dir = PathBuf::from(&manifest.dir);
    Ok(Rec { manifest, dir })
}

fn exports_of(dir: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("exports")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "mp4") {
                out.push(json!({
                    "file": p.to_string_lossy(),
                    "size_mb": (e.metadata().map(|m| m.len()).unwrap_or(0) as f64 / 104_857.6).round() / 10.0,
                }));
            }
        }
    }
    out
}

fn summary(m: &Manifest) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "recorded_at": m.recorded_at,
        "duration_seconds": m.duration_ms as f64 / 1000.0,
        "status": m.status,
        "audio_only": !m.tracks.iter().any(|t| t.width.is_some()),
        "folder": m.dir,
        "tracks": m.tracks.iter().map(|t| json!({
            "kind": t.kind, "file": t.file, "source": t.source,
            "width": t.width, "height": t.height, "ok": t.error.is_none(),
        })).collect::<Vec<_>>(),
        "exports": exports_of(Path::new(&m.dir)),
    })
}

fn detect_speech(rec: &Rec, db: f64) -> Result<Vec<Seg>, String> {
    let audio = rec
        .track("mic")
        .or_else(|| rec.track("system"))
        .ok_or("La grabación no tiene pista de audio para detectar la voz.")?;
    let args: Vec<String> = [
        "-hide_banner", "-nostats", "-i", &audio.to_string_lossy(),
        "-af", &format!("silencedetect=noise={db}dB:d=0.45"), "-f", "null", "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let out = ff_output(&args, None)?;
    let log = String::from_utf8_lossy(&out.stderr);
    let dur = rec.duration();
    let mut silences = Vec::new();
    let mut start: Option<f64> = None;
    let num = |line: &str, key: &str| -> Option<f64> {
        let rest = &line[line.find(key)? + key.len()..];
        rest.split_whitespace().next()?.parse().ok()
    };
    for line in log.lines() {
        if let Some(s) = num(line, "silence_start: ") {
            start = Some(s);
        } else if let Some(e) = num(line, "silence_end: ") {
            silences.push((start.take().unwrap_or(0.0), e));
        }
    }
    if let Some(s) = start {
        silences.push((s, dur));
    }
    Ok(speech_from_silences(&silences, dur))
}

// ---------- Exportación ----------

#[derive(Clone)]
enum JobState {
    Running,
    Done(Value),
    Failed(String),
}

struct Job {
    state: Mutex<JobState>,
    progress: PathBuf,
    total: f64,
}

#[derive(Default)]
struct Ctx {
    default_dir: PathBuf,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

fn region(input: &str, out: &str, w: u32, h: u32, cover: bool, zoom: f64, fx: f64, fy: f64) -> String {
    let crop = if zoom > 1.001 {
        // Con zoom se recorta con la proporción del destino, así el recuadro se llena.
        format!("crop=w=iw/{zoom:.3}:h='min(ih,iw/{zoom:.3}*{h}/{w})':x=(iw-ow)*{fx:.3}:y=(ih-oh)*{fy:.3},")
    } else {
        String::new()
    };
    if cover {
        format!("[{input}]{crop}scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1[{out}]")
    } else {
        format!(
            "[{input}]{crop}split[{out}x][{out}y];\
             [{out}x]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},boxblur=24:4,eq=brightness=-0.14[{out}b];\
             [{out}y]scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2[{out}f];\
             [{out}b][{out}f]overlay=(W-w)/2:(H-h)/2,setsar=1[{out}]"
        )
    }
}

fn num_arg(args: &Value, key: &str, default: f64, lo: f64, hi: f64) -> f64 {
    args.get(key).and_then(Value::as_f64).unwrap_or(default).clamp(lo, hi)
}

fn str_arg<'a>(args: &'a Value, key: &str, default: &'a str) -> &'a str {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(default)
}

/// Color en hexadecimal (`#8b7cff` o `8b7cff`), devuelto sin el numeral.
pub fn color_arg(args: &Value, key: &str, default: &str) -> Result<String, String> {
    let raw = str_arg(args, key, default).trim().trim_start_matches('#');
    if raw.len() == 6 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(raw.to_ascii_lowercase())
    } else {
        Err(format!("{key} tiene que ser un color en hexadecimal, por ejemplo #8b7cff."))
    }
}

fn segs_arg(args: &Value, key: &str) -> Result<Option<Vec<Seg>>, String> {
    let Some(list) = args.get(key).and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for s in list {
        let (a, b) = (s.get("start").and_then(Value::as_f64), s.get("end").and_then(Value::as_f64));
        match (a, b) {
            (Some(a), Some(b)) if b > a => out.push((a, b)),
            _ => return Err(format!("Cada elemento de {key} necesita start y end en segundos, con end mayor que start.")),
        }
    }
    Ok(Some(out))
}

struct Plan {
    args: Vec<String>,
    dir: PathBuf,
    out: PathBuf,
    temp: Vec<PathBuf>,
    progress: PathBuf,
    total: f64,
    info: Value,
}

fn plan_render(rec: &Rec, a: &Value) -> Result<Plan, String> {
    let dur = rec.duration();
    let screen = rec.track("screen");
    let camera = rec.track("camera");
    let voice = rec.track("mic").or_else(|| rec.track("system"));
    if screen.is_none() && camera.is_none() && voice.is_none() {
        return Err("La grabación no tiene ninguna pista que se pueda usar.".into());
    }
    let format = str_arg(a, "format", "horizontal");
    let (w, h) = match format {
        "horizontal" => (1920u32, 1080u32),
        "vertical" => (1080, 1920),
        "square" => (1080, 1080),
        _ => return Err("format tiene que ser horizontal, vertical o square.".into()),
    };
    let default_layout = if format == "horizontal" { "pip" } else { "stack" };
    let mut layout = str_arg(a, "layout", default_layout).to_string();
    if !["pip", "stack", "side", "screen", "camera", "audio"].contains(&layout.as_str()) {
        return Err("layout tiene que ser pip, stack, side, screen, camera o audio.".into());
    }
    if screen.is_none() && camera.is_none() {
        // Grabación de solo audio: se arma un video con fondo y la onda de la voz.
        layout = "audio".into();
    } else if layout == "audio" {
        if voice.is_none() {
            return Err("La grabación no tiene audio para armar un video de solo audio.".into());
        }
    } else if camera.is_none() {
        layout = "screen".into();
    } else if screen.is_none() {
        layout = "camera".into();
    }

    // Qué tramos entran
    let mut kept = match segs_arg(a, "segments")? {
        Some(s) => normalize(s, dur),
        None => vec![(0.0, dur)],
    };
    if a.get("remove_silences").and_then(Value::as_bool).unwrap_or(false) {
        let speech = detect_speech(rec, num_arg(a, "silence_db", -35.0, -70.0, -10.0))?;
        kept = normalize(intersect(&kept, &speech), dur);
    }
    if kept.is_empty() {
        return Err("No queda nada para exportar con esos tramos.".into());
    }
    let total: f64 = kept.iter().map(|s| s.1 - s.0).sum();
    let base = kept[0].0;
    let span = kept.last().unwrap().1 - base;

    let name = match a.get("output_name").and_then(Value::as_str).map(safe_name) {
        Some(n) if !n.is_empty() => n,
        _ => format!("{format}-{}", chrono::Local::now().format("%H%M%S")),
    };
    let dir = rec.dir.join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta exports: {e}"))?;
    let out = dir.join(format!("{name}.mp4"));
    let mut temp = Vec::new();
    let mut tmp = |suffix: &str, data: &[u8]| -> Result<String, String> {
        let file = format!(".{name}.{suffix}");
        std::fs::write(dir.join(&file), data).map_err(|e| format!("No se pudo escribir {file}: {e}"))?;
        temp.push(dir.join(&file));
        Ok(file)
    };

    // Entradas
    let mut args: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error", "-nostats"].iter().map(|s| s.to_string()).collect();
    let progress_file = format!(".{name}.progress");
    args.extend(["-progress".into(), progress_file.clone()]);
    let n_inputs = std::cell::Cell::new(0usize);
    let add_media = |args: &mut Vec<String>, p: &Path| -> usize {
        args.extend(["-ss".into(), format!("{base:.3}"), "-t".into(), format!("{span:.3}"), "-i".into(), p.to_string_lossy().into_owned()]);
        n_inputs.set(n_inputs.get() + 1);
        n_inputs.get() - 1
    };
    let use_screen = !["camera", "audio"].contains(&layout.as_str());
    let use_camera = !["screen", "audio"].contains(&layout.as_str());
    let i_screen = screen.as_deref().filter(|_| use_screen).map(|p| add_media(&mut args, p));
    let i_camera = camera.as_deref().filter(|_| use_camera).map(|p| add_media(&mut args, p));
    let mic_vol = num_arg(a, "mic_volume", 1.0, 0.0, 4.0);
    let sys_vol = num_arg(a, "system_volume", 0.6, 0.0, 4.0);
    let i_mic = rec.track("mic").filter(|_| mic_vol > 0.0).map(|p| add_media(&mut args, &p));
    let i_sys = rec.track("system").filter(|_| sys_vol > 0.0).map(|p| add_media(&mut args, &p));

    // Video
    let zoom = num_arg(a, "screen_zoom", 1.0, 1.0, 4.0);
    let (fx, fy) = (num_arg(a, "screen_focus_x", 0.5, 0.0, 1.0), num_arg(a, "screen_focus_y", 0.5, 0.0, 1.0));
    let mut g: Vec<String> = Vec::new();
    if let Some(i) = i_screen {
        g.push(format!("[{i}:v]fps={FPS}[sv]"));
    }
    if let Some(i) = i_camera {
        g.push(format!("[{i}:v]fps={FPS}[cv]"));
    }
    let mut seam: Option<u32> = None;
    match layout.as_str() {
        "audio" => {
            // Fondo: la imagen que se pase o un degradé que se mueve despacio.
            let i_bg = match a.get("background_image").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                Some(img) => {
                    let img = Path::new(img.trim());
                    if !img.is_file() {
                        return Err(format!("No encontré la imagen de fondo {}.", img.display()));
                    }
                    args.extend(["-loop".into(), "1".into(), "-framerate".into(), FPS.to_string(), "-t".into(), format!("{span:.3}"), "-i".into(), img.to_string_lossy().into_owned()]);
                    n_inputs.set(n_inputs.get() + 1);
                    g.push(region(&format!("{}:v", n_inputs.get() - 1), "bg", w, h, true, 1.0, 0.5, 0.5));
                    None
                }
                None => {
                    let c0 = color_arg(a, "background_color", "1b1638")?;
                    args.extend([
                        "-f".into(), "lavfi".into(), "-t".into(), format!("{span:.3}"), "-i".into(),
                        format!("gradients=s={w}x{h}:r={FPS}:c0=0x{c0}:c1=0x0b0d12:x0=0:y0=0:x1={w}:y1={h}:speed=0.004"),
                    ]);
                    n_inputs.set(n_inputs.get() + 1);
                    Some(n_inputs.get() - 1)
                }
            };
            if let Some(i) = i_bg {
                g.push(format!("[{i}:v]format=yuv420p,setsar=1[bg]"));
            }
            match voice.as_deref() {
                Some(p) if a.get("waveform").and_then(Value::as_bool).unwrap_or(true) => {
                    let i = add_media(&mut args, p);
                    let wave = color_arg(a, "wave_color", "8b7cff")?;
                    let ww = even(w as f64 * 0.84);
                    let wh = even(h as f64 * if format == "vertical" { 0.16 } else { 0.3 });
                    let y = even(h as f64 * if format == "vertical" { 0.42 } else { 0.5 } - wh as f64 / 2.0);
                    g.push(format!(
                        "[{i}:a]aformat=channel_layouts=mono,showwaves=s={ww}x{wh}:mode=cline:n=1:scale=sqrt:colors=0x{wave},format=rgba[wv]"
                    ));
                    g.push(format!("[bg][wv]overlay=(W-w)/2:{y}:shortest=1[v0]"));
                }
                _ => g.push("[bg]null[v0]".into()),
            }
        }
        "screen" => g.push(region("sv", "v0", w, h, false, zoom, fx, fy)),
        "camera" => g.push(region("cv", "v0", w, h, true, 1.0, 0.5, 0.5)),
        "stack" => {
            let top = even(h as f64 * if format == "vertical" { 0.5 } else { 0.56 });
            g.push(region("sv", "top", w, top, false, zoom, fx, fy));
            g.push(region("cv", "bot", w, h - top, true, 1.0, 0.5, 0.5));
            g.push("[top][bot]vstack[v0]".into());
            seam = Some(top);
        }
        "side" => {
            let left = even(w as f64 * if format == "horizontal" { 0.68 } else { 0.5 });
            g.push(region("sv", "lft", left, h, false, zoom, fx, fy));
            g.push(region("cv", "rgt", w - left, h, true, 1.0, 0.5, 0.5));
            g.push("[lft][rgt]hstack[v0]".into());
        }
        _ => {
            // pip: pantalla de fondo y la cámara en una burbuja
            g.push(region("sv", "base", w, h, false, zoom, fx, fy));
            let short = w.min(h) as f64;
            let d = even(short * num_arg(a, "camera_size", 28.0, 10.0, 60.0) / 100.0);
            let border = even((d as f64 / 45.0).max(4.0));
            let shape = str_arg(a, "camera_shape", "circle");
            let m = (short * 0.04) as u32;
            let bottom = if format == "vertical" { (h as f64 * 0.16) as u32 } else { m };
            let corner = str_arg(a, "camera_position", if format == "vertical" { "bottom-center" } else { "bottom-right" });
            let x = if corner.ends_with("left") { m } else if corner.ends_with("center") { (w - d) / 2 } else { w - d - m };
            let y = if corner.starts_with("top") { m + if format == "vertical" { (h as f64 * 0.08) as u32 } else { 0 } } else { h - d - bottom };
            let mask = tmp("mask.pgm", &mask_pgm(d, shape))?;
            let ring = tmp("ring.pgm", &mask_pgm(d + 2 * border, shape))?;
            args.extend(["-i".into(), mask, "-i".into(), ring]);
            let (i_mask, i_ring) = (n_inputs.get(), n_inputs.get() + 1);
            g.push(format!("[cv]scale={d}:{d}:force_original_aspect_ratio=increase,crop={d}:{d},format=yuva420p[cq]"));
            g.push(format!("[{i_mask}:v]format=gray[mk]"));
            g.push("[cq][mk]alphamerge[cb]".into());
            g.push(format!("[{i_ring}:v]format=gray,split[r1][r2]"));
            g.push("[r1]format=yuva420p,lutyuv=y=235:u=128:v=128[r1w]".into());
            g.push("[r1w][r2]alphamerge[rg]".into());
            g.push(format!("[base][rg]overlay={}:{}[b1]", x.saturating_sub(border), y.saturating_sub(border)));
            g.push(format!("[b1][cb]overlay={x}:{y}[v0]"));
        }
    }

    // Cortes
    let select: Option<String> = (kept.len() > 1).then(|| {
        kept.iter().map(|s| format!("between(t,{:.3},{:.3})", s.0 - base, s.1 - base)).collect::<Vec<_>>().join("+")
    });
    let mut v = "v0".to_string();
    if let Some(expr) = &select {
        g.push(format!("[{v}]select='{expr}',setpts=N/{FPS}/TB[v1]"));
        v = "v1".into();
    }

    // Título y subtítulos
    let title = a.get("title").and_then(Value::as_str).map(ass_text).filter(|s| !s.is_empty());
    let mut events = String::new();
    if let Some(t) = &title {
        let secs = num_arg(a, "title_seconds", if layout == "audio" { total } else { 3.5 }, 0.5, 36_000.0).min(total);
        events.push_str(&format!("Dialogue: 0,{},{},Title,,0,0,0,,{t}\n", ass_time(0.0), ass_time(secs)));
    }
    let mut n_caps = 0;
    if let Some(caps) = a.get("captions").and_then(Value::as_array) {
        for c in caps {
            let (Some(s), Some(e), Some(text)) = (
                c.get("start").and_then(Value::as_f64),
                c.get("end").and_then(Value::as_f64),
                c.get("text").and_then(Value::as_str),
            ) else {
                return Err("Cada subtítulo necesita start, end (segundos de la grabación original) y text.".into());
            };
            let text = ass_text(text);
            if text.is_empty() {
                continue;
            }
            for (os, oe) in map_interval(&kept, (s, e)) {
                events.push_str(&format!("Dialogue: 1,{},{},Cap,,0,0,0,,{text}\n", ass_time(os), ass_time(oe)));
                n_caps += 1;
            }
        }
    }
    if !events.is_empty() {
        let vertical = format != "horizontal";
        let cap_size = (h as f64 * if vertical { 0.04 } else { 0.052 }).round();
        let title_size = (cap_size * 1.25).round();
        // En los diseños apilados el subtítulo va sobre la unión de pantalla y cámara.
        let cap_margin = match seam {
            Some(top) => h - top - (cap_size as u32) / 2 - 10,
            None => (h as f64 * if vertical { 0.2 } else { 0.07 }) as u32,
        };
        let title_margin = (h as f64 * if format == "vertical" { 0.1 } else { 0.06 }) as u32;
        let side = (w as f64 * 0.07) as u32;
        let style = |name: &str, size: f64, align: u32, margin: u32| {
            format!("Style: {name},Segoe UI,{size},&H00FFFFFF,&H00FFFFFF,&H00101010,&H96000000,-1,0,0,0,100,100,0,0,1,{},{},{align},{side},{side},{margin},1\n",
                (size / 14.0).max(3.0).round(), (size / 30.0).round())
        };
        let ass = format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
             [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
             {}{}\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{events}",
            style("Cap", cap_size, 2, cap_margin),
            style("Title", title_size, 8, title_margin),
        );
        let file = tmp("ass", ass.as_bytes())?;
        g.push(format!("[{v}]subtitles=filename={file}[v2]"));
        v = "v2".into();
    }
    g.push(format!("[{v}]format=yuv420p[vout]"));

    // Audio
    let mut labels = Vec::new();
    if let Some(i) = i_mic {
        g.push(format!("[{i}:a]aresample=48000,volume={mic_vol:.2}[am]"));
        labels.push("[am]");
    }
    if let Some(i) = i_sys {
        g.push(format!("[{i}:a]aresample=48000,volume={sys_vol:.2}[as]"));
        labels.push("[as]");
    }
    let has_audio = !labels.is_empty();
    if has_audio {
        let mix = if labels.len() == 2 { "amix=inputs=2:normalize=0:duration=longest," } else { "" };
        let cut = select.as_ref().map(|e| format!("aselect='{e}',asetpts=N/SR/TB,")).unwrap_or_default();
        let norm = if a.get("normalize_audio").and_then(Value::as_bool).unwrap_or(true) { "loudnorm=I=-16:TP=-1.5:LRA=11," } else { "" };
        g.push(format!("{}{mix}{cut}{norm}aresample=48000[aout]", labels.concat()));
    }

    let graph = tmp("graph.txt", g.join(";\n").as_bytes())?;
    args.extend(["-/filter_complex".into(), graph, "-map".into(), "[vout]".into()]);
    if has_audio {
        args.extend(["-map", "[aout]", "-c:a", "aac", "-b:a", "192k", "-ac", "2"].iter().map(|s| s.to_string()));
    }
    args.extend(
        ["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-r", "30", "-movflags", "+faststart", "-t"]
            .iter()
            .map(|s| s.to_string()),
    );
    args.push(format!("{total:.3}"));
    args.push(out.file_name().unwrap().to_string_lossy().into_owned());

    let progress = dir.join(&progress_file);
    drop(tmp);
    temp.push(progress.clone());
    let info = json!({
        "file": out.to_string_lossy(),
        "format": format, "layout": layout, "width": w, "height": h,
        "duration_seconds": (total * 10.0).round() / 10.0,
        "segments_used": kept.len(),
        "captions": n_caps,
    });
    Ok(Plan { args, dir, out, temp, progress, total, info })
}

fn run_plan(plan: &Plan) -> Result<Value, String> {
    let res = ff_output(&plan.args, Some(&plan.dir));
    for t in &plan.temp {
        let _ = std::fs::remove_file(t);
    }
    let out = res?;
    if !out.status.success() || !plan.out.is_file() {
        let _ = std::fs::remove_file(&plan.out);
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = log.lines().rev().take(6).collect();
        return Err(format!("FFmpeg no pudo exportar el video: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | ")));
    }
    let mut info = plan.info.clone();
    let size = std::fs::metadata(&plan.out).map(|m| m.len()).unwrap_or(0);
    info["size_mb"] = json!((size as f64 / 104_857.6).round() / 10.0);
    info["status"] = json!("done");
    Ok(info)
}

fn job_report(id: &str, job: &Job) -> Result<Value, String> {
    match job.state.lock().unwrap().clone() {
        JobState::Done(v) => Ok(v),
        JobState::Failed(e) => Err(e),
        JobState::Running => {
            let done = std::fs::read_to_string(&job.progress)
                .ok()
                .and_then(|s| s.lines().rev().find_map(|l| l.strip_prefix("out_time_us=")?.trim().parse::<f64>().ok()))
                .unwrap_or(0.0)
                / 1e6;
            Ok(json!({
                "status": "running",
                "job": id,
                "percent": ((done / job.total.max(0.1)) * 100.0).clamp(0.0, 99.0).round(),
                "next": "Llamá a render_status con este job en unos segundos para ver si terminó.",
            }))
        }
    }
}

fn tool_render(ctx: &Ctx, a: &Value) -> Result<Value, String> {
    let rec = find_rec(a, &ctx.default_dir)?;
    let plan = plan_render(&rec, a)?;
    let id = format!("{}-{}", rec.manifest.id, plan.out.file_stem().unwrap().to_string_lossy());
    let job = Arc::new(Job { state: Mutex::new(JobState::Running), progress: plan.progress.clone(), total: plan.total });
    ctx.jobs.lock().unwrap().insert(id.clone(), job.clone());
    let worker = job.clone();
    std::thread::spawn(move || {
        let state = match run_plan(&plan) {
            Ok(v) => JobState::Done(v),
            Err(e) => JobState::Failed(e),
        };
        *worker.state.lock().unwrap() = state;
    });
    let start = Instant::now();
    while start.elapsed() < RENDER_WAIT {
        if !matches!(*job.state.lock().unwrap(), JobState::Running) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    job_report(&id, &job)
}

fn tool_frames(ctx: &Ctx, a: &Value) -> Result<Vec<Value>, String> {
    let rec = find_rec(a, &ctx.default_dir)?;
    let times: Vec<f64> = a
        .get("times")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    if times.is_empty() {
        return Err("Pasá en times los segundos que querés ver (hasta 6).".into());
    }
    let which = str_arg(a, "track", "both");
    let screen = rec.track("screen").filter(|_| which != "camera");
    let camera = rec.track("camera").filter(|_| which != "screen");
    if screen.is_none() && camera.is_none() {
        return Err(if rec.track("screen").is_none() && rec.track("camera").is_none() {
            "Es una grabación de solo audio: no hay imágenes para ver. Usá get_recording para ver dónde se habla y render para armar el video.".into()
        } else {
            "La grabación no tiene esa pista de video.".into()
        });
    }
    let dur = rec.duration();
    let mut content = Vec::new();
    for t in times.into_iter().take(6) {
        let t = t.clamp(0.0, (dur - 0.2).max(0.0));
        let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
        for p in [&screen, &camera].into_iter().flatten() {
            args.extend(["-ss".into(), format!("{t:.3}"), "-i".into(), p.to_string_lossy().into_owned()]);
        }
        let graph = if screen.is_some() && camera.is_some() {
            "[0:v]scale=-2:400[a];[1:v]scale=-2:400[b];[a][b]hstack[o]"
        } else {
            "[0:v]scale=-2:480[o]"
        };
        args.extend(
            ["-filter_complex", graph, "-map", "[o]", "-frames:v", "1", "-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "6", "-"]
                .iter()
                .map(|s| s.to_string()),
        );
        let out = ff_output(&args, None)?;
        if out.stdout.is_empty() {
            content.push(json!({ "type": "text", "text": format!("No se pudo leer el cuadro del segundo {t:.1}.") }));
            continue;
        }
        let what = match (&screen, &camera) {
            (Some(_), Some(_)) => "pantalla a la izquierda, cámara a la derecha",
            (Some(_), None) => "pantalla",
            _ => "cámara",
        };
        content.push(json!({ "type": "text", "text": format!("Segundo {t:.1} ({what}):") }));
        content.push(json!({ "type": "image", "data": base64(&out.stdout), "mimeType": "image/jpeg" }));
    }
    Ok(content)
}

// ---------- Protocolo ----------

const INSTRUCTIONS: &str = "Dualrec records the screen, the webcam, the microphone and the system audio as separate, synced files. \
Use these tools to turn a recording into finished videos. Typical flow: list_recordings, then get_recording to see the tracks and \
where the person is speaking, optionally get_frames to look at what is on screen at given moments, then render. \
For a long horizontal video use format=horizontal with layout=pip (or side) and remove_silences=true. \
For reels, Shorts and TikToks use format=vertical with layout=stack (screen on top, camera below) or pip, pick 15 to 60 second \
segments, and use screen_zoom with screen_focus_x/y so that the relevant part of the screen is readable on a phone. \
Some recordings are audio only (audio_only=true, for example a voice note or a podcast): render turns them into a video with \
layout=audio, a moving gradient or a background_image the user gives you (a full file path), an animated waveform of the voice, \
a title that stays on screen and captions. Ask the user what the audio is about to write the title, and for the words or a \
transcript if they want captions. You cannot hear the audio: only add captions when the user gives you the words or a transcript, and ask which moments matter \
if speech timing and frames are not enough. Times are always seconds in the original recording. \
Exports are saved in the recording's 'exports' folder; tell the user the full path. Answer in the user's language.";

fn tools() -> Value {
    let folder = json!({ "type": "string", "description": "Folder with the recordings. Omit to use the default (Videos\\Dualrec)." });
    let id = json!({ "type": "string", "description": "Recording id from list_recordings, or \"latest\" (default) for the most recent one." });
    let seg_items = json!({ "type": "object", "properties": { "start": { "type": "number" }, "end": { "type": "number" } }, "required": ["start", "end"] });
    json!([
        {
            "name": "list_recordings",
            "description": "List the Dualrec recordings on this computer, newest first, with duration, tracks and videos already exported.",
            "inputSchema": { "type": "object", "properties": { "folder": folder } },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_recording",
            "description": "Details of one recording: tracks, duration, the time ranges where the person is speaking (detected from the microphone) and existing exports. Use it to decide what to keep or cut.",
            "inputSchema": { "type": "object", "properties": {
                "id": id, "folder": folder,
                "silence_db": { "type": "number", "description": "Level below which audio counts as silence, in dB. Default -35. Use -45 for quiet voices, -28 for noisy rooms." }
            } },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_frames",
            "description": "Look at the recording: returns still images of the screen and the camera at the given seconds (up to 6 per call). Use it to find what is being shown and where to zoom.",
            "inputSchema": { "type": "object", "properties": {
                "id": id, "folder": folder,
                "times": { "type": "array", "items": { "type": "number" }, "description": "Seconds in the recording." },
                "track": { "type": "string", "enum": ["both", "screen", "camera"] }
            }, "required": ["times"] },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "render",
            "description": "Export a finished MP4 from a recording by combining screen, camera and audio. Makes long horizontal videos and vertical reels, and turns audio-only recordings into videos with a waveform. Saved to the recording's exports folder. If it takes long it returns a job to check with render_status.",
            "inputSchema": { "type": "object", "properties": {
                "id": id, "folder": folder,
                "format": { "type": "string", "enum": ["horizontal", "vertical", "square"], "description": "horizontal 1920x1080 (YouTube), vertical 1080x1920 (Reels, Shorts, TikTok), square 1080x1080." },
                "layout": { "type": "string", "enum": ["pip", "stack", "side", "screen", "camera", "audio"], "description": "pip: screen with the camera in a bubble. stack: screen on top, camera below (best for vertical). side: screen left, camera right. screen / camera: only that source. audio: background and animated waveform of the voice (always used for audio-only recordings). Default: pip for horizontal, stack otherwise." },
                "background_image": { "type": "string", "description": "layout audio: full path of an image (JPG/PNG) to use as background, cropped to fill. Omit for a moving gradient." },
                "background_color": { "type": "string", "description": "layout audio without image: main color of the gradient, hex like #1b1638." },
                "waveform": { "type": "boolean", "description": "layout audio: show the animated waveform. Default true." },
                "wave_color": { "type": "string", "description": "layout audio: waveform color, hex. Default #8b7cff." },
                "segments": { "type": "array", "items": seg_items, "description": "Parts of the recording to keep, in seconds. They are joined in order. Omit to use the whole recording." },
                "remove_silences": { "type": "boolean", "description": "Cut the pauses where nobody speaks (jump cuts). Default false." },
                "silence_db": { "type": "number", "description": "Silence threshold in dB for remove_silences. Default -35." },
                "screen_zoom": { "type": "number", "description": "1 shows the whole screen; 2 zooms in to half of it. Useful in vertical so text is readable. Max 4." },
                "screen_focus_x": { "type": "number", "description": "Where to zoom horizontally, 0 (left) to 1 (right). Default 0.5." },
                "screen_focus_y": { "type": "number", "description": "Where to zoom vertically, 0 (top) to 1 (bottom). Default 0.5." },
                "camera_position": { "type": "string", "enum": ["bottom-right", "bottom-left", "bottom-center", "top-right", "top-left", "top-center"], "description": "Bubble position for layout pip." },
                "camera_size": { "type": "number", "description": "Bubble size as a percentage of the short side of the video. Default 28." },
                "camera_shape": { "type": "string", "enum": ["circle", "rounded", "rect"] },
                "title": { "type": "string", "description": "Hook text shown at the top during the first seconds." },
                "title_seconds": { "type": "number", "description": "How long the title stays. Default 3.5, or the whole video for layout audio." },
                "captions": { "type": "array", "description": "Subtitles burned into the video. Times are seconds in the ORIGINAL recording; they are shifted automatically when parts are cut.", "items": { "type": "object", "properties": { "start": { "type": "number" }, "end": { "type": "number" }, "text": { "type": "string" } }, "required": ["start", "end", "text"] } },
                "mic_volume": { "type": "number", "description": "Voice volume, 1 = as recorded. 0 mutes it." },
                "system_volume": { "type": "number", "description": "Computer audio volume. Default 0.6. 0 mutes it." },
                "normalize_audio": { "type": "boolean", "description": "Level the loudness for social networks. Default true." },
                "output_name": { "type": "string", "description": "File name without extension." }
            } }
        },
        {
            "name": "render_status",
            "description": "Check an export started by render that was still running.",
            "inputSchema": { "type": "object", "properties": { "job": { "type": "string" } }, "required": ["job"] },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

fn text(v: &Value) -> Vec<Value> {
    vec![json!({ "type": "text", "text": serde_json::to_string_pretty(v).unwrap_or_default() })]
}

fn call_tool(ctx: &Ctx, name: &str, a: &Value) -> Result<Vec<Value>, String> {
    match name {
        "list_recordings" => {
            let folder = folder_arg(a, &ctx.default_dir);
            let list: Vec<Value> = recorder::list_recordings(&folder).iter().map(summary).collect();
            Ok(text(&json!({ "folder": folder.to_string_lossy(), "count": list.len(), "recordings": list })))
        }
        "get_recording" => {
            let rec = find_rec(a, &ctx.default_dir)?;
            let mut v = summary(&rec.manifest);
            match detect_speech(&rec, num_arg(a, "silence_db", -35.0, -70.0, -10.0)) {
                Ok(speech) => {
                    let total: f64 = speech.iter().map(|s| s.1 - s.0).sum();
                    v["speech_seconds"] = json!((total * 10.0).round() / 10.0);
                    v["speech_segments_truncated"] = json!(speech.len() > 400);
                    v["speech_segments"] = speech
                        .iter()
                        .take(400)
                        .map(|s| json!([(s.0 * 100.0).round() / 100.0, (s.1 * 100.0).round() / 100.0]))
                        .collect();
                }
                Err(e) => v["speech_error"] = json!(e),
            }
            Ok(text(&v))
        }
        "get_frames" => tool_frames(ctx, a),
        "render" => tool_render(ctx, a).map(|v| text(&v)),
        "render_status" => {
            let id = a.get("job").and_then(Value::as_str).unwrap_or_default();
            let job = ctx.jobs.lock().unwrap().get(id).cloned().ok_or("No hay ninguna exportación con ese job en esta sesión.")?;
            job_report(id, &job).map(|v| text(&v))
        }
        _ => Err(format!("Herramienta desconocida: {name}")),
    }
}

fn handle(ctx: &Ctx, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "dualrec", "title": "Dualrec", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(ctx, name, &args) {
                Ok(content) => json!({ "content": content, "isError": false }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            }
        }
        _ => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Method not found: {method}") } }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Atiende el protocolo hasta que el cliente cierra la entrada.
pub fn run(default_dir: PathBuf) -> i32 {
    let ctx = Ctx { default_dir, jobs: Mutex::default() };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => handle(&ctx, &msg),
            Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Parse error" } })),
        };
        if let Some(r) = reply {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    0
}

// ---------- Conexión con Claude ----------

/// Carpetas de configuración de Claude Desktop que existen en esta máquina
/// (instalador clásico y versión de la Tienda de Windows).
fn claude_desktop_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = dirs::config_dir().map(|d| d.join("Claude")) {
        out.push(d);
    }
    if let Some(pk) = dirs::data_local_dir().map(|d| d.join("Packages")) {
        for e in std::fs::read_dir(pk).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("Claude_") {
                out.push(e.path().join("LocalCache").join("Roaming").join("Claude"));
            }
        }
    }
    out.retain(|d| d.is_dir());
    out
}

fn exe_path() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "dualrec.exe".into())
}

fn desktop_entry(dir: &str) -> Value {
    json!({ "command": exe_path(), "args": ["--mcp", dir] })
}

/// Estado de la conexión: si Claude Desktop está instalado, si ya tiene a
/// Dualrec configurado y el comando para sumarlo en Claude Code.
pub fn claude_status(dir: &str) -> Value {
    let dirs = claude_desktop_dirs();
    let wanted = desktop_entry(dir);
    let connected = !dirs.is_empty()
        && dirs.iter().all(|d| {
            std::fs::read(d.join("claude_desktop_config.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|v| v["mcpServers"]["dualrec"] == wanted)
        });
    json!({
        "desktop_found": !dirs.is_empty(),
        "connected": connected,
        "code_command": format!("claude mcp add dualrec -- \"{}\" --mcp \"{dir}\"", exe_path()),
    })
}

/// Escribe la entrada `dualrec` en el `claude_desktop_config.json` de `d`, conservando lo demás.
fn write_entry(d: &Path, entry: &Value) -> Result<(), String> {
    let file = d.join("claude_desktop_config.json");
    let mut cfg: Value = match std::fs::read(&file) {
        Ok(b) if !b.iter().all(u8::is_ascii_whitespace) => serde_json::from_slice(&b)
            .map_err(|_| format!("El archivo {} tiene un formato que no pude leer; no lo modifiqué.", file.display()))?,
        _ => json!({}),
    };
    if !cfg.is_object() {
        return Err(format!("El archivo {} tiene un formato inesperado; no lo modifiqué.", file.display()));
    }
    if !cfg["mcpServers"].is_object() {
        cfg["mcpServers"] = json!({});
    }
    cfg["mcpServers"]["dualrec"] = entry.clone();
    let tmp = d.join("claude_desktop_config.json.dualrec-tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?)
        .and_then(|_| std::fs::rename(&tmp, &file))
        .map_err(|e| format!("No se pudo guardar {}: {e}", file.display()))?;
    Ok(())
}

/// Agrega Dualrec a la configuración de Claude Desktop sin tocar el resto.
pub fn claude_connect(dir: &str) -> Result<(), String> {
    let dirs = claude_desktop_dirs();
    if dirs.is_empty() {
        return Err("No encontré Claude Desktop en esta computadora. Instalalo desde claude.ai/download, abrilo una vez y volvé a intentar.".into());
    }
    let entry = desktop_entry(dir);
    for d in dirs {
        write_entry(&d, &entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn colors_accept_hex_with_or_without_hash() {
        assert_eq!(color_arg(&json!({ "c": "#8B7CFF" }), "c", "000000").unwrap(), "8b7cff");
        assert_eq!(color_arg(&json!({ "c": "1b1638" }), "c", "000000").unwrap(), "1b1638");
        assert_eq!(color_arg(&json!({}), "c", "0b0d12").unwrap(), "0b0d12");
        assert!(color_arg(&json!({ "c": "red" }), "c", "000000").is_err());
        assert!(color_arg(&json!({ "c": "#12345;" }), "c", "000000").is_err());
    }

    #[test]
    fn normalize_sorts_clamps_and_merges() {
        let s = normalize(vec![(5.0, 8.0), (-1.0, 2.0), (1.5, 3.0), (20.0, 30.0)], 10.0);
        assert_eq!(s, vec![(0.0, 3.0), (5.0, 8.0)]);
    }

    #[test]
    fn speech_is_the_complement_of_silence() {
        let s = speech_from_silences(&[(0.0, 1.0), (4.0, 6.0)], 10.0);
        assert_eq!(s.len(), 2);
        assert!((s[0].0 - 0.88).abs() < 1e-6 && (s[0].1 - 4.15).abs() < 1e-6);
        assert!((s[1].0 - 5.88).abs() < 1e-6 && (s[1].1 - 10.0).abs() < 1e-6);
    }

    #[test]
    fn captions_follow_the_cuts() {
        let kept = [(10.0, 20.0), (30.0, 40.0)];
        assert_eq!(map_interval(&kept, (12.0, 14.0)), vec![(2.0, 4.0)]);
        assert_eq!(map_interval(&kept, (31.0, 33.0)), vec![(11.0, 13.0)]);
        assert_eq!(map_interval(&kept, (18.0, 32.0)), vec![(8.0, 10.0), (10.0, 12.0)]);
        assert!(map_interval(&kept, (22.0, 28.0)).is_empty());
    }

    #[test]
    fn mask_has_header_and_round_shape() {
        let m = mask_pgm(40, "circle");
        let header = b"P5\n40 40\n255\n";
        assert_eq!(&m[..header.len()], header);
        let px = &m[header.len()..];
        assert_eq!(px.len(), 1600);
        assert_eq!(px[0], 0); // esquina afuera
        assert_eq!(px[20 * 40 + 20], 255); // centro adentro
        assert!(mask_pgm(40, "rect")[header.len()..].iter().all(|&p| p == 255));
    }

    #[test]
    fn connect_keeps_other_settings_and_refuses_broken_files() {
        let d = std::env::temp_dir().join(format!("dualrec-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("claude_desktop_config.json");
        let entry = json!({ "command": "x.exe", "args": ["--mcp", "C:/v"] });
        write_entry(&d, &entry).unwrap(); // sin archivo previo
        std::fs::write(&file, r#"{"theme":"dark","mcpServers":{"otro":{"command":"a"}}}"#).unwrap();
        write_entry(&d, &entry).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["otro"]["command"], "a");
        assert_eq!(v["mcpServers"]["dualrec"], entry);
        std::fs::write(&file, "{ roto").unwrap();
        assert!(write_entry(&d, &entry).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ roto");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn protocol_lists_tools_and_ignores_notifications() {
        let ctx = Ctx::default();
        let init = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&ctx, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 5);
        let bad = handle(&ctx, &json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }
}
