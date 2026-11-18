//! Subida a la nube mientras se graba.
//!
//! Cada pista se escribe en disco como MP4 fragmentado, que solo crece por el
//! final, así que se puede ir subiendo en partes de 8 MB (multipart de S3 con
//! URLs prefirmadas) sin esperar a que termine la grabación.
//!
//! La cola vive en disco: `upload.json`, dentro de la carpeta de la grabación,
//! guarda qué partes ya subieron. Si se corta internet se reintenta con espera
//! creciente, y si la app se cierra, al reabrirla se retoma desde ese archivo.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const STATE_FILE: &str = "upload.json";
const DEFAULT_PART_SIZE: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct CloudConfig {
    pub api_url: String,
    pub token: String,
}

impl CloudConfig {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_url.trim_end_matches('/'), path)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartState {
    pub number: u32,
    pub etag: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackState {
    pub kind: String,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub offset_ms: i64,
    /// Archivo fragmentado que se sube (p. ej. `screen.part.mp4`).
    pub file: String,
    pub track_id: Option<String>,
    #[serde(default)]
    pub parts: Vec<PartState>,
}

impl TrackState {
    fn uploaded(&self) -> u64 {
        self.parts.iter().map(|p| p.size).sum()
    }
}

/// Estado persistente de la subida de una grabación.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadState {
    pub api_url: String,
    /// Id de la grabación: la API lo usa para no duplicar el video al reintentar.
    pub client_id: String,
    pub title: String,
    pub recorded_at: String,
    pub video_id: Option<String>,
    pub share_url: Option<String>,
    #[serde(default)]
    pub part_size: u64,
    /// `Some` cuando la grabación terminó: ya no van a llegar más datos.
    pub duration_ms: Option<u64>,
    pub tracks: Vec<TrackState>,
    #[serde(default)]
    pub completed: bool,
    /// Error definitivo (por ejemplo, token inválido): no se reintenta solo.
    #[serde(default)]
    pub failed: Option<String>,
}

impl UploadState {
    pub fn load(dir: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(dir.join(STATE_FILE)).ok()?).ok()
    }

    fn save(&self, dir: &Path) -> Result<()> {
        let tmp = dir.join("upload.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(STATE_FILE))?;
        Ok(())
    }
}

/// Lo que ve la interfaz.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CloudProgress {
    pub dir: String,
    /// connecting | uploading | offline | finishing | done | failed
    pub state: String,
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub share_url: Option<String>,
    pub error: Option<String>,
}

pub struct Upload {
    pub dir: PathBuf,
    state: Arc<Mutex<UploadState>>,
    progress: Arc<Mutex<CloudProgress>>,
}

/// Cuántos bytes subir ahora: mientras se graba solo partes completas (S3 exige
/// 5 MB como mínimo salvo en la última); al terminar, también el resto.
pub fn next_chunk(file_len: u64, uploaded: u64, part_size: u64, finished: bool) -> u64 {
    let pending = file_len.saturating_sub(uploaded);
    if pending >= part_size {
        part_size
    } else if finished {
        pending
    } else {
        0
    }
}

/// Libera el fragmentado de una pista ya subida. Normalmente el MP4 final ya
/// está al lado y el fragmentado sobra; pero si la app se cerró a mitad de la
/// grabación, el fragmentado es la única copia local: se conserva con el
/// nombre final (`screen.part.mp4` -> `screen.mp4`).
pub fn release_part(dir: &Path, part_file: &str) {
    let part = dir.join(part_file);
    let final_path = dir.join(part_file.replacen(".part.", ".", 1));
    if final_path.exists() {
        let _ = std::fs::remove_file(part);
    } else {
        let _ = std::fs::rename(part, final_path);
    }
}

/// Espera antes del próximo reintento: 1 s, 2 s, 4 s… hasta 60 s.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt.min(6)).min(Duration::from_secs(60))
}

enum Failure {
    /// Sin conexión, tiempo agotado o error del servidor: se reintenta.
    Retry(String),
    /// El servidor rechazó el pedido: reintentar igual no lo va a arreglar.
    Fatal(String),
}

