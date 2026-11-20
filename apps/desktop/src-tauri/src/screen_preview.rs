//! Vista previa en vivo de la pantalla o ventana elegida.
//!
//! Un hilo mantiene una captura liviana (pocos cuadros por segundo) del
//! destino seleccionado en la interfaz, la achica y la publica como JPEG en
//! un `Feed` que sirve `preview.rs`. Es independiente de la captura de la
//! grabación, así que sigue andando mientras se graba.

use crate::preview::Feed;
use crate::screen::{ScreenCapture, ScreenTarget};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_capture::graphics_capture_api::GraphicsCaptureApi;

const FPS: u32 = 4;
const MAX_W: u32 = 800;

pub struct ScreenPreview {
    /// (destino, generación). La generación sube con cada cambio.
    target: Mutex<(Option<ScreenTarget>, u64)>,
}

impl ScreenPreview {
    pub fn spawn(feed: Arc<Feed>) -> Arc<Self> {
        let me = Arc::new(Self {
            target: Mutex::new((None, 0)),
        });
        let worker = me.clone();
        let _ = std::thread::Builder::new()
            .name("dualrec-screen-preview".into())
            .spawn(move || worker.run(feed));
        me
    }

    pub fn set_target(&self, target: Option<ScreenTarget>) {
        let mut t = self.target.lock();
        t.0 = target;
        t.1 += 1;
    }

    fn run(&self, feed: Arc<Feed>) {
        // Sin soporte para capturar sin borde (Windows 10 viejo), una vista
        // previa permanente dejaría el borde amarillo siempre a la vista.
        if !GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
            return;
        }
        let mut gen = 0u64;
        let mut target: Option<ScreenTarget> = None;
        let mut cap: Option<ScreenCapture> = None;
        let mut last_seq = 0u64;
        let mut retry_at = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(1000 / FPS as u64));
            {
                let t = self.target.lock();
                if t.1 != gen {
                    gen = t.1;
                    target = t.0.clone();
                    cap = None;
                    last_seq = 0;
                    retry_at = Instant::now();
                    feed.publish(Vec::new());
                }
            }
            if cap.as_ref().is_some_and(|c| {
                c.shared.closed.load(std::sync::atomic::Ordering::SeqCst)
            }) {
                cap = None;
                feed.publish(Vec::new());
            }
            let Some(t) = &target else { continue };
            if cap.is_none() {
                // Una ventana minimizada o cerrada no se puede capturar: se reintenta cada tanto.
                if Instant::now() < retry_at {
                    continue;
                }
                retry_at = Instant::now() + Duration::from_secs(2);
                cap = ScreenCapture::start(t, FPS, true).ok();
                last_seq = 0;
            }
            let Some(c) = &cap else { continue };
            let jpeg = {
                let slot = c.shared.slot.lock();
                if slot.seq == last_seq || slot.width == 0 {
                    None
                } else {
                    last_seq = slot.seq;
                    encode(&slot.data, slot.width, slot.height)
                }
            };
            if let Some(jpeg) = jpeg {
                feed.publish(jpeg);
            }
        }
    }
}

/// Achica un cuadro BGRA promediando bloques y lo codifica como JPEG.
pub fn encode(bgra: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let step = w.div_ceil(MAX_W).max(1) as usize;
    let (w, h) = (w as usize, h as usize);
    let (pw, ph) = (w / step, h / step);
    if pw == 0 || ph == 0 || bgra.len() < w * h * 4 {
        return None;
    }
    // Hasta 2x2 muestras repartidas dentro de cada bloque: alcanza para que el texto no se vea dentado.
    let offs: &[usize] = if step >= 2 { &[step / 4, step * 3 / 4] } else { &[0] };
    let n = (offs.len() * offs.len()) as u32;
    let mut small = Vec::with_capacity(pw * ph * 3);
    for y in 0..ph {
        for x in 0..pw {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for oy in offs {
                let row = (y * step + oy) * w * 4;
                for ox in offs {
                    let i = row + (x * step + ox) * 4;
                    b += bgra[i] as u32;
                    g += bgra[i + 1] as u32;
                    r += bgra[i + 2] as u32;
                }
            }
            small.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8]);
        }
    }
    let mut out = Vec::with_capacity(64 * 1024);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70)
        .encode(&small, pw as u32, ph as u32, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_shrinks_to_a_jpeg() {
        let (w, h) = (1920u32, 1080u32);
        let bgra = vec![128u8; (w * h * 4) as usize];
        let jpeg = encode(&bgra, w, h).expect("jpeg");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let img = image::load_from_memory(&jpeg).expect("decodifica");
        assert_eq!((img.width(), img.height()), (640, 360));
    }

    #[test]
    fn encode_rejects_short_buffers() {
        assert!(encode(&[0u8; 16], 100, 100).is_none());
    }
}
