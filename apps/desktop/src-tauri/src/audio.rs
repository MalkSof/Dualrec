//! Audio con cpal: micrófono (entrada) y audio del sistema (WASAPI loopback,
//! que cpal activa al abrir un stream de entrada sobre un dispositivo de salida).

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|x| x.name().to_string())
}

pub fn list_inputs() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| device_name(&d))
}

pub fn default_output_name() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| device_name(&d))
}

#[derive(Clone, Debug)]
pub enum AudioSource {
    /// Micrófono por nombre; `None` usa el predeterminado.
    Input(Option<String>),
    /// Lo que suena por la salida predeterminada.
    SystemLoopback,
}

type Sink = Arc<Mutex<Option<Sender<Vec<f32>>>>>;

pub struct AudioCapture {
    pub name: String,
    pub rate: u32,
    pub channels: u16,
    /// Pico reciente (0..1) como bits de f32, para el medidor de nivel.
    peak: Arc<AtomicU32>,
    sink: Sink,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioCapture {
    pub fn open(source: AudioSource) -> Result<Self> {
        let peak = Arc::new(AtomicU32::new(0));
        let sink: Sink = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<(String, u32, u16)>>(1);
        let (peak_t, sink_t, stop_t) = (peak.clone(), sink.clone(), stop.clone());

        // El stream de cpal no es `Send`: vive entero dentro de este hilo.
        let thread = std::thread::Builder::new()
            .name("dualrec-audio".into())
            .spawn(move || {
                let built = build_stream(&source, peak_t, sink_t);
                let stream = match built {
                    Ok((stream, name, rate, channels)) => {
                        let _ = ready_tx.send(Ok((name, rate, channels)));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while !stop_t.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(stream);
            })?;

        match ready_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok((name, rate, channels))) => Ok(Self {
                name,
                rate,
                channels,
                peak,
                sink,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err(anyhow!("El dispositivo de audio no respondió a tiempo."))
            }
        }
    }

    /// Empieza a enviar los bloques de audio (f32 intercalado) a `tx`.
    pub fn attach(&self, tx: Sender<Vec<f32>>) {
        *self.sink.lock() = Some(tx);
    }

    pub fn detach(&self) {
        *self.sink.lock() = None;
    }

    /// Devuelve el pico acumulado desde la última lectura y lo reinicia.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.detach();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn deliver(data: &[f32], peak: &AtomicU32, sink: &Sink) {
    let mut max = 0f32;
    for s in data {
        let a = s.abs();
        if a > max {
            max = a;
        }
    }
    let prev = f32::from_bits(peak.load(Ordering::Relaxed));
    if max > prev {
        peak.store(max.to_bits(), Ordering::Relaxed);
    }
    if let Some(tx) = sink.lock().as_ref() {
        let _ = tx.try_send(data.to_vec());
    }
}

fn build_stream(
    source: &AudioSource,
    peak: Arc<AtomicU32>,
    sink: Sink,
) -> Result<(cpal::Stream, String, u32, u16)> {
    let host = cpal::default_host();
    let (device, supported) = match source {
        AudioSource::Input(wanted) => {
            let device = match wanted {
                Some(name) => host
                    .input_devices()
                    .map_err(|e| anyhow!("No se pudieron listar los micrófonos: {e}"))?
                    .find(|d| device_name(d).as_deref() == Some(name.as_str()))
                    .or_else(|| host.default_input_device()),
                None => host.default_input_device(),
            }
            .ok_or_else(|| anyhow!("No hay ningún micrófono conectado."))?;
            let cfg = device.default_input_config().map_err(|e| {
                anyhow!(
                    "No se pudo abrir el micrófono ({e}). Revisá Configuración → Privacidad y \
                     seguridad → Micrófono."
                )
            })?;
            (device, cfg)
        }
        AudioSource::SystemLoopback => {
            let device = host
                .default_output_device()
                .ok_or_else(|| anyhow!("No hay un dispositivo de salida de audio."))?;
            let cfg = device
                .default_output_config()
                .map_err(|e| anyhow!("No se pudo leer el audio del sistema: {e}"))?;
            (device, cfg)
        }
    };

    let name = device_name(&device).unwrap_or_else(|| "Audio".into());
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate: u32 = config.sample_rate;
    let channels = config.channels;
    let on_error = |e| eprintln!("[dualrec] error de audio: {e}");

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| deliver(data, &peak, &sink),
            on_error,
            None,
        ),
        SampleFormat::I16 => {
            let mut buf: Vec<f32> = Vec::new();
            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    buf.clear();
                    buf.extend(data.iter().map(|s| *s as f32 / 32768.0));
                    deliver(&buf, &peak, &sink);
                },
                on_error,
                None,
            )
        }
        other => return Err(anyhow!("Formato de audio no soportado: {other:?}")),
    }
    .map_err(|e| anyhow!("No se pudo abrir el audio de «{name}»: {e}"))?;

    stream
        .play()
        .map_err(|e| anyhow!("No se pudo iniciar el audio de «{name}»: {e}"))?;
    Ok((stream, name, rate, channels))
}
