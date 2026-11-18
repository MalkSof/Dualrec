// En release no se abre una consola junto a la ventana.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod camera;
mod clock;
mod cloud;
mod ffmpeg;
mod preview;
mod recorder;
mod screen;

use audio::{AudioCapture, AudioSource};
use camera::{CamShared, CameraHandle};
use cloud::{CloudConfig, CloudProgress, Upload, UploadState};
use parking_lot::Mutex;
use recorder::{CameraSource, Manifest, RecOptions, Session, Sources};
use screen::ScreenTarget;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

struct AppState {
    cam_shared: Arc<CamShared>,
    camera: Mutex<Option<CameraHandle>>,
    mic: Mutex<Option<Arc<AudioCapture>>>,
    session: Mutex<Option<Session>>,
    /// Subidas a la nube de esta ejecución; la más nueva va primero.
    uploads: Mutex<Vec<Arc<Upload>>>,
    preview_port: u16,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn default_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Dualrec")
}

#[derive(Serialize)]
struct Devices {
    monitors: Vec<screen::MonitorInfo>,
    windows: Vec<screen::WindowInfo>,
    cameras: Vec<camera::CameraInfo>,
    mics: Vec<String>,
    default_mic: Option<String>,
    system_output: Option<String>,
    preview_url: String,
    default_dir: String,
    ffmpeg_ok: bool,
}

#[tauri::command(async)]
fn get_devices(state: State<AppState>) -> Devices {
    Devices {
        monitors: screen::list_monitors(),
        windows: screen::list_windows(),
        cameras: camera::list_cameras(),
        mics: audio::list_inputs(),
        default_mic: audio::default_input_name(),
        system_output: audio::default_output_name(),
        preview_url: format!("http://127.0.0.1:{}/cam.mjpg", state.preview_port),
        default_dir: default_dir().to_string_lossy().into_owned(),
        ffmpeg_ok: ffmpeg::available(),
    }
}

#[tauri::command]
fn get_preview_url(state: State<AppState>) -> String {
    format!("http://127.0.0.1:{}/cam.mjpg", state.preview_port)
}

/// Detecta el encoder (hace codificaciones de prueba, tarda un momento).
#[tauri::command(async)]
fn get_encoder() -> String {
    ffmpeg::detect_encoder().label().to_string()
}

#[derive(Serialize)]
struct CameraDesc {
    index: u32,
    name: String,
    width: u32,
    height: u32,
    fps: u32,
}

#[tauri::command(async)]
fn set_camera(state: State<AppState>, index: Option<u32>) -> CmdResult<Option<CameraDesc>> {
    if state.session.lock().is_some() {
        return Err("No se puede cambiar la cámara mientras se graba.".into());
    }
    let mut slot = state.camera.lock();
    if let (Some(current), Some(wanted)) = (slot.as_ref(), index) {
        if current.index == wanted {
            return Ok(Some(describe(current)));
        }
    }
    // Se suelta la cámara anterior antes de abrir la nueva.
    *slot = None;
    {
        let mut f = state.cam_shared.frame.lock();
        *f = Default::default();
    }
    let Some(index) = index else {
        return Ok(None);
    };
    let handle = if std::env::var("DUALREC_FAKE_CAMERA").is_ok() {
        CameraHandle::open_test_pattern(state.cam_shared.clone())
    } else {
        CameraHandle::open(index, state.cam_shared.clone())
    }
    .map_err(err)?;
    let desc = describe(&handle);
    *slot = Some(handle);
    Ok(Some(desc))
}

fn describe(c: &CameraHandle) -> CameraDesc {
    CameraDesc {
        index: c.index,
        name: c.name.clone(),
        width: c.width,
        height: c.height,
        fps: c.fps,
    }
}

#[derive(Serialize)]
struct MicDesc {
    name: String,
    rate: u32,
    channels: u16,
}

