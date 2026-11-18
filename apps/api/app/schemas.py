from datetime import datetime
from typing import Literal

from pydantic import BaseModel, Field

TrackKind = Literal["screen", "camera", "mic", "system"]


class TrackIn(BaseModel):
    kind: TrackKind
    codec: str = Field(max_length=40)
    width: int | None = None
    height: int | None = None
    offset_ms: int = 0


class VideoCreate(BaseModel):
    title: str | None = Field(default=None, max_length=300)
    recorded_at: datetime | None = None
    # Identificador de la grabación en la app de escritorio: si la respuesta se
    # pierde y la app reintenta, se devuelve el mismo video en vez de duplicarlo.
    client_id: str | None = Field(default=None, max_length=100)
    tracks: list[TrackIn] = Field(min_length=1)


class TrackOut(BaseModel):
    id: str
    kind: str
    codec: str
    width: int | None
    height: int | None
    offset_ms: int
    size_bytes: int
    uploaded: bool
    url: str | None = None
    download_url: str | None = None


class VideoOut(BaseModel):
    id: str
    title: str
    status: str
    error: str | None
    duration_ms: int
    visibility: str
    recorded_at: datetime
    share_token: str
    share_url: str
    part_size: int
    tracks: list[TrackOut]


class PartUrl(BaseModel):
    part_number: int
    url: str


class PartIn(BaseModel):
    etag: str = Field(min_length=1, max_length=100)
    size_bytes: int = Field(gt=0)


class PartOut(BaseModel):
    part_number: int
    etag: str
    size_bytes: int


class CompleteIn(BaseModel):
    duration_ms: int = Field(ge=0)


class VideoPatch(BaseModel):
    title: str | None = Field(default=None, min_length=1, max_length=300)
    visibility: Literal["link", "private"] | None = None


class SharedVideo(BaseModel):
    """Lo que ve cualquiera con el link: sin ids internos ni datos del dueño."""

    title: str
    status: str
    duration_ms: int
    recorded_at: datetime
    author: str
    tracks: list[TrackOut]
