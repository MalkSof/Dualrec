"""Videos del usuario: alta, subida multipart reanudable y cierre."""

import secrets

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException, Response
from sqlalchemy import select
from sqlalchemy.orm import Session

from .. import storage
from ..auth import current_user
from ..config import get_settings
from ..db import get_db
from ..models import Track, UploadPart, User, Video
from ..processing import content_type, finalize_video, raw_key
from ..schemas import (
    CompleteIn,
    PartIn,
    PartOut,
    PartUrl,
    TrackOut,
    VideoCreate,
    VideoOut,
    VideoPatch,
)

router = APIRouter(prefix="/videos", tags=["videos"])

MAX_PARTS = 10_000  # límite de S3 para una subida multipart


def share_url(video: Video) -> str:
    return f"{get_settings().public_web_url.rstrip('/')}/v/{video.share_token}"


def track_out(track: Track, *, with_urls: bool) -> TrackOut:
    uploaded = track.upload_id is None
    filename = track.storage_key.rsplit("/", 1)[-1]
    return TrackOut(
        id=track.id,
        kind=track.kind,
        codec=track.codec,
        width=track.width,
        height=track.height,
        offset_ms=track.offset_ms,
        size_bytes=track.size_bytes,
        uploaded=uploaded,
        url=storage.download_url(track.storage_key) if with_urls and uploaded else None,
        download_url=(
            storage.download_url(track.storage_key, filename) if with_urls and uploaded else None
        ),
    )


def video_out(video: Video) -> VideoOut:
    playable = video.status == "ready"
    return VideoOut(
        id=video.id,
        title=video.title,
        status=video.status,
        error=video.error,
        duration_ms=video.duration_ms,
        visibility=video.visibility,
        recorded_at=video.recorded_at,
        share_token=video.share_token,
        share_url=share_url(video),
        part_size=get_settings().part_size,
        tracks=[track_out(t, with_urls=playable) for t in sorted(video.tracks, key=lambda t: t.kind)],
    )


def own_video(video_id: str, user: User, db: Session) -> Video:
    video = db.get(Video, video_id)
    # 404 también si es de otro workspace: no se revela que el id existe.
    if video is None or video.workspace_id != user.workspace_id:
        raise HTTPException(404, "Video no encontrado.")
    return video


def own_track(video: Video, track_id: str) -> Track:
    for track in video.tracks:
        if track.id == track_id:
            return track
    raise HTTPException(404, "Pista no encontrada.")


@router.post("", response_model=VideoOut, status_code=201)
def create_video(body: VideoCreate, response: Response, user: User = Depends(current_user), db: Session = Depends(get_db)):
    kinds = [t.kind for t in body.tracks]
    if len(set(kinds)) != len(kinds):
        raise HTTPException(422, "Cada tipo de pista puede aparecer una sola vez.")

    if body.client_id:
        existing = db.scalar(
            select(Video).where(
                Video.workspace_id == user.workspace_id, Video.client_id == body.client_id
            )
        )
        if existing is not None:
            response.status_code = 200
            return video_out(existing)

    video = Video(
        workspace_id=user.workspace_id,
        user_id=user.id,
        title=body.title or "Grabación sin título",
        status="recording",
        share_token=secrets.token_urlsafe(16),
        client_id=body.client_id,
    )
    if body.recorded_at is not None:
        video.recorded_at = body.recorded_at
    db.add(video)
    db.flush()  # asigna el id, que forma parte de la clave de almacenamiento

    for t in body.tracks:
        key = raw_key(user.workspace_id, video.id, t.kind)
        video.tracks.append(
            Track(
                kind=t.kind,
                codec=t.codec,
                width=t.width,
                height=t.height,
                offset_ms=t.offset_ms,
                storage_key=key,
                upload_id=storage.start_upload(key, content_type(t.kind)),
            )
        )
    db.commit()
    return video_out(video)


@router.get("", response_model=list[VideoOut])
def list_videos(user: User = Depends(current_user), db: Session = Depends(get_db)):
    videos = db.scalars(
        select(Video)
        .where(Video.workspace_id == user.workspace_id)
        .order_by(Video.recorded_at.desc())
        .limit(200)
    )
    return [video_out(v) for v in videos]


@router.get("/{video_id}", response_model=VideoOut)
def get_video(video_id: str, user: User = Depends(current_user), db: Session = Depends(get_db)):
    return video_out(own_video(video_id, user, db))


