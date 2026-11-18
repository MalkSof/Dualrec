//! Cámara con nokhwa. Un hilo mantiene abierta la cámara elegida y publica el
//! último cuadro en RGB (para grabar) y un JPEG chico (para la burbuja).

use anyhow::{anyhow, Result};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    Resolution,
};
use nokhwa::Camera;
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
pub struct CameraInfo {
    pub index: u32,
    pub name: String,
}

pub fn list_cameras() -> Vec<CameraInfo> {
    nokhwa::query(ApiBackend::Auto)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| match c.index() {
            CameraIndex::Index(i) => Some(CameraInfo {
                index: *i,
                name: c.human_name(),
            }),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
pub struct CamFrame {
    /// RGB24, de arriba hacia abajo.
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub seq: u64,
}

#[derive(Default)]
pub struct Preview {
    pub jpeg: Arc<Vec<u8>>,
    pub seq: u64,
}

/// Estado compartido de la cámara; vive durante toda la app para que el
/// servidor de vista previa pueda seguir conectado al cambiar de cámara.
#[derive(Default)]
pub struct CamShared {
    pub frame: Mutex<CamFrame>,
    pub preview: Mutex<Preview>,
    pub preview_cv: Condvar,
    pub active: AtomicBool,
}

pub struct CameraHandle {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn make_preview(rgb: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    const MAX_W: u32 = 480;
    let step = w.div_ceil(MAX_W).max(1);
    let (pw, ph) = (w / step, h / step);
    if pw == 0 || ph == 0 {
        return None;
    }
    let mut small = Vec::with_capacity((pw * ph * 3) as usize);
    for y in 0..ph {
        let row = (y * step * w * 3) as usize;
        for x in 0..pw {
            let i = row + (x * step * 3) as usize;
            small.extend_from_slice(&rgb[i..i + 3]);
        }
    }
    let mut out = Vec::with_capacity(32 * 1024);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 72)
        .encode(&small, pw, ph, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(out)
}

fn friendly_error(e: impl std::fmt::Display) -> anyhow::Error {
    let text = e.to_string();
    if text.contains("0xC00D3704") || text.contains("hardware resources") {
        return anyhow!(
            "La cámara no está entregando imagen. Suele pasar cuando otra app la está usando              (OBS, Teams, el navegador), cuando la tapa de la notebook está cerrada o cuando              el obturador de privacidad está activado."
        );
    }
    anyhow!(
        "No se pudo abrir la cámara ({text}). Revisá que no la esté usando otra app y que Windows          permita el acceso en Configuración → Privacidad y seguridad → Cámara."
    )
}

/// Publica un cuadro RGB: lo deja como "último cuadro" para el grabador y,
/// cuadro por medio, genera el JPEG de la vista previa.
fn publish(shared: &CamShared, data: Arc<Vec<u8>>, w: u32, h: u32, n: u64) {
    if n % 2 == 0 {
        if let Some(jpeg) = make_preview(&data, w, h) {
            let mut p = shared.preview.lock();
            p.jpeg = Arc::new(jpeg);
            p.seq += 1;
            drop(p);
            shared.preview_cv.notify_all();
        }
    }
    let mut f = shared.frame.lock();
    f.data = data;
    f.width = w;
    f.height = h;
    f.seq += 1;
    drop(f);
    shared.active.store(true, Ordering::SeqCst);
}

#[inline(always)]
fn yuv_to_rgb(y: u8, u: u8, v: u8, out: &mut [u8]) {
    // BT.601, rango de video, en punto fijo.
    let c = 298 * (y as i32 - 16) + 128;
    let (d, e) = (u as i32 - 128, v as i32 - 128);
    out[0] = ((c + 409 * e) >> 8).clamp(0, 255) as u8;
    out[1] = ((c - 100 * d - 208 * e) >> 8).clamp(0, 255) as u8;
    out[2] = ((c + 516 * d) >> 8).clamp(0, 255) as u8;
}

/// YUYV (YUY2) 4:2:2 -> RGB24. Devuelve `None` si el buffer no tiene el tamaño esperado.
pub fn yuyv_to_rgb(src: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let pixels = (w * h) as usize;
    if w % 2 != 0 || src.len() < pixels * 2 {
        return None;
    }
    let mut rgb = vec![0u8; pixels * 3];
    for (yuyv, out) in src[..pixels * 2].chunks_exact(4).zip(rgb.chunks_exact_mut(6)) {
        yuv_to_rgb(yuyv[0], yuyv[1], yuyv[3], &mut out[..3]);
        yuv_to_rgb(yuyv[2], yuyv[1], yuyv[3], &mut out[3..]);
    }
    Some(rgb)
}

/// NV12 4:2:0 (plano Y seguido de UV intercalado) -> RGB24.
pub fn nv12_to_rgb(src: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let (w, h) = (w as usize, h as usize);
    if w % 2 != 0 || h % 2 != 0 || src.len() < w * h * 3 / 2 {
        return None;
    }
    let (luma, chroma) = src.split_at(w * h);
    let mut rgb = vec![0u8; w * h * 3];
    for (row, out_row) in rgb.chunks_exact_mut(w * 3).enumerate() {
        let y_row = &luma[row * w..(row + 1) * w];
        let uv_row = &chroma[(row / 2) * w..(row / 2 + 1) * w];
        for (x, out) in out_row.chunks_exact_mut(3).enumerate() {
            let uv = (x / 2) * 2;
            yuv_to_rgb(y_row[x], uv_row[uv], uv_row[uv + 1], out);
        }
    }
    Some(rgb)
}

/// Convierte un cuadro de la cámara a RGB24. YUYV y NV12 usan conversión
/// propia: la de nokhwa tarda cerca de medio segundo por cuadro en 720p.
fn frame_to_rgb(frame: &nokhwa::Buffer) -> Result<(Vec<u8>, u32, u32), nokhwa::NokhwaError> {
    let res = frame.resolution();
    let (w, h) = (res.width(), res.height());
    let fast = match frame.source_frame_format() {
        FrameFormat::YUYV => yuyv_to_rgb(frame.buffer(), w, h),
        FrameFormat::NV12 => nv12_to_rgb(frame.buffer(), w, h),
        _ => None,
    };
    match fast {
        Some(rgb) => Ok((rgb, w, h)),
        None => {
            let img = frame.decode_image::<RgbFormat>()?;
            let (w, h) = (img.width(), img.height());
            Ok((img.into_raw(), w, h))
        }
    }
}

/// Cuadros por segundo reales a partir de `intervals` cuadros recibidos en `elapsed`.
pub fn measured_fps(intervals: u64, elapsed: Duration) -> u32 {
    if intervals == 0 || elapsed.is_zero() {
        return 30;
    }
    ((intervals as f64 / elapsed.as_secs_f64()).round() as u32).clamp(1, 60)
}

/// Diagnóstico por consola (`dualrec --caminfo`): formatos de cada cámara.
pub fn print_diagnostics() {
    for info in list_cameras() {
        println!("== {} (indice {})", info.name, info.index);
        let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
        match Camera::new(CameraIndex::Index(info.index), requested) {
            Ok(mut cam) => {
                println!("formato por defecto: {}", cam.camera_format());
                match cam.compatible_camera_formats() {
                    Ok(mut formats) => {
                        formats.sort_by_key(|f| (f.format() as u32, f.width(), f.frame_rate()));
                        for f in formats {
                            println!("  {f}");
                        }
                    }
                    Err(e) => println!("no se pudieron listar formatos: {e}"),
                }
                match cam.open_stream().and_then(|_| cam.frame()) {
                    Ok(f) => println!("cuadro OK: {} {:?}", f.resolution(), f.source_frame_format()),
                    Err(e) => println!("no entrega cuadros: {e}"),
                }
            }
            Err(e) => println!("no se pudo abrir: {e}"),
        }
        // Velocidad real con el formato que usa la app, separando captura y conversión.
        match open_best(info.index) {
            Ok(mut cam) => {
                println!("formato elegido: {}", cam.camera_format());
                let _ = cam.frame();
                let (mut capture, mut decode) = (Duration::ZERO, Duration::ZERO);
                const N: u32 = 15;
                for _ in 0..N {
                    let t = std::time::Instant::now();
                    let frame = cam.frame();
                    capture += t.elapsed();
                    let t = std::time::Instant::now();
                    if let Ok(f) = frame {
                        let _ = frame_to_rgb(&f);
                    }
                    decode += t.elapsed();
                }
                println!(
                    "por cuadro: captura {:.0} ms, conversion {:.0} ms",
                    capture.as_secs_f64() * 1000.0 / N as f64,
                    decode.as_secs_f64() * 1000.0 / N as f64
                );
            }
            Err(e) => println!("open_best fallo: {e}"),
        }
    }
}

/// Abre la cámara probando formatos de mejor a peor: 720p a 30 fps en los
/// formatos de píxel habituales y, si la cámara no los ofrece, lo que tenga.
fn open_best(index: u32) -> Result<Camera, nokhwa::NokhwaError> {
    let hd = Resolution::new(1280, 720);
    let mut attempts: Vec<RequestedFormatType> = [FrameFormat::MJPEG, FrameFormat::NV12, FrameFormat::YUYV]
        .into_iter()
        .map(|f| RequestedFormatType::Closest(CameraFormat::new(hd, f, 30)))
        .collect();
    attempts.extend([
        RequestedFormatType::HighestResolution(hd),
        RequestedFormatType::AbsoluteHighestFrameRate,
        RequestedFormatType::AbsoluteHighestResolution,
        RequestedFormatType::None,
    ]);
    let mut last_error = None;
    for attempt in attempts {
        let requested = RequestedFormat::new::<RgbFormat>(attempt);
        match Camera::new(CameraIndex::Index(index), requested)
            .and_then(|mut c| c.open_stream().map(|_| c))
        {
            Ok(c) => return Ok(c),
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.expect("hay al menos un intento"))
}

impl CameraHandle {
    pub fn open(index: u32, shared: Arc<CamShared>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(u32, u32, u32, String)>>(1);
        let stop_t = stop.clone();

        let thread = std::thread::Builder::new()
            .name("dualrec-camera".into())
            .spawn(move || {
                let mut cam = match open_best(index) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready_tx.send(Err(friendly_error(e)));
                        return;
                    }
                };

                // Algunos drivers informan mal los cuadros por segundo (por ejemplo
                // "1 fps" entregando 30), así que se mide lo que llega de verdad.
                const MEASURE_FRAMES: u64 = 12;
                let mut first_frame_at: Option<std::time::Instant> = None;
                let mut announced = false;
                let mut n: u64 = 0;
                let mut errors = 0u32;
                while !stop_t.load(Ordering::Relaxed) {
                    let decoded = cam.frame().and_then(|f| frame_to_rgb(&f));
                    let (rgb, w, h) = match decoded {
                        Ok(x) => x,
                        Err(e) => {
                            errors += 1;
                            if !announced && errors > 30 {
                                let _ = ready_tx.send(Err(friendly_error(e)));
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    };
                    errors = 0;
                    let data = Arc::new(rgb);
                    let started = *first_frame_at.get_or_insert_with(std::time::Instant::now);
                    if !announced && (n >= MEASURE_FRAMES || started.elapsed() > Duration::from_secs(2)) {
                        announced = true;
                        let fps = measured_fps(n, started.elapsed());
                        let _ = ready_tx.send(Ok((w, h, fps, cam.info().human_name())));
                    }
                    publish(&shared, data, w, h, n);
                    n += 1;
                }
                shared.active.store(false, Ordering::SeqCst);
                let _ = cam.stop_stream();
            })?;

        match ready_rx.recv_timeout(Duration::from_secs(12)) {
            Ok(Ok((width, height, fps, name))) => Ok(Self {
                index,
                name,
                width,
                height,
                fps,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err(anyhow!("La cámara no respondió a tiempo."))
            }
        }
    }
}

impl CameraHandle {
    /// Cámara sintética (patrón animado) para probar la grabación y la burbuja
    /// sin hardware: `DUALREC_FAKE_CAMERA=1` o `--selftest ... --fake-camera`.
    pub fn open_test_pattern(shared: Arc<CamShared>) -> Result<Self> {
        const W: u32 = 1280;
        const H: u32 = 720;
        const FPS: u32 = 30;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let thread = std::thread::Builder::new()
            .name("dualrec-camera-test".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                let mut n: u64 = 0;
                let noisy = std::env::var("DUALREC_FAKE_NOISE").is_ok();
                let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
                while !stop_t.load(Ordering::Relaxed) {
                    let t = started.elapsed().as_secs_f32();
                    let mut data = vec![0u8; (W * H * 3) as usize];
                    // Degradé de fondo y una barra vertical que cruza la imagen cada 2 s.
                    let bar = ((t % 2.0) / 2.0 * W as f32) as u32;
                    for y in 0..H {
                        let row = (y * W * 3) as usize;
                        for x in 0..W {
                            let i = row + (x * 3) as usize;
                            let on_bar = x.abs_diff(bar) < 24;
                            data[i] = if on_bar { 255 } else { (x * 160 / W) as u8 + 40 };
                            data[i + 1] = if on_bar { 255 } else { (y * 120 / H) as u8 + 30 };
                            data[i + 2] = if on_bar { 255 } else { 150 };
                        }
                    }
                    if noisy {
                        // Ruido: no se comprime, así una prueba corta genera
                        // archivos grandes (sirve para probar la subida en partes).
                        for px in data.chunks_exact_mut(3).step_by(2) {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            px[0] = seed as u8;
                            px[1] = (seed >> 8) as u8;
                            px[2] = (seed >> 16) as u8;
                        }
                    }
                    publish(&shared, Arc::new(data), W, H, n);
                    n += 1;
                    let next = Duration::from_secs_f64(n as f64 / FPS as f64);
                    std::thread::sleep(next.saturating_sub(started.elapsed()));
                }
                shared.active.store(false, Ordering::SeqCst);
            })?;
        Ok(Self {
            index: u32::MAX,
            name: "Cámara de prueba".into(),
            width: W,
            height: H,
            fps: FPS,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for CameraHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_and_nv12_convert_known_colors() {
        // Gris medio (Y=126, U=V=128), blanco (Y=235) y rojo puro (Y=81, U=90, V=240).
        let near = |a: &[u8], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2);
        let yuyv = [126, 128, 235, 128, 81, 90, 81, 240];
        let rgb = yuyv_to_rgb(&yuyv, 4, 1).unwrap();
        assert!(near(&rgb[0..3], [128, 128, 128]), "{:?}", &rgb[0..3]);
        assert!(near(&rgb[3..6], [255, 255, 255]), "{:?}", &rgb[3..6]);
        assert!(near(&rgb[6..9], [255, 0, 0]), "{:?}", &rgb[6..9]);
        // NV12 2x2 todo rojo: 4 de luma y un par UV.
        let rgb = nv12_to_rgb(&[81, 81, 81, 81, 90, 240], 2, 2).unwrap();
        assert!(rgb.chunks(3).all(|p| near(p, [255, 0, 0])), "{rgb:?}");
        // Un buffer más chico de lo esperado no se convierte (ni se lee fuera de rango).
        assert!(yuyv_to_rgb(&[0; 7], 4, 1).is_none());
        assert!(nv12_to_rgb(&[0; 5], 2, 2).is_none());
    }

    #[test]
    fn fps_is_measured_from_real_frames() {
        // 12 intervalos en 400 ms son 30 fps, diga lo que diga el driver.
        assert_eq!(measured_fps(12, Duration::from_millis(400)), 30);
        assert_eq!(measured_fps(12, Duration::from_millis(800)), 15);
        assert_eq!(measured_fps(0, Duration::ZERO), 30);
    }

    #[test]
    fn preview_is_a_small_jpeg() {
        let (w, h) = (1280u32, 720u32);
        let rgb = vec![120u8; (w * h * 3) as usize];
        let jpeg = make_preview(&rgb, w, h).expect("jpeg");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert!(jpeg.len() < rgb.len() / 20);
    }
}