fn classify(e: ureq::Error) -> Failure {
    match e {
        ureq::Error::Status(code, response) => {
            let detail = response
                .into_json::<Value>()
                .ok()
                .and_then(|v| v.get("detail").map(|d| d.to_string()))
                .unwrap_or_default();
            let msg = format!("la API respondió {code} {detail}");
            if code == 408 || code == 429 || code >= 500 {
                Failure::Retry(msg)
            } else if code == 401 {
                Failure::Fatal("El token de acceso no es válido. Revisalo en Ajustes → Nube.".into())
            } else {
                Failure::Fatal(msg)
            }
        }
        ureq::Error::Transport(t) => Failure::Retry(format!("sin conexión ({t})")),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(120))
        .build()
}

/// Verifica la URL y el token contra la API. Devuelve el nombre del usuario.
pub fn check(cfg: &CloudConfig) -> Result<String> {
    let response = agent()
        .get(&cfg.url("/me"))
        .set("Authorization", &format!("Bearer {}", cfg.token))
        .call()
        .map_err(|e| match classify(e) {
            Failure::Retry(m) | Failure::Fatal(m) => anyhow!("{m}"),
        })?;
    let me: Value = response.into_json().context("respuesta inesperada de la API")?;
    Ok(me["name"].as_str().unwrap_or("").to_string())
}

struct Worker {
    dir: PathBuf,
    cfg: CloudConfig,
    agent: ureq::Agent,
    state: Arc<Mutex<UploadState>>,
    progress: Arc<Mutex<CloudProgress>>,
}

impl Worker {
    fn auth(&self, req: ureq::Request) -> ureq::Request {
        req.set("Authorization", &format!("Bearer {}", self.cfg.token))
    }

    fn save(&self) {
        let snapshot = self.state.lock().clone();
        let _ = snapshot.save(&self.dir);
    }

    fn set_state(&self, state: &str, error: Option<String>) {
        let mut p = self.progress.lock();
        p.state = state.into();
        p.error = error;
    }

    fn refresh_totals(&self) {
        let st = self.state.lock();
        let (mut up, mut total) = (0u64, 0u64);
        for t in &st.tracks {
            let len = std::fs::metadata(self.dir.join(&t.file)).map(|m| m.len()).unwrap_or(0);
            up += t.uploaded();
            total += len.max(t.uploaded());
        }
        let mut p = self.progress.lock();
        p.uploaded_bytes = up;
        p.total_bytes = total;
        p.share_url = st.share_url.clone();
    }