@router.patch("/{video_id}", response_model=VideoOut)
def update_video(video_id: str, body: VideoPatch, user: User = Depends(current_user), db: Session = Depends(get_db)):
    video = own_video(video_id, user, db)
    if body.title is not None:
        video.title = body.title
    if body.visibility is not None:
        video.visibility = body.visibility
    db.commit()
    return video_out(video)


@router.delete("/{video_id}", status_code=204)
def delete_video(video_id: str, user: User = Depends(current_user), db: Session = Depends(get_db)):
    video = own_video(video_id, user, db)
    for track in video.tracks:
        if track.upload_id:
            storage.abort_upload(track.storage_key, track.upload_id)
        else:
            storage.delete(track.storage_key)
    db.delete(video)
    db.commit()


@router.post("/{video_id}/tracks/{track_id}/parts/{part_number}/url", response_model=PartUrl)
def presign_part(video_id: str, track_id: str, part_number: int, user: User = Depends(current_user), db: Session = Depends(get_db)):
    """URL prefirmada para subir una parte directo al almacenamiento."""
    track = own_track(own_video(video_id, user, db), track_id)
    if not 1 <= part_number <= MAX_PARTS:
        raise HTTPException(422, f"El número de parte va de 1 a {MAX_PARTS}.")
    if track.upload_id is None:
        raise HTTPException(409, "La subida de esta pista ya está cerrada.")
    return PartUrl(
        part_number=part_number,
        url=storage.part_url(track.storage_key, track.upload_id, part_number),
    )


@router.put("/{video_id}/tracks/{track_id}/parts/{part_number}", response_model=PartOut)
def register_part(video_id: str, track_id: str, part_number: int, body: PartIn, user: User = Depends(current_user), db: Session = Depends(get_db)):
    """Registra una parte ya subida. Es idempotente: reintentar no duplica."""
    video = own_video(video_id, user, db)
    track = own_track(video, track_id)
    if track.upload_id is None:
        raise HTTPException(409, "La subida de esta pista ya está cerrada.")
    part = next((p for p in track.parts if p.part_number == part_number), None)
    if part is None:
        part = UploadPart(part_number=part_number, etag=body.etag, size_bytes=body.size_bytes)
        track.parts.append(part)
    else:
        part.etag, part.size_bytes = body.etag, body.size_bytes
    if video.status == "recording":
        video.status = "uploading"
    db.commit()
    return PartOut(part_number=part.part_number, etag=part.etag, size_bytes=part.size_bytes)


@router.get("/{video_id}/tracks/{track_id}/parts", response_model=list[PartOut])
def list_parts(video_id: str, track_id: str, user: User = Depends(current_user), db: Session = Depends(get_db)):
    """Partes ya registradas: la app las usa para retomar una subida."""
    track = own_track(own_video(video_id, user, db), track_id)
    return [
        PartOut(part_number=p.part_number, etag=p.etag, size_bytes=p.size_bytes)
        for p in track.parts
    ]


@router.post("/{video_id}/complete", response_model=VideoOut)
def complete_video(video_id: str, body: CompleteIn, background: BackgroundTasks, user: User = Depends(current_user), db: Session = Depends(get_db)):
    """Cierra las subidas multipart y manda el video a procesar."""
    video = own_video(video_id, user, db)
    if video.status in ("processing", "ready"):
        return video_out(video)  # reintento de un cierre que ya se hizo

    video.duration_ms = body.duration_ms
    for track in list(video.tracks):
        if track.upload_id is None:
            continue
        if not track.parts:
            # Pista que no llegó a tener datos: se descarta.
            storage.abort_upload(track.storage_key, track.upload_id)
            video.tracks.remove(track)
            continue
        try:
            storage.finish_upload(
                track.storage_key, track.upload_id, [(p.part_number, p.etag) for p in track.parts]
            )
        except Exception as e:
            db.rollback()
            raise HTTPException(409, f"No se pudo cerrar la pista {track.kind}: {e}") from e
        track.size_bytes = sum(p.size_bytes for p in track.parts)
        track.upload_id = None
        db.commit()  # cada pista cerrada queda guardada aunque falle la siguiente

    if not video.tracks:
        video.status, video.error = "failed", "La grabación no tiene ninguna pista con datos."
        db.commit()
        return video_out(video)

    video.status = "processing"
    db.commit()
    background.add_task(finalize_video, video.id)
    return video_out(video)
