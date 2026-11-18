"""Procesamiento posterior a la subida.

La app sube cada pista como MP4 fragmentado (se puede ir subiendo mientras se
graba), pero ese formato no trae índice: el navegador no conoce la duración ni
puede saltar a un minuto cualquiera. Acá se reempaqueta sin recodificar a un
MP4 clásico con el índice al principio.

En la etapa 3 este paso pasa a ser la primera tarea de la cadena de Celery
(video compuesto, miniaturas, HLS, transcripción).
"""

import logging
import re
import subprocess
import tempfile
from pathlib import Path

from . import storage
from .config import get_settings
from .db import SessionLocal
from .models import Track, Video

log = logging.getLogger("dualrec.processing")

EXTENSION = {"screen": "mp4", "camera": "mp4", "composite": "mp4", "mic": "m4a", "system": "m4a"}
CONTENT_TYPE = {"mp4": "video/mp4", "m4a": "audio/mp4"}


def raw_key(workspace_id: str, video_id: str, kind: str) -> str:
    return f"workspaces/{workspace_id}/videos/{video_id}/raw/{kind}.{EXTENSION[kind]}"


def final_key(workspace_id: str, video_id: str, kind: str) -> str:
    return f"workspaces/{workspace_id}/videos/{video_id}/{kind}.{EXTENSION[kind]}"


def content_type(kind: str) -> str:
    return CONTENT_TYPE[EXTENSION[kind]]


def remux(src: Path, dst: Path) -> None:
    """Fragmentado -> MP4 con índice al principio, copiando los streams."""
    result = subprocess.run(
        [
            get_settings().ffmpeg_path, "-hide_banner", "-loglevel", "error", "-y",
            "-i", str(src), "-c", "copy", "-movflags", "+faststart", "-f", "mp4", str(dst),
        ],
        capture_output=True,
        text=True,
        timeout=3600,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip()[-500:] or "ffmpeg falló")


def probe_duration_ms(path: Path) -> int:
    """Duración de un archivo según ffmpeg (0 si no se puede leer)."""
    result = subprocess.run(
        [get_settings().ffmpeg_path, "-hide_banner", "-i", str(path)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    m = re.search(r"Duration: (\d+):(\d+):(\d+(?:\.\d+)?)", result.stderr)
    if not m:
        return 0
    hours, minutes, seconds = int(m[1]), int(m[2]), float(m[3])
    return int((hours * 3600 + minutes * 60 + seconds) * 1000)


def _finalize_track(video: Video, track: Track) -> None:
    target = final_key(video.workspace_id, video.id, track.kind)
    if track.storage_key == target:
        return  # ya estaba finalizada (reintento)
    with tempfile.TemporaryDirectory(prefix="dualrec-") as tmp:
        ext = EXTENSION[track.kind]
        src, dst = Path(tmp) / f"in.{ext}", Path(tmp) / f"out.{ext}"
        storage.download_file(track.storage_key, str(src))
        remux(src, dst)
        storage.upload_file(str(dst), target, content_type(track.kind))
        size = dst.stat().st_size
        if video.duration_ms == 0:
            # Grabación que quedó sin cerrar (la app se cerró a mitad): la duración
            # sale del propio archivo.
            video.duration_ms = probe_duration_ms(dst)
    old = track.storage_key
    track.storage_key = target
    track.size_bytes = size
    storage.delete(old)


def finalize_video(video_id: str) -> None:
    """Tarea en segundo plano: deja el video en `ready`.

    Si una pista no se puede reempaquetar se conserva el archivo fragmentado,
    que igual se reproduce; el aviso queda en `videos.error`.
    """
    with SessionLocal() as db:
        video = db.get(Video, video_id)
        if video is None or video.status != "processing":
            return
        problems = []
        for track in video.tracks:
            try:
                _finalize_track(video, track)
                db.commit()
            except Exception as e:  # una pista con problemas no frena a las demás
                db.rollback()
                log.exception("no se pudo finalizar la pista %s de %s", track.kind, video_id)
                problems.append(f"{track.kind}: {e}")
        video.status = "ready"
        video.error = "; ".join(problems) or None
        db.commit()
