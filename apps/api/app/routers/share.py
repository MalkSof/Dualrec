"""Link para compartir: acceso público por token, sin iniciar sesión."""

from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy import select
from sqlalchemy.orm import Session

from ..db import get_db
from ..models import User, Video
from ..schemas import SharedVideo
from .videos import track_out

router = APIRouter(prefix="/share", tags=["share"])


@router.get("/{token}", response_model=SharedVideo)
def shared_video(token: str, db: Session = Depends(get_db)):
    video = db.scalar(select(Video).where(Video.share_token == token))
    # Un video privado responde igual que uno inexistente.
    if video is None or video.visibility != "link":
        raise HTTPException(404, "Este video no existe o ya no está compartido.")
    author = db.get(User, video.user_id)
    playable = video.status == "ready"
    return SharedVideo(
        title=video.title,
        status=video.status,
        duration_ms=video.duration_ms,
        recorded_at=video.recorded_at,
        author=author.name if author else "",
        tracks=[track_out(t, with_urls=playable) for t in video.tracks] if playable else [],
    )
