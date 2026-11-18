//! Captura de pantalla o ventana con Windows Graphics Capture.
//!
//! WGC solo entrega cuadros cuando la imagen cambia, así que acá únicamente se
//! guarda "el último cuadro"; el ritmo constante lo pone el grabador con el
//! reloj común (ver `recorder.rs`).

use anyhow::{anyhow, Result};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

type HandlerError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ScreenTarget {
    Monitor { index: usize },
    Window { hwnd: isize },
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub primary: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub process: String,
}

pub fn list_monitors() -> Vec<MonitorInfo> {
    let primary = Monitor::primary()
        .ok()
        .and_then(|m| m.device_name().ok());
    Monitor::enumerate()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let device = m.device_name().unwrap_or_default();
            let friendly = m
                .name()
                .ok()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("Pantalla {}", i + 1));
            MonitorInfo {
                index: i,
                name: friendly,
                width: m.width().unwrap_or(0),
                height: m.height().unwrap_or(0),
                refresh_rate: m.refresh_rate().unwrap_or(60),
                primary: primary.as_deref() == Some(device.as_str()),
            }
        })
        .collect()
}

pub fn list_windows() -> Vec<WindowInfo> {
    let me = std::process::id();
    Window::enumerate()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.is_valid() && w.process_id().map(|p| p != me).unwrap_or(true))
        .filter_map(|w| {
            let title = w.title().ok()?;
            if title.trim().is_empty() {
                return None;
            }
            Some(WindowInfo {
                hwnd: w.as_raw_hwnd() as isize,
                title,
                process: w
                    .process_name()
                    .unwrap_or_default()
                    .trim_end_matches(".exe")
                    .to_string(),
            })
        })
        .collect()
}

pub fn target_label(target: &ScreenTarget) -> String {
    match target {
        ScreenTarget::Monitor { index } => list_monitors()
            .into_iter()
            .find(|m| m.index == *index)
            .map(|m| m.name)
            .unwrap_or_else(|| format!("Pantalla {}", index + 1)),
        ScreenTarget::Window { hwnd } => Window::from_raw_hwnd(*hwnd as *mut _)
            .title()
            .unwrap_or_else(|_| "Ventana".into()),
    }
}

/// Último cuadro capturado, en BGRA sin padding y con tamaño fijo.
pub struct Slot {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub seq: u64,
}

pub struct ScreenShared {
    pub slot: Mutex<Slot>,
    pub cv: Condvar,
    pub closed: AtomicBool,
}

struct Handler {
    shared: Arc<ScreenShared>,
    scratch: Vec<u8>,
}

/// Copia `src` (sw x sh) en un lienzo fijo `dst` (dw x dh), recortando o
/// rellenando con negro. Se usa cuando la ventana cambia de tamaño a mitad de
/// la grabación: el video mantiene el tamaño con el que empezó.
pub fn fit_into(dst: &mut [u8], dw: u32, dh: u32, src: &[u8], sw: u32, sh: u32) {
    let (dw, dh, sw, sh) = (dw as usize, dh as usize, sw as usize, sh as usize);
    for px in dst.chunks_exact_mut(4) {
        px.copy_from_slice(&[0, 0, 0, 255]);
    }
    let cols = dw.min(sw) * 4;
    for y in 0..dh.min(sh) {
        dst[y * dw * 4..y * dw * 4 + cols].copy_from_slice(&src[y * sw * 4..y * sw * 4 + cols]);
    }
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Arc<ScreenShared>;
    type Error = HandlerError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            shared: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _ctl: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let fb = frame.buffer()?;
        let (w, h) = (fb.width(), fb.height());
        let src = fb.as_nopadding_buffer(&mut self.scratch);
        if w == 0 || h == 0 || src.len() < (w * h * 4) as usize {
            return Ok(());
        }
        let mut slot = self.shared.slot.lock();
        if slot.seq == 0 {
            slot.width = w;
            slot.height = h;
            slot.data = src[..(w * h * 4) as usize].to_vec();
        } else if (w, h) == (slot.width, slot.height) {
            slot.data.copy_from_slice(&src[..(w * h * 4) as usize]);
        } else {
            let (dw, dh) = (slot.width, slot.height);
            fit_into(&mut slot.data, dw, dh, src, w, h);
        }
        slot.seq += 1;
        drop(slot);
        self.shared.cv.notify_all();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.shared.closed.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
        Ok(())
    }
}