    /// Da de alta el video y guarda los ids de cada pista.
    fn create_video(&self) -> Result<(), Failure> {
        let body = {
            let st = self.state.lock();
            json!({
                "title": st.title,
                "recorded_at": st.recorded_at,
                "client_id": st.client_id,
                "tracks": st.tracks.iter().map(|t| json!({
                    "kind": t.kind, "codec": t.codec, "width": t.width,
                    "height": t.height, "offset_ms": t.offset_ms,
                })).collect::<Vec<_>>(),
            })
        };
        let video: Value = self
            .auth(self.agent.post(&self.cfg.url("/videos")))
            .send_json(body)
            .map_err(classify)?
            .into_json()
            .map_err(|e| Failure::Retry(e.to_string()))?;
        let mut st = self.state.lock();
        st.video_id = video["id"].as_str().map(String::from);
        st.share_url = video["share_url"].as_str().map(String::from);
        st.part_size = video["part_size"].as_u64().unwrap_or(DEFAULT_PART_SIZE);
        for t in st.tracks.iter_mut() {
            t.track_id = video["tracks"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["kind"] == t.kind.as_str()))
                .and_then(|x| x["id"].as_str())
                .map(String::from);
        }
        if st.video_id.is_none() {
            return Err(Failure::Fatal("la API no devolvió el id del video".into()));
        }
        Ok(())
    }

    /// Sube la próxima parte pendiente de alguna pista. `Ok(false)` = nada que subir por ahora.
    fn upload_next_part(&self) -> Result<bool, Failure> {
        let (video_id, part_size, finished, tracks) = {
            let st = self.state.lock();
            (
                st.video_id.clone().unwrap_or_default(),
                if st.part_size > 0 { st.part_size } else { DEFAULT_PART_SIZE },
                st.duration_ms.is_some(),
                st.tracks.clone(),
            )
        };
        for (i, t) in tracks.iter().enumerate() {
            let Some(track_id) = &t.track_id else { continue };
            let path = self.dir.join(&t.file);
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let uploaded = t.uploaded();
            let size = next_chunk(len, uploaded, part_size, finished);
            if size == 0 {
                continue;
            }
            let number = t.parts.len() as u32 + 1;
            let mut buf = vec![0u8; size as usize];
            let read = std::fs::File::open(&path).and_then(|mut f| {
                f.seek(SeekFrom::Start(uploaded))?;
                f.read_exact(&mut buf)
            });
            if let Err(e) = read {
                return Err(Failure::Retry(format!("no se pudo leer {}: {e}", t.file)));
            }

            let base = format!("/videos/{video_id}/tracks/{track_id}/parts/{number}");
            let presigned: Value = self
                .auth(self.agent.post(&self.cfg.url(&format!("{base}/url"))))
                .call()
                .map_err(classify)?
                .into_json()
                .map_err(|e| Failure::Retry(e.to_string()))?;
            let url = presigned["url"]
                .as_str()
                .ok_or_else(|| Failure::Fatal("la API no devolvió la URL de subida".into()))?;

            // Directo al almacenamiento: los bytes no pasan por la API.
            let put = self.agent.put(url).send_bytes(&buf).map_err(|e| match e {
                ureq::Error::Status(code, _) => Failure::Retry(format!("el almacenamiento respondió {code}")),
                ureq::Error::Transport(t) => Failure::Retry(format!("sin conexión ({t})")),
            })?;
            let etag = put
                .header("ETag")
                .ok_or_else(|| Failure::Retry("el almacenamiento no devolvió ETag".into()))?
                .to_string();

            self.auth(self.agent.put(&self.cfg.url(&base)))
                .send_json(json!({ "etag": etag, "size_bytes": size }))
                .map_err(classify)?;

            self.state.lock().tracks[i].parts.push(PartState { number, etag, size });
            self.save();
            return Ok(true);
        }
        Ok(false)
    }

    fn all_uploaded(&self) -> bool {
        let st = self.state.lock();
        st.duration_ms.is_some()
            && st.tracks.iter().all(|t| {
                let len = std::fs::metadata(self.dir.join(&t.file)).map(|m| m.len()).unwrap_or(0);
                t.uploaded() >= len
            })
    }

    fn complete(&self) -> Result<(), Failure> {
        let (video_id, duration) = {
            let st = self.state.lock();
            (st.video_id.clone().unwrap_or_default(), st.duration_ms.unwrap_or(0))
        };
        self.auth(self.agent.post(&self.cfg.url(&format!("/videos/{video_id}/complete"))))
            .send_json(json!({ "duration_ms": duration }))
            .map_err(classify)?;
        let files: Vec<String> = {
            let mut st = self.state.lock();
            st.completed = true;
            st.tracks.iter().map(|t| t.file.clone()).collect()
        };
        self.save();
        for f in files {
            release_part(&self.dir, &f);
        }
        Ok(())
    }

    fn run(self) {
        let mut attempt = 0u32;
        loop {
            let step: Result<bool, Failure> = (|| {
                if self.state.lock().video_id.is_none() {
                    self.set_state("connecting", None);
                    self.create_video()?;
                    self.save();
                    return Ok(true);
                }
                if self.upload_next_part()? {
                    return Ok(true);
                }
                if self.all_uploaded() {
                    self.set_state("finishing", None);
                    self.complete()?;
                }
                Ok(false)
            })();
            self.refresh_totals();

            match step {
                Ok(progressed) => {
                    attempt = 0;
                    if self.state.lock().completed {
                        self.set_state("done", None);
                        return;
                    }
                    self.set_state("uploading", None);
                    if !progressed {
                        std::thread::sleep(Duration::from_millis(700));
                    }
                }
                Err(Failure::Retry(msg)) => {
                    self.set_state("offline", Some(msg));
                    std::thread::sleep(backoff(attempt));
                    attempt += 1;
                }
                Err(Failure::Fatal(msg)) => {
                    self.state.lock().failed = Some(msg.clone());
                    self.save();
                    self.set_state("failed", Some(msg));
                    return;
                }
            }
        }
    }
}

impl Upload {
    /// Arranca (o retoma) la subida de la grabación guardada en `dir`.
    pub fn spawn(dir: PathBuf, cfg: CloudConfig, state: UploadState) -> Result<Arc<Upload>> {
        state.save(&dir)?;
        let progress = Arc::new(Mutex::new(CloudProgress {
            dir: dir.to_string_lossy().into_owned(),
            state: "connecting".into(),
            share_url: state.share_url.clone(),
            ..Default::default()
        }));
        let state = Arc::new(Mutex::new(state));
        let worker = Worker {
            dir: dir.clone(),
            cfg,
            agent: agent(),
            state: state.clone(),
            progress: progress.clone(),
        };
        std::thread::Builder::new()
            .name("dualrec-upload".into())
            .spawn(move || worker.run())?;
        Ok(Arc::new(Upload { dir, state, progress }))
    }

