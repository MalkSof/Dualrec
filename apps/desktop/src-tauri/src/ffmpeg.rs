//! FFmpeg como sidecar: localización, detección de encoder por hardware y
//! armado de los argumentos de cada pista.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// Busca ffmpeg: primero el sidecar junto al ejecutable, después el binario
/// de desarrollo y por último el PATH.
pub fn locate() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            let p = dir.join(exe_name);
            if p.exists() {
                return p;
            }
        }
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        if let Ok(rd) = std::fs::read_dir(&dev) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("ffmpeg") {
                    return e.path();
                }
            }
        }
        PathBuf::from(exe_name)
    })
    .clone()
}

pub fn available() -> bool {
    let mut cmd = Command::new(locate());
    cmd.arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    hide_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    Qsv,
    Amf,
    X264,
}

impl Encoder {
    pub fn codec(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
            Encoder::X264 => "libx264",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Encoder::Nvenc => "NVIDIA NVENC (hardware)",
            Encoder::Qsv => "Intel Quick Sync (hardware)",
            Encoder::Amf => "AMD AMF (hardware)",
            Encoder::X264 => "x264 (CPU)",
        }
    }

    /// Opciones de calidad. `q` es el factor de calidad constante (menor = mejor).
    fn quality_args(self, q: u32) -> Vec<String> {
        let q = q.to_string();
        let v: Vec<&str> = match self {
            Encoder::Nvenc => vec!["-preset", "p4", "-rc", "vbr", "-cq", &q, "-b:v", "0"],
            Encoder::Qsv => vec!["-preset", "fast", "-global_quality", &q],
            Encoder::Amf => vec![
                "-quality", "balanced", "-rc", "cqp", "-qp_i", &q, "-qp_p", &q,
            ],
            Encoder::X264 => vec!["-preset", "veryfast", "-crf", &q],
        };
        v.into_iter().map(String::from).collect()
    }
}

fn encoder_works(enc: Encoder) -> bool {
    let mut cmd = Command::new(locate());
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=640x360:r=30:d=0.2",
        "-c:v",
        enc.codec(),
        "-f",
        "null",
        "-",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    hide_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// Elige el mejor encoder H.264 disponible haciendo una codificación de prueba
/// real con cada uno (que figure en `-encoders` no garantiza que haya GPU).
pub fn detect_encoder() -> Encoder {
    static ENC: OnceLock<Encoder> = OnceLock::new();
    *ENC.get_or_init(|| {
        if let Ok(forced) = std::env::var("DUALREC_ENCODER") {
            match forced.as_str() {
                "nvenc" => return Encoder::Nvenc,
                "qsv" => return Encoder::Qsv,
                "amf" => return Encoder::Amf,
                "x264" => return Encoder::X264,
                _ => {}
            }
        }
        [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf]
            .into_iter()
            .find(|e| encoder_works(*e))
            .unwrap_or(Encoder::X264)
    })
}

pub fn quality_factor(quality: &str) -> u32 {
    match quality {
        "alta" => 19,
        "baja" => 28,
        _ => 23,
    }
}

/// MP4 fragmentado: el archivo es reproducible aunque la app se cierre a mitad
/// de la grabación, porque cada fragmento se escribe completo en disco.
const FRAG_FLAGS: &str = "+frag_keyframe+empty_moov+default_base_moof";

pub struct VideoSpec<'a> {
    pub pix_fmt: &'a str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoder: Encoder,
    pub quality: u32,
    pub max_height: Option<u32>,
}