#[tauri::command(async)]
fn set_mic(state: State<AppState>, enabled: bool, name: Option<String>) -> CmdResult<Option<MicDesc>> {
    if state.session.lock().is_some() {
        return Err("No se puede cambiar el micrófono mientras se graba.".into());
    }
    let mut slot = state.mic.lock();
    *slot = None;
    if !enabled {
        return Ok(None);
    }
    let cap = AudioCapture::open(AudioSource::Input(name)).map_err(err)?;
    let desc = MicDesc {
        name: cap.name.clone(),
        rate: cap.rate,
        channels: cap.channels,
    };
    *slot = Some(Arc::new(cap));
    Ok(Some(desc))
}

#[derive(Deserialize)]
struct StartRequest {
    screen: Option<ScreenTarget>,
    camera: bool,
    mic: bool,
    system_audio: bool,
    options: RecOptions,
    /// Si viene, la grabación se sube a la nube mientras se graba.
    #[serde(default)]
    cloud: Option<CloudConfig>,
    #[serde(default)]
    title: Option<String>,
}

/// Estado inicial de la subida de una grabación recién empezada.
fn upload_state(manifest: &Manifest, cfg: &CloudConfig, title: Option<String>) -> UploadState {
    UploadState {
        api_url: cfg.api_url.clone(),
        client_id: manifest.id.clone(),
        title: title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Grabación {}", manifest.id.replace('_', " "))),
        recorded_at: manifest.recorded_at.clone(),
        video_id: None,
        share_url: None,
        part_size: 0,
        duration_ms: None,
        tracks: manifest
            .tracks
            .iter()
            .map(|t| cloud::TrackState {
                kind: t.kind.clone(),
                codec: t.codec.clone(),
                width: t.width,
                height: t.height,
                offset_ms: t.offset_ms,
                file: recorder::part_name(&t.file),
                track_id: None,
                parts: Vec::new(),
            })
            .collect(),
        completed: false,
        failed: None,
    }
}

#[tauri::command(async)]
fn start_recording(state: State<AppState>, request: StartRequest) -> CmdResult<Manifest> {
    let mut session = state.session.lock();
    if session.is_some() {
        return Err("Ya hay una grabación en curso.".into());
    }
    let camera = if request.camera {
        let cam = state.camera.lock();
        let cam = cam
            .as_ref()
            .ok_or("Elegí una cámara antes de grabar.")?;
        Some(CameraSource {
            shared: state.cam_shared.clone(),
            name: cam.name.clone(),
            fps: cam.fps,
        })
    } else {
        None
    };
    let mic = if request.mic {
        Some(
            state
                .mic
                .lock()
                .clone()
                .ok_or("Elegí un micrófono antes de grabar.")?,
        )
    } else {
        None
    };
    let sources = Sources {
        screen: request.screen,
        camera,
        mic,
        system_audio: request.system_audio,
    };
    let mut options = request.options;
    options.keep_parts = request.cloud.is_some();
    let s = Session::start(sources, &options).map_err(err)?;
    let manifest = s.manifest.clone();
    if let Some(cfg) = request.cloud {
        // Si la subida no puede arrancar, la grabación local sigue igual.
        let initial = upload_state(&manifest, &cfg, request.title);
        if let Ok(upload) = Upload::spawn(s.dir.clone(), cfg, initial) {
            state.uploads.lock().insert(0, upload);
        }
    }
    *session = Some(s);
    Ok(manifest)
}

#[tauri::command(async)]
fn pause_recording(state: State<AppState>) -> CmdResult<()> {
    state
        .session
        .lock()
        .as_ref()
        .map(|s| s.pause())
        .ok_or_else(|| "No hay una grabación en curso.".to_string())
}

#[tauri::command(async)]
fn resume_recording(state: State<AppState>) -> CmdResult<()> {
    state
        .session
        .lock()
        .as_ref()
        .map(|s| s.resume())
        .ok_or_else(|| "No hay una grabación en curso.".to_string())
}

#[tauri::command(async)]
fn stop_recording(state: State<AppState>) -> CmdResult<Manifest> {
    let session = state
        .session
        .lock()
        .take()
        .ok_or("No hay una grabación en curso.")?;
    Ok(finish_session(&state, session))
}

