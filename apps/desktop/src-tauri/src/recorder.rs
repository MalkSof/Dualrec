//! Orquestador de una grabación: una carpeta por sesión con una pista por
//! fuente (`screen.mp4`, `camera.mp4`, `mic.m4a`, `system.m4a`).
//!
//! Sincronía: todas las pistas se rigen por el mismo `RecClock`. El video se
//! escribe a ritmo constante (el cuadro `k` es el instante `k / fps`, repitiendo
//! el último cuadro si la fuente no entregó uno nuevo) y el audio se mide en
//! muestras contra el mismo reloj, rellenando con silencio los huecos. Así
//! todas las pistas arrancan en `t0` (offset 0) y duran lo mismo.

use crate::audio::{AudioCapture, AudioSource};
use crate::camera::CamShared;
use crate::clock::{frames_due, samples_due, RecClock};
use crate::ffmpeg::{self, Encoder, VideoSpec};
use crate::screen::{self, ScreenCapture, ScreenTarget};
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
pub struct RecOptions {
    pub out_dir: String,
    pub fps: u32,
    /// "alta" | "media" | "baja"
    pub quality: String,
    /// Alto máximo de la pista de pantalla (p. ej. 1080); `None` = nativo.
    pub max_height: Option<u32>,
    pub cursor: bool,
    /// Conserva los MP4 fragmentados al cortar: los necesita la subida a la nube.
    #[serde(default)]
    pub keep_parts: bool,
}

/// Nombre del archivo fragmentado de una pista: `screen.mp4` -> `screen.part.mp4`.
pub fn part_name(file: &str) -> String {
    match file.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.part.{ext}"),
        None => format!("{file}.part"),
    }
}

pub struct CameraSource {
    pub shared: Arc<CamShared>,
    pub name: String,
    pub fps: u32,
}

pub struct Sources {
    pub screen: Option<ScreenTarget>,
    pub camera: Option<CameraSource>,
    pub mic: Option<Arc<AudioCapture>>,
    pub system_audio: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    /// screen | camera | mic | system
    pub kind: String,
    pub file: String,
    pub codec: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    /// Desfase respecto de `t0` del reloj común.
    pub offset_ms: i64,
    pub duration_ms: u64,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub app_version: String,
    /// recording | ready | failed
    pub status: String,
    pub recorded_at: String,
    pub duration_ms: u64,
    pub encoder: String,
    pub clock: String,
    pub dir: String,
    pub tracks: Vec<Track>,
}

impl Manifest {
    fn write(&self, dir: &Path) -> Result<()> {
        let tmp = dir.join("session.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join("session.json"))?;
        Ok(())
    }
}

struct Outcome {
    /// Cuadros o muestras escritos.
    units: u64,
    error: Option<String>,
}

struct Worker {
    track: Track,
    part: PathBuf,
    log: PathBuf,
    handle: JoinHandle<Outcome>,
}

pub struct Session {
    pub manifest: Manifest,
    pub dir: PathBuf,
    clock: Arc<RecClock>,
    stop: Arc<AtomicBool>,
    screen: Option<ScreenCapture>,
    mic: Option<Arc<AudioCapture>>,
    system: Option<AudioCapture>,
    workers: Vec<Worker>,
    pub screen_frames: Arc<AtomicU64>,
    keep_parts: bool,
}

fn read_log(log: &Path) -> String {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn finish_child(mut child: Child, units: u64, write_error: Option<String>, log: &Path) -> Outcome {
    drop(child.stdin.take());
    let status = child.wait();
    let error = match (write_error, status) {
        (Some(e), _) => Some(format!("{e}: {}", read_log(log))),
        (None, Ok(s)) if !s.success() => Some(format!("ffmpeg terminó con error: {}", read_log(log))),
        (None, Err(e)) => Some(e.to_string()),
        _ => None,
    };
    Outcome { units, error }
}

/// Escribe video a ritmo constante contra el reloj común. `pull` copia el
/// último cuadro disponible en el buffer (o lo deja como está si no hay uno
/// nuevo, con lo que se repite el anterior).
fn video_worker(
    mut child: Child,
    mut pull: impl FnMut(&mut Vec<u8>),
    frame_len: usize,
    fps: u32,
    clock: Arc<RecClock>,
    stop: Arc<AtomicBool>,
    counter: Option<Arc<AtomicU64>>,
    log: PathBuf,
) -> Outcome {
    let mut stdin = child.stdin.take().expect("stdin de ffmpeg");
    let mut buf = vec![0u8; frame_len];
    let mut written: u64 = 0;
    let mut write_error = None;
    loop {
        // Se lee `stop` antes que el reloj: al cortar, el reloj ya está
        // congelado, así que todas las pistas terminan en el mismo instante.
        let stopping = stop.load(Ordering::SeqCst);
        let elapsed = clock.elapsed();
        if written < frames_due(elapsed, fps) {
            pull(&mut buf);
            if let Err(e) = stdin.write_all(&buf) {
                write_error = Some(format!("se cortó la escritura de video ({e})"));
                break;
            }
            written += 1;
            if let Some(c) = &counter {
                c.store(written, Ordering::Relaxed);
            }
            continue;
        }
        if stopping {
            break;
        }
        let next = Duration::from_nanos(written * 1_000_000_000 / fps as u64);
        let wait = next
            .saturating_sub(elapsed)
            .clamp(Duration::from_millis(1), Duration::from_millis(8));
        std::thread::sleep(wait);
    }
    drop(stdin);
    finish_child(child, written, write_error, &log)
}

fn f32_bytes(samples: &[f32]) -> &[u8] {
    // f32 little-endian en todas las plataformas soportadas (x86_64 / aarch64).
    unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 4) }
}