pub fn video_args(spec: &VideoSpec, out: &Path) -> Vec<String> {
    // H.264 4:2:0 necesita dimensiones pares.
    let mut vf = String::from("crop=trunc(iw/2)*2:trunc(ih/2)*2");
    if let Some(mh) = spec.max_height {
        if spec.height > mh {
            vf.push_str(&format!(",scale=-2:{mh}:flags=bicubic"));
        }
    }
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        spec.pix_fmt,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.extend([
        "-video_size".into(),
        format!("{}x{}", spec.width, spec.height),
        "-framerate".into(),
        spec.fps.to_string(),
        "-i".into(),
        "pipe:0".into(),
        "-vf".into(),
        vf,
        "-c:v".into(),
        spec.encoder.codec().into(),
    ]);
    a.extend(spec.encoder.quality_args(spec.quality));
    a.extend([
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-g".into(),
        (spec.fps * 2).to_string(),
        "-movflags".into(),
        FRAG_FLAGS.into(),
        "-f".into(),
        "mp4".into(),
        out.to_string_lossy().into_owned(),
    ]);
    a
}

pub fn audio_args(rate: u32, channels: u16, out: &Path) -> Vec<String> {
    [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "f32le",
        "-ar",
        &rate.to_string(),
        "-ac",
        &channels.to_string(),
        "-i",
        "pipe:0",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-movflags",
        "+empty_moov+default_base_moof",
        "-frag_duration",
        "2000000",
        "-f",
        "mp4",
        &out.to_string_lossy(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Lanza ffmpeg con stdin por pipe y stderr a un archivo de log.
pub fn spawn(args: &[String], log: &Path) -> Result<Child> {
    let log_file = std::fs::File::create(log).context("no se pudo crear el log de ffmpeg")?;
    let mut cmd = Command::new(locate());
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file));
    hide_window(&mut cmd);
    cmd.spawn().map_err(|e| {
        anyhow!("No se pudo iniciar FFmpeg ({e}). Verificá que ffmpeg esté instalado junto a Dualrec.")
    })
}

/// Pasa el MP4 fragmentado a un MP4 clásico con el índice al principio
/// (copia de streams, sin recodificar) para que cualquier editor lo abra.
pub fn finalize(part: &Path, out: &Path) -> Result<()> {
    let mut cmd = Command::new(locate());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(part)
        .args(["-c", "copy", "-movflags", "+faststart", "-f", "mp4"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hide_window(&mut cmd);
    let o = cmd.output()?;
    if !o.status.success() {
        return Err(anyhow!(
            "ffmpeg no pudo finalizar {}: {}",
            out.display(),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(h: u32, max: Option<u32>) -> VideoSpec<'static> {
        VideoSpec {
            pix_fmt: "bgra",
            width: 2560,
            height: h,
            fps: 30,
            encoder: Encoder::X264,
            quality: 23,
            max_height: max,
        }
    }

    #[test]
    fn video_args_describe_the_raw_input() {
        let a = video_args(&spec(1440, None), Path::new("out.mp4"));
        let joined = a.join(" ");
        assert!(joined.contains("-f rawvideo -pix_fmt bgra -video_size 2560x1440 -framerate 30"));
        assert!(joined.contains("-c:v libx264 -preset veryfast -crf 23"));
        assert!(joined.contains("-g 60"));
        assert!(joined.contains("frag_keyframe"));
        assert!(!joined.contains("scale="));
    }

    #[test]
    fn video_is_downscaled_only_above_the_cap() {
        let a = video_args(&spec(1440, Some(1080)), Path::new("o.mp4")).join(" ");
        assert!(a.contains("scale=-2:1080"));
        let b = video_args(&spec(1080, Some(1080)), Path::new("o.mp4")).join(" ");
        assert!(!b.contains("scale="));
    }

    #[test]
    fn audio_args_use_device_format() {
        let a = audio_args(44_100, 1, Path::new("mic.m4a")).join(" ");
        assert!(a.contains("-f f32le -ar 44100 -ac 1 -i pipe:0 -c:a aac"));
    }

    #[test]
    fn quality_names_map_to_factors() {
        assert!(quality_factor("alta") < quality_factor("media"));
        assert!(quality_factor("media") < quality_factor("baja"));
    }
}