/// Corta la grabación y, si se estaba subiendo, avisa a la subida que ya no
/// llegan más datos y deja el link para compartir en el portapapeles.
fn finish_session(state: &AppState, session: Session) -> Manifest {
    let dir = session.dir.clone();
    let manifest = session.stop();
    let upload = state.uploads.lock().iter().find(|u| u.dir == dir).cloned();
    if let Some(upload) = upload {
        upload.finish(manifest.duration_ms);
        if let Some(link) = upload.share_url() {
            copy_to_clipboard(&link);
        }
    }
    manifest
}

fn copy_to_clipboard(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .is_ok()
}

#[tauri::command(async)]
fn copy_text(text: String) -> bool {
    copy_to_clipboard(&text)
}

/// En desarrollo, DUALREC_API_URL y DUALREC_TOKEN precargan los ajustes de nube.
#[tauri::command]
fn cloud_defaults() -> Option<(String, String)> {
    Some((std::env::var("DUALREC_API_URL").ok()?, std::env::var("DUALREC_TOKEN").ok()?))
}

/// Verifica la URL de la API y el token; devuelve el nombre del usuario.
#[tauri::command(async)]
fn cloud_check(cloud: CloudConfig) -> CmdResult<String> {
    cloud::check(&cloud).map_err(err)
}

/// Retoma las subidas que quedaron pendientes en `dir` (app cerrada, sin
/// internet, etc.). Devuelve cuántas se retomaron.
#[tauri::command(async)]
fn resume_uploads(state: State<AppState>, cloud: CloudConfig, dir: String) -> usize {
    let recording_dir = state.session.lock().as_ref().map(|s| s.dir.clone());
    let mut uploads = state.uploads.lock();
    let mut resumed = 0;
    for (path, mut pending) in cloud::pending(&PathBuf::from(dir)) {
        if uploads.iter().any(|u| u.dir == path) || recording_dir.as_ref() == Some(&path) {
            continue;
        }
        // Una grabación que quedó sin cerrar (la app se cerró a mitad): se sube lo que haya.
        if pending.duration_ms.is_none() {
            pending.duration_ms = Some(0);
        }
        pending.failed = None;
        if let Ok(upload) = Upload::spawn(path, cloud.clone(), pending) {
            uploads.push(upload);
            resumed += 1;
        }
    }
    resumed
}

#[derive(Serialize)]
struct Status {
    recording: bool,
    paused: bool,
    elapsed_ms: u64,
    screen_frames: u64,
    mic_peak: f32,
    camera_active: bool,
    /// Subida más reciente y cuántas siguen en curso.
    cloud: Option<CloudProgress>,
    uploads_active: usize,
}

// `async`: corre fuera del hilo de la ventana, que no debe esperar el candado
// de la sesión mientras una grabación está arrancando.
#[tauri::command(async)]
fn get_status(state: State<AppState>) -> Status {
    let session = state.session.lock();
    let uploads = state.uploads.lock();
    Status {
        recording: session.is_some(),
        paused: session.as_ref().map(|s| s.is_paused()).unwrap_or(false),
        elapsed_ms: session
            .as_ref()
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0),
        screen_frames: session
            .as_ref()
            .map(|s| s.screen_frames.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0),
        mic_peak: state.mic.lock().as_ref().map(|m| m.take_peak()).unwrap_or(0.0),
        camera_active: state
            .cam_shared
            .active
            .load(std::sync::atomic::Ordering::Relaxed),
        cloud: uploads.first().map(|u| u.progress()),
        uploads_active: uploads
            .iter()
            .filter(|u| !matches!(u.progress().state.as_str(), "done" | "failed"))
            .count(),
    }
}

#[tauri::command(async)]
fn list_recordings(dir: String) -> Vec<Manifest> {
    recorder::list_recordings(&PathBuf::from(dir))
        .into_iter()
        .filter(|m| m.status != "recording")
        .take(30)
        .collect()
}

#[tauri::command(async)]
fn open_folder(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        std::fs::create_dir_all(&p).map_err(err)?;
    }
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(err)
}