    /// La grabación terminó: se sube lo que falte y se cierra el video.
    pub fn finish(&self, duration_ms: u64) {
        let snapshot = {
            let mut st = self.state.lock();
            st.duration_ms = Some(duration_ms);
            st.clone()
        };
        let _ = snapshot.save(&self.dir);
    }

    pub fn progress(&self) -> CloudProgress {
        self.progress.lock().clone()
    }

    pub fn share_url(&self) -> Option<String> {
        self.state.lock().share_url.clone()
    }
}

/// Subidas sin terminar dentro de `out_dir` (una carpeta por grabación).
pub fn pending(out_dir: &Path) -> Vec<(PathBuf, UploadState)> {
    std::fs::read_dir(out_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let state = UploadState::load(&e.path())?;
            (!state.completed).then(|| (e.path(), state))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn only_full_parts_while_recording() {
        // Mientras se graba no se suben partes de menos de 8 MB.
        assert_eq!(next_chunk(3 * MB, 0, 8 * MB, false), 0);
        assert_eq!(next_chunk(8 * MB, 0, 8 * MB, false), 8 * MB);
        assert_eq!(next_chunk(20 * MB, 8 * MB, 8 * MB, false), 8 * MB);
        assert_eq!(next_chunk(20 * MB, 16 * MB, 8 * MB, false), 0);
    }

    #[test]
    fn the_tail_goes_up_when_recording_ends() {
        assert_eq!(next_chunk(20 * MB, 16 * MB, 8 * MB, true), 4 * MB);
        assert_eq!(next_chunk(3 * MB, 0, 8 * MB, true), 3 * MB);
        assert_eq!(next_chunk(16 * MB, 16 * MB, 8 * MB, true), 0);
    }

    #[test]
    fn the_only_local_copy_is_never_deleted() {
        let dir = std::env::temp_dir().join(format!("dualrec-release-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Grabación normal: existe el final, el fragmentado se borra.
        std::fs::write(dir.join("screen.part.mp4"), b"frag").unwrap();
        std::fs::write(dir.join("screen.mp4"), b"final").unwrap();
        release_part(&dir, "screen.part.mp4");
        assert!(!dir.join("screen.part.mp4").exists());
        assert_eq!(std::fs::read(dir.join("screen.mp4")).unwrap(), b"final");
        // App cerrada a mitad: solo hay fragmentado, pasa a ser el archivo final.
        std::fs::write(dir.join("mic.part.m4a"), b"frag").unwrap();
        release_part(&dir, "mic.part.m4a");
        assert_eq!(std::fs::read(dir.join("mic.m4a")).unwrap(), b"frag");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(6), Duration::from_secs(60));
        assert_eq!(backoff(40), Duration::from_secs(60));
    }

    #[test]
    fn state_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("dualrec-upload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = UploadState {
            api_url: "http://localhost:8000".into(),
            client_id: "2026-11-18_18-00-00".into(),
            title: "Demo".into(),
            recorded_at: "2026-11-18T18:00:00-03:00".into(),
            video_id: Some("01ABC".into()),
            share_url: Some("http://localhost/v/x".into()),
            part_size: 8 * MB,
            duration_ms: None,
            tracks: vec![TrackState {
                kind: "screen".into(),
                codec: "h264".into(),
                width: Some(1920),
                height: Some(1080),
                offset_ms: 0,
                file: "screen.part.mp4".into(),
                track_id: Some("01T".into()),
                parts: vec![PartState { number: 1, etag: "\"abc\"".into(), size: 8 * MB }],
            }],
            completed: false,
            failed: None,
        };
        state.save(&dir).unwrap();
        let parent = dir.parent().unwrap();
        let found = pending(parent)
            .into_iter()
            .find(|(p, _)| p == &dir)
            .expect("la subida pendiente se encuentra al reabrir");
        assert_eq!(found.1.tracks[0].uploaded(), 8 * MB);
        assert_eq!(found.1.video_id.as_deref(), Some("01ABC"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