/// Cuántas muestras de un bloque entrante se aceptan sin adelantarse al reloj.
pub fn frames_to_accept(written: u64, due: u64, incoming: u64, rate: u32, finishing: bool) -> u64 {
    // Hasta 20 ms de adelanto; al cortar, ni una muestra más allá del reloj.
    let ceiling = if finishing { due } else { due + rate as u64 / 50 };
    incoming.min(ceiling.saturating_sub(written))
}

/// Cuánto silencio hay que insertar para no quedar atrás del reloj.
pub fn silence_needed(written: u64, due: u64, rate: u32, finishing: bool) -> u64 {
    if finishing {
        return due.saturating_sub(written);
    }
    let tolerance = rate as u64 * 150 / 1000;
    if written + tolerance < due {
        due - written - rate as u64 / 50
    } else {
        0
    }
}

/// Escribe audio contando muestras contra el reloj común: descarta lo que
/// llega en pausa y rellena con silencio los huecos (por ejemplo, el loopback
/// de WASAPI no entrega nada mientras no suena ningún sonido).
fn audio_worker(
    mut child: Child,
    rx: Receiver<Vec<f32>>,
    rate: u32,
    channels: u16,
    clock: Arc<RecClock>,
    stop: Arc<AtomicBool>,
    log: PathBuf,
) -> Outcome {
    let mut stdin = child.stdin.take().expect("stdin de ffmpeg");
    let ch = channels.max(1) as u64;
    let zeros = vec![0f32; (rate as usize / 10) * ch as usize];
    let mut written: u64 = 0;
    let mut write_error: Option<String> = None;

    let mut put = |samples: &[f32], written: &mut u64| -> bool {
        match stdin.write_all(f32_bytes(samples)) {
            Ok(()) => {
                *written += samples.len() as u64 / ch;
                true
            }
            Err(e) => {
                write_error = Some(format!("se cortó la escritura de audio ({e})"));
                false
            }
        }
    };

    'outer: loop {
        let stopping = stop.load(Ordering::SeqCst);
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => {
                if !clock.is_paused() || stopping {
                    let due = samples_due(clock.elapsed(), rate);
                    let incoming = chunk.len() as u64 / ch;
                    let take = frames_to_accept(written, due, incoming, rate, stopping);
                    if take > 0 && !put(&chunk[..(take * ch) as usize], &mut written) {
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if !stopping {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        let due = samples_due(clock.elapsed(), rate);
        let mut missing = silence_needed(written, due, rate, stopping);
        while missing > 0 {
            let n = missing.min(zeros.len() as u64 / ch);
            if !put(&zeros[..(n * ch) as usize], &mut written) {
                break 'outer;
            }
            missing -= n;
        }
        if stopping {
            break;
        }
    }
    drop(put);
    drop(stdin);
    finish_child(child, written, write_error, &log)
}

fn audio_track(kind: &str, cap: &AudioCapture) -> Track {
    Track {
        kind: kind.into(),
        file: format!("{kind}.m4a"),
        codec: "aac".into(),
        source: cap.name.clone(),
        width: None,
        height: None,
        fps: None,
        sample_rate: Some(cap.rate),
        channels: Some(cap.channels),
        offset_ms: 0,
        duration_ms: 0,
        size_bytes: 0,
        error: None,
    }
}

impl Session {
    pub fn start(sources: Sources, opts: &RecOptions) -> Result<Session> {
        if sources.screen.is_none()
            && sources.camera.is_none()
            && sources.mic.is_none()
            && !sources.system_audio
        {
            return Err(anyhow!("Elegí al menos una fuente para grabar."));
        }
        if !ffmpeg::available() {
            return Err(anyhow!(
                "No se encontró FFmpeg. Reinstalá Dualrec o instalá ffmpeg y agregalo al PATH."
            ));
        }
        let fps = opts.fps.clamp(5, 60);
        let encoder: Encoder = ffmpeg::detect_encoder();
        let quality = ffmpeg::quality_factor(&opts.quality);

        let now = chrono::Local::now();
        let id = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let dir = PathBuf::from(&opts.out_dir).join(&id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("No se pudo crear la carpeta {}", dir.display()))?;

        // 1) Se abren las fuentes y se espera a que estén entregando datos,
        //    para que `t0` no incluya tiempos de arranque de dispositivos.
        let screen = match &sources.screen {
            Some(target) => {
                let cap = ScreenCapture::start(target, fps, opts.cursor)?;
                let size = cap.wait_first_frame(Duration::from_secs(4))?;
                Some((cap, size, screen::target_label(target)))
            }
            None => None,
        };
        let camera_size = match &sources.camera {
            Some(cam) => {
                let f = cam.shared.frame.lock();
                if f.seq == 0 || f.width == 0 {
                    return Err(anyhow!("La cámara todavía no entregó imagen."));
                }
                Some((f.width, f.height))
            }
            None => None,
        };
        let system = if sources.system_audio {
            Some(AudioCapture::open(AudioSource::SystemLoopback)?)
        } else {
            None
        };

        // 2) Se lanzan los encoders (uno por pista).
        struct Pending {
            track: Track,
            part: PathBuf,
            log: PathBuf,
            child: Child,
        }
        let spawn_track = |track: Track, args_for: &dyn Fn(&Path) -> Vec<String>| -> Result<Pending> {
            let stem = track.file.rsplit_once('.').map(|x| x.0).unwrap_or(&track.file);
            let part = dir.join(part_name(&track.file));
            let log = dir.join(format!("{stem}.log"));
            let child = ffmpeg::spawn(&args_for(&part), &log)?;
            Ok(Pending {
                track,
                part,
                log,
                child,
            })
        };

        let mut video_pending: Vec<(Pending, &'static str)> = Vec::new();
        if let Some((_, (w, h), label)) = &screen {
            let spec = VideoSpec {
                pix_fmt: "bgra",
                width: *w,
                height: *h,
                fps,
                encoder,
                quality,
                max_height: opts.max_height,
            };
            let track = Track {
                kind: "screen".into(),
                file: "screen.mp4".into(),
                codec: "h264".into(),
                source: label.clone(),
                width: Some(*w),
                height: Some(*h),
                fps: Some(fps),
                sample_rate: None,
                channels: None,
                offset_ms: 0,
                duration_ms: 0,
                size_bytes: 0,
                error: None,
            };
            video_pending.push((spawn_track(track, &|p| ffmpeg::video_args(&spec, p))?, "screen"));
        }
        // La pista de cámara va a 30 fps fijos: si la cámara entrega menos se
        // repiten cuadros y si entrega más se saltean, siempre contra el reloj común.
        let cam_fps = sources.camera.as_ref().map(|_| 30u32);
        if let (Some(cam), Some((w, h))) = (&sources.camera, camera_size) {
            let spec = VideoSpec {
                pix_fmt: "rgb24",
                width: w,
                height: h,
                fps: cam_fps.unwrap(),
                encoder,
                quality,
                max_height: None,
            };
            let track = Track {
                kind: "camera".into(),
                file: "camera.mp4".into(),
                codec: "h264".into(),
                source: cam.name.clone(),
                width: Some(w),
                height: Some(h),
                fps: cam_fps,
                sample_rate: None,
                channels: None,
                offset_ms: 0,
                duration_ms: 0,
                size_bytes: 0,
                error: None,
            };
            video_pending.push((spawn_track(track, &|p| ffmpeg::video_args(&spec, p))?, "camera"));
        }
        let mut audio_pending: Vec<(Pending, Receiver<Vec<f32>>, u32, u16)> = Vec::new();
        let mut audio_senders = Vec::new();
        for (kind, cap) in [("mic", sources.mic.as_deref()), ("system", system.as_ref())] {
            if let Some(cap) = cap {
                let (rate, channels) = (cap.rate, cap.channels);
                let pending =
                    spawn_track(audio_track(kind, cap), &|p| ffmpeg::audio_args(rate, channels, p))?;
                let (tx, rx) = crossbeam_channel::unbounded();
                audio_senders.push((cap, tx));
                audio_pending.push((pending, rx, rate, channels));
            }
        }

        // 3) `t0`: arranca el reloj común y recién ahí se conectan las fuentes.
        let clock = Arc::new(RecClock::new());
        let stop = Arc::new(AtomicBool::new(false));
        let screen_frames = Arc::new(AtomicU64::new(0));
        for (cap, tx) in audio_senders {
            cap.attach(tx);
        }

        let mut workers = Vec::new();
        for (p, kind) in video_pending {
            let (w, h, track_fps) = (p.track.width.unwrap(), p.track.height.unwrap(), p.track.fps.unwrap());
            let (clock_t, stop_t, log_t) = (clock.clone(), stop.clone(), p.log.clone());
            let child = p.child;
            let handle = if kind == "screen" {
                let shared = screen.as_ref().unwrap().0.shared.clone();
                let counter = Some(screen_frames.clone());
                let mut last = 0u64;
                std::thread::Builder::new().name("dualrec-screen".into()).spawn(move || {
                    let pull = move |buf: &mut Vec<u8>| {
                        let slot = shared.slot.lock();
                        if slot.seq != last && slot.data.len() == buf.len() {
                            buf.copy_from_slice(&slot.data);
                            last = slot.seq;
                        }
                    };
                    video_worker(child, pull, (w * h * 4) as usize, track_fps, clock_t, stop_t, counter, log_t)
                })?
            } else {
                let shared = sources.camera.as_ref().unwrap().shared.clone();
                let mut last = 0u64;
                std::thread::Builder::new().name("dualrec-camera-rec".into()).spawn(move || {
                    let pull = move |buf: &mut Vec<u8>| {
                        let f = shared.frame.lock();
                        if f.seq != last && f.data.len() == buf.len() {
                            buf.copy_from_slice(&f.data);
                            last = f.seq;
                        }
                    };
                    video_worker(child, pull, (w * h * 3) as usize, track_fps, clock_t, stop_t, None, log_t)
                })?
            };
            workers.push(Worker {
                track: p.track,
                part: p.part,
                log: p.log,
                handle,
            });
        }
        for (p, rx, rate, channels) in audio_pending {
            let (clock_t, stop_t, log_t) = (clock.clone(), stop.clone(), p.log.clone());
            let child = p.child;
            let handle = std::thread::Builder::new()
                .name("dualrec-audio-rec".into())
                .spawn(move || audio_worker(child, rx, rate, channels, clock_t, stop_t, log_t))?;
            workers.push(Worker {
                track: p.track,
                part: p.part,
                log: p.log,
                handle,
            });
        }

        let manifest = Manifest {
            id,
            app_version: env!("CARGO_PKG_VERSION").into(),
            status: "recording".into(),
            recorded_at: now.to_rfc3339(),
            duration_ms: 0,
            encoder: encoder.codec().into(),
            clock: "monotonic; todas las pistas comparten t0 (offset_ms relativo a t0)".into(),
            dir: dir.to_string_lossy().into_owned(),
            tracks: workers.iter().map(|w| w.track.clone()).collect(),
        };
        manifest.write(&dir)?;

        Ok(Session {
            manifest,
            dir,
            clock,
            stop,
            screen: screen.map(|s| s.0),
            mic: sources.mic,
            system,
            workers,
            screen_frames,
            keep_parts: opts.keep_parts,
        })
    }

    pub fn pause(&self) {
        self.clock.pause();
    }

    pub fn resume(&self) {
        self.clock.resume();
    }

    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Corta la grabación, finaliza los archivos y devuelve el manifiesto.
    pub fn stop(mut self) -> Manifest {
        // Primero se congela el reloj: todas las pistas cierran en el mismo instante.
        self.clock.pause();
        let total = self.clock.elapsed();
        self.stop.store(true, Ordering::SeqCst);

        let mut tracks = Vec::new();
        for w in self.workers.drain(..) {
            let outcome = w.handle.join().unwrap_or(Outcome {
                units: 0,
                error: Some("el hilo de grabación terminó de forma inesperada".into()),
            });
            let mut track = w.track;
            track.duration_ms = match (track.fps, track.sample_rate) {
                (Some(fps), _) => outcome.units * 1000 / fps as u64,
                (_, Some(rate)) => outcome.units * 1000 / rate as u64,
                _ => 0,
            };
            let out = self.dir.join(&track.file);
            let finalized = if w.part.exists() {
                ffmpeg::finalize(&w.part, &out)
            } else {
                Err(anyhow!("no se generó el archivo"))
            };
            match finalized {
                Ok(()) => {
                    if !self.keep_parts {
                        let _ = std::fs::remove_file(&w.part);
                    }
                }
                Err(e) => {
                    // El fragmentado igual se puede reproducir: se conserva.
                    if self.keep_parts {
                        let _ = std::fs::copy(&w.part, &out);
                    } else if w.part.exists() && std::fs::rename(&w.part, &out).is_err() {
                        track.file = w.part.file_name().unwrap().to_string_lossy().into_owned();
                    }
                    track.error.get_or_insert(e.to_string());
                }
            }
            if let Some(e) = outcome.error {
                track.error = Some(e);
            }
            track.size_bytes = std::fs::metadata(self.dir.join(&track.file))
                .map(|m| m.len())
                .unwrap_or(0);
            if track.error.is_none() {
                let _ = std::fs::remove_file(&w.log);
            }
            tracks.push(track);
        }

        if let Some(mic) = &self.mic {
            mic.detach();
        }
        self.system = None;
        if let Some(mut s) = self.screen.take() {
            s.stop();
        }

        let mut manifest = self.manifest.clone();
        manifest.duration_ms = total.as_millis() as u64;
        manifest.status = if tracks.iter().all(|t| t.error.is_none()) {
            "ready".into()
        } else if tracks.iter().any(|t| t.size_bytes > 0) {
            "ready_with_errors".into()
        } else {
            "failed".into()
        };
        manifest.tracks = tracks;
        let _ = manifest.write(&self.dir);
        manifest
    }
}

/// Lee los manifiestos de las grabaciones guardadas en `dir`, más nuevas primero.
pub fn list_recordings(dir: &Path) -> Vec<Manifest> {
    let mut out: Vec<Manifest> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let bytes = std::fs::read(e.path().join("session.json")).ok()?;
            let mut m: Manifest = serde_json::from_slice(&bytes).ok()?;
            m.dir = e.path().to_string_lossy().into_owned();
            Some(m)
        })
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_never_runs_ahead_of_the_clock() {
        // Al día: se acepta el bloque entero.
        assert_eq!(frames_to_accept(48_000, 48_480, 480, 48_000, false), 480);
        // 20 ms de adelanto como máximo (960 muestras a 48 kHz).
        assert_eq!(frames_to_accept(48_900, 48_000, 480, 48_000, false), 60);
        assert_eq!(frames_to_accept(50_000, 48_000, 480, 48_000, false), 0);
        // Al cortar no se pasa del reloj: todas las pistas duran lo mismo.
        assert_eq!(frames_to_accept(47_900, 48_000, 480, 48_000, true), 100);
    }

    #[test]
    fn gaps_are_filled_with_silence() {
        // Un retraso chico es latencia normal del dispositivo: no se rellena.
        assert_eq!(silence_needed(48_000, 50_000, 48_000, false), 0);
        // Hueco de 1 s (p. ej. loopback en silencio): se rellena hasta 20 ms antes del reloj.
        assert_eq!(silence_needed(0, 48_000, 48_000, false), 48_000 - 960);
        // Al cortar se completa exacto para que todas las pistas duren lo mismo.
        assert_eq!(silence_needed(47_000, 48_000, 48_000, true), 1_000);
        assert_eq!(silence_needed(48_500, 48_000, 48_000, true), 0);
    }

    #[test]
    fn manifest_roundtrips_and_lists_newest_first() {
        let base = std::env::temp_dir().join(format!("dualrec-test-{}", std::process::id()));
        for id in ["2026-01-01_10-00-00", "2026-01-02_10-00-00"] {
            let dir = base.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            Manifest {
                id: id.into(),
                app_version: "0".into(),
                status: "ready".into(),
                recorded_at: String::new(),
                duration_ms: 1000,
                encoder: "libx264".into(),
                clock: String::new(),
                dir: String::new(),
                tracks: vec![],
            }
            .write(&dir)
            .unwrap();
        }
        let list = list_recordings(&base);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "2026-01-02_10-00-00");
        let _ = std::fs::remove_dir_all(&base);
    }
}