/// Muestra u oculta la burbuja flotante de la cámara.
#[tauri::command]
fn set_bubble(app: tauri::AppHandle, visible: bool) -> CmdResult<()> {
    let w = app.get_webview_window("bubble").ok_or("sin burbuja")?;
    if visible { w.show() } else { w.hide() }.map_err(err)
}

/// Excluye las ventanas de Dualrec de la captura de pantalla
/// (en Windows usa `WDA_EXCLUDEFROMCAPTURE`).
#[tauri::command]
fn set_capture_hidden(app: tauri::AppHandle, hidden: bool) {
    // En desarrollo, DUALREC_NO_PROTECT deja las ventanas capturables.
    let hidden = hidden && std::env::var("DUALREC_NO_PROTECT").is_err();
    for label in ["main", "bubble"] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_content_protected(hidden);
        }
    }
}

/// Graba unos segundos sin interfaz, para verificar el motor de captura:
/// `dualrec --selftest <segundos> <carpeta>`.
fn selftest(args: &[String]) -> i32 {
    let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(6);
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| default_dir().to_string_lossy().into_owned());
    println!("ffmpeg: {:?} (ok={})", ffmpeg::locate(), ffmpeg::available());
    println!("encoder: {}", ffmpeg::detect_encoder().label());
    println!("pantallas: {:?}", screen::list_monitors());
    println!("camaras: {:?}", camera::list_cameras());
    println!("microfonos: {:?}", audio::list_inputs());

    let cam_shared = Arc::new(CamShared::default());
    let fake = args.iter().any(|a| a == "--fake-camera");
    let cam = if fake {
        CameraHandle::open_test_pattern(cam_shared.clone()).ok()
    } else {
        camera::list_cameras().first().and_then(|c| {
            match CameraHandle::open(c.index, cam_shared.clone()) {
            Ok(h) => {
                    println!("camara abierta: {} {}x{} @ {} fps", h.name, h.width, h.height, h.fps);
                    Some(h)
                }
                Err(e) => {
                    println!("camara no disponible: {e}");
                    None
                }
            }
        })
    };
    if cam.is_some() {
        // Se espera el primer cuadro antes de arrancar.
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    // DUALREC_TEST_MIC permite elegir el micrófono de la prueba por nombre.
    let mic = match AudioCapture::open(AudioSource::Input(std::env::var("DUALREC_TEST_MIC").ok())) {
        Ok(m) => Some(Arc::new(m)),
        Err(e) => {
            println!("microfono no disponible: {e}");
            None
        }
    };
    let sources = Sources {
        screen: Some(ScreenTarget::Monitor { index: 0 }),
        camera: cam.as_ref().map(|c| CameraSource {
            shared: cam_shared.clone(),
            name: c.name.clone(),
            fps: c.fps,
        }),
        mic,
        system_audio: true,
    };
    let opts = RecOptions {
        out_dir: out,
        fps: 30,
        quality: "media".into(),
        max_height: Some(1080),
        cursor: true,
        keep_parts: false,
    };
    let cloud = match (std::env::var("DUALREC_API_URL"), std::env::var("DUALREC_TOKEN")) {
        (Ok(api_url), Ok(token)) => Some(CloudConfig { api_url, token }),
        _ => None,
    };
    let opts = RecOptions {
        keep_parts: cloud.is_some(),
        ..opts
    };
    let session = match Session::start(sources, &opts) {
        Ok(s) => s,
        Err(e) => {
            println!("ERROR al iniciar: {e}");
            return 1;
        }
    };
    // Con una pausa en el medio, para probar que el reloj la descuenta.
    let upload = cloud.and_then(|cfg| {
        let initial = upload_state(&session.manifest, &cfg, Some("Autoprueba".into()));
        Upload::spawn(session.dir.clone(), cfg, initial).ok()
    });
    let third = std::time::Duration::from_millis(secs * 1000 / 3);
    std::thread::sleep(third);
    session.pause();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    session.resume();
    std::thread::sleep(third * 2);
    let manifest = session.stop();
    println!("{}", serde_json::to_string_pretty(&manifest).unwrap());
    let mut cloud_ok = true;
    if let Some(upload) = upload {
        upload.finish(manifest.duration_ms);
        // Se espera a que termine la subida (o a que falle de forma definitiva).
        let wait: u64 = std::env::var("DUALREC_UPLOAD_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(180);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
        let mut last = String::new();
        loop {
            let p = upload.progress();
            let line = format!("nube: {} {}/{} bytes {}", p.state, p.uploaded_bytes, p.total_bytes, p.error.clone().unwrap_or_default());
            if line != last {
                println!("{line}");
                last = line;
            }
            if p.state == "done" || p.state == "failed" || std::time::Instant::now() > deadline {
                cloud_ok = p.state == "done";
                println!("link: {}", p.share_url.unwrap_or_default());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    i32::from(manifest.status != "ready" || !cloud_ok)
}

/// Retoma por consola las subidas pendientes de una carpeta:
/// `dualrec --resume-uploads <carpeta>` (usa DUALREC_API_URL y DUALREC_TOKEN).
fn resume_cli(dir: Option<&String>) -> i32 {
    let (Ok(api_url), Ok(token)) = (std::env::var("DUALREC_API_URL"), std::env::var("DUALREC_TOKEN")) else {
        println!("Faltan DUALREC_API_URL y DUALREC_TOKEN.");
        return 2;
    };
    let cfg = CloudConfig { api_url, token };
    let dir = dir.map(PathBuf::from).unwrap_or_else(default_dir);
    let mut failed = 0;
    for (path, mut pending) in cloud::pending(&dir) {
        println!("retomando {} ({} partes ya subidas)", path.display(), pending.tracks.iter().map(|t| t.parts.len()).sum::<usize>());
        if pending.duration_ms.is_none() {
            pending.duration_ms = Some(0);
        }
        pending.failed = None;
        let Ok(upload) = Upload::spawn(path, cfg.clone(), pending) else { continue };
        loop {
            let p = upload.progress();
            if p.state == "done" || p.state == "failed" {
                println!("nube: {} {}/{} bytes {}", p.state, p.uploaded_bytes, p.total_bytes, p.error.unwrap_or_default());
                println!("link: {}", p.share_url.unwrap_or_default());
                failed += i32::from(p.state != "done");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    failed
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--caminfo") {
        camera::print_diagnostics();
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--resume-uploads") {
        std::process::exit(resume_cli(args.get(i + 1)));
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        std::process::exit(selftest(&args[i + 1..]));
    }

    let cam_shared = Arc::new(CamShared::default());
    let preview_port = preview::start(cam_shared.clone()).unwrap_or(0);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            cam_shared,
            camera: Mutex::new(None),
            mic: Mutex::new(None),
            session: Mutex::new(None),
            uploads: Mutex::new(Vec::new()),
            preview_port,
        })
        .setup(|app| {
            // Atajos globales: la interfaz decide qué hacer con cada uno.
            for (keys, action) in [("ctrl+alt+r", "toggle-record"), ("ctrl+alt+p", "toggle-pause")] {
                let handle = app.handle().clone();
                let _ = app.global_shortcut().on_shortcut(keys, move |_, _, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = handle.emit("shortcut", action);
                    }
                });
            }
            if std::env::var("DUALREC_NO_PROTECT").is_err() {
                for label in ["main", "bubble"] {
                    if let Some(w) = app.get_webview_window(label) {
                        let _ = w.set_content_protected(true);
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar la ventana principal corta la grabación en curso
            // (los archivos se finalizan) y cierra también la burbuja.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    let state = window.state::<AppState>();
                    let session = state.session.lock().take();
                    if let Some(s) = session {
                        // Lo que falte subir se retoma al volver a abrir la app.
                        finish_session(&state, s);
                    }
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_devices,
            get_preview_url,
            get_encoder,
            set_camera,
            set_mic,
            start_recording,
            pause_recording,
            resume_recording,
            stop_recording,
            get_status,
            list_recordings,
            open_folder,
            copy_text,
            cloud_check,
            cloud_defaults,
            resume_uploads,
            set_bubble,
            set_capture_hidden,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Dualrec");
}
