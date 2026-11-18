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

                let mut announced = false;
                let mut n: u64 = 0;
                let mut errors = 0u32;
                while !stop_t.load(Ordering::Relaxed) {
                    let decoded = cam
                        .frame()
                        .and_then(|f| f.decode_image::<RgbFormat>());
                    let img = match decoded {
                        Ok(i) => i,
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
                    let (w, h) = (img.width(), img.height());
                    let data = Arc::new(img.into_raw());
                    if !announced {
                        announced = true;
                        let fps = cam.frame_rate().max(1);
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
    fn preview_is_a_small_jpeg() {
        let (w, h) = (1280u32, 720u32);
        let rgb = vec![120u8; (w * h * 3) as usize];
        let jpeg = make_preview(&rgb, w, h).expect("jpeg");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert!(jpeg.len() < rgb.len() / 20);
    }
}