pub struct ScreenCapture {
    pub shared: Arc<ScreenShared>,
    ctl: Option<CaptureControl<Handler, HandlerError>>,
}

impl ScreenCapture {
    pub fn start(target: &ScreenTarget, fps: u32, cursor: bool) -> Result<Self> {
        let shared = Arc::new(ScreenShared {
            slot: Mutex::new(Slot {
                data: Vec::new(),
                width: 0,
                height: 0,
                seq: 0,
            }),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
        });

        let cursor = if !GraphicsCaptureApi::is_cursor_settings_supported().unwrap_or(false) {
            CursorCaptureSettings::Default
        } else if cursor {
            CursorCaptureSettings::WithCursor
        } else {
            CursorCaptureSettings::WithoutCursor
        };
        let border = if GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
            DrawBorderSettings::WithoutBorder
        } else {
            DrawBorderSettings::Default
        };
        let interval =
            if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
                MinimumUpdateIntervalSettings::Custom(Duration::from_nanos(
                    1_000_000_000 / fps.max(1) as u64,
                ))
            } else {
                MinimumUpdateIntervalSettings::Default
            };

        macro_rules! launch {
            ($item:expr) => {
                Handler::start_free_threaded(Settings::new(
                    $item,
                    cursor,
                    border,
                    SecondaryWindowSettings::Default,
                    interval,
                    DirtyRegionSettings::Default,
                    ColorFormat::Bgra8,
                    shared.clone(),
                ))
                .map_err(|e| anyhow!("No se pudo iniciar la captura de pantalla: {e}"))?
            };
        }

        let ctl = match target {
            ScreenTarget::Monitor { index } => {
                let monitor = Monitor::enumerate()
                    .map_err(|e| anyhow!("No se pudieron listar las pantallas: {e}"))?
                    .into_iter()
                    .nth(*index)
                    .ok_or_else(|| anyhow!("La pantalla elegida ya no está conectada."))?;
                launch!(monitor)
            }
            ScreenTarget::Window { hwnd } => {
                let window = Window::from_raw_hwnd(*hwnd as *mut _);
                if !window.is_valid() {
                    return Err(anyhow!("La ventana elegida ya no existe."));
                }
                launch!(window)
            }
        };

        Ok(Self {
            shared,
            ctl: Some(ctl),
        })
    }

    /// Espera el primer cuadro y devuelve el tamaño fijo de la pista.
    pub fn wait_first_frame(&self, timeout: Duration) -> Result<(u32, u32)> {
        let deadline = Instant::now() + timeout;
        let mut slot = self.shared.slot.lock();
        while slot.seq == 0 {
            if self.shared.closed.load(Ordering::SeqCst) {
                return Err(anyhow!("La ventana se cerró antes de empezar a grabar."));
            }
            if self.shared.cv.wait_until(&mut slot, deadline).timed_out() && slot.seq == 0 {
                return Err(anyhow!(
                    "La pantalla no entregó imagen. Si elegiste una ventana, fijate que no esté minimizada."
                ));
            }
        }
        Ok((slot.width, slot.height))
    }

    pub fn stop(&mut self) {
        if let Some(ctl) = self.ctl.take() {
            let _ = ctl.stop();
        }
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_crops_and_pads() {
        // Origen 3x1 en un lienzo 2x2: se recorta a 2 columnas y la fila 2 queda negra.
        let src = [1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        let mut dst = vec![9u8; 2 * 2 * 4];
        fit_into(&mut dst, 2, 2, &src, 3, 1);
        assert_eq!(&dst[..8], &[1, 1, 1, 1, 2, 2, 2, 2]);
        assert_eq!(&dst[8..], &[0, 0, 0, 255, 0, 0, 0, 255]);
    }
}
