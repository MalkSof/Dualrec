//! Reloj monotónico común a todas las pistas.
//!
//! Todas las pistas (pantalla, cámara, micrófono, audio del sistema) miden el
//! tiempo contra el mismo `t0`. El tiempo "activo" excluye las pausas, así que
//! el cuadro N de video y la muestra M de audio caen siempre en el mismo
//! instante de la línea de tiempo, sin importar a qué ritmo entregue datos
//! cada dispositivo.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

struct Inner {
    start: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
}

pub struct RecClock {
    inner: Mutex<Inner>,
}

/// Tiempo activo (sin pausas) transcurrido en `now`.
pub fn active_at(
    start: Instant,
    now: Instant,
    paused_total: Duration,
    paused_at: Option<Instant>,
) -> Duration {
    let end = paused_at.unwrap_or(now);
    end.saturating_duration_since(start)
        .saturating_sub(paused_total)
}

/// Cantidad de cuadros que ya deberían estar escritos: el cuadro `k` corresponde
/// al instante `k / fps`, así que en `t = 0` ya se debe el primero.
pub fn frames_due(elapsed: Duration, fps: u32) -> u64 {
    (elapsed.as_nanos() * fps as u128 / 1_000_000_000) as u64 + 1
}

/// Cantidad de muestras de audio (por canal) que corresponden a `elapsed`.
pub fn samples_due(elapsed: Duration, rate: u32) -> u64 {
    (elapsed.as_nanos() * rate as u128 / 1_000_000_000) as u64
}

impl RecClock {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                start: Instant::now(),
                paused_total: Duration::ZERO,
                paused_at: None,
            }),
        }
    }

    pub fn pause(&self) {
        let mut i = self.inner.lock();
        if i.paused_at.is_none() {
            i.paused_at = Some(Instant::now());
        }
    }

    pub fn resume(&self) {
        let mut i = self.inner.lock();
        if let Some(at) = i.paused_at.take() {
            i.paused_total += at.elapsed();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.inner.lock().paused_at.is_some()
    }

    pub fn elapsed(&self) -> Duration {
        let i = self.inner.lock();
        active_at(i.start, Instant::now(), i.paused_total, i.paused_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_time_excludes_pauses() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        // 10 s de reloj, 3 s en pausa ya terminada.
        assert_eq!(active_at(t0, t0 + s(10), s(3), None), s(7));
        // En pausa desde el segundo 8: el tiempo queda congelado.
        assert_eq!(active_at(t0, t0 + s(20), s(3), Some(t0 + s(8))), s(5));
    }

    #[test]
    fn frames_and_samples_share_the_timeline() {
        assert_eq!(frames_due(Duration::ZERO, 30), 1);
        assert_eq!(frames_due(Duration::from_millis(33), 30), 1);
        assert_eq!(frames_due(Duration::from_millis(34), 30), 2);
        assert_eq!(frames_due(Duration::from_secs(600), 30), 18_001);
        assert_eq!(samples_due(Duration::from_secs(600), 48_000), 28_800_000);
        // Tras 10 minutos, video y audio difieren en menos de un cuadro.
        let d = Duration::from_secs(600);
        let video_s = (frames_due(d, 30) - 1) as f64 / 30.0;
        let audio_s = samples_due(d, 48_000) as f64 / 48_000.0;
        assert!((video_s - audio_s).abs() < 1.0 / 30.0);
    }

    #[test]
    fn pause_and_resume_freeze_the_clock() {
        let c = RecClock::new();
        c.pause();
        let a = c.elapsed();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(a, c.elapsed());
        assert!(c.is_paused());
        c.resume();
        std::thread::sleep(Duration::from_millis(10));
        assert!(c.elapsed() > a);
    }
}
