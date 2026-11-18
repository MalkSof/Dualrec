"""Modelo de datos. La etapa 2 usa workspaces, users, videos, tracks y
upload_parts; chapters, segments y chat_messages llegan con sus etapas."""

from datetime import UTC, datetime

from sqlalchemy import BigInteger, DateTime, ForeignKey, Integer, String, Text, UniqueConstraint
from sqlalchemy.orm import Mapped, mapped_column, relationship
from ulid import ULID

from .db import Base


def new_id() -> str:
    return str(ULID())


def now() -> datetime:
    return datetime.now(UTC)


class Workspace(Base):
    __tablename__ = "workspaces"

    id: Mapped[str] = mapped_column(String(26), primary_key=True, default=new_id)
    name: Mapped[str] = mapped_column(String(200))
    owner_id: Mapped[str | None] = mapped_column(String(26), nullable=True)
    plan: Mapped[str] = mapped_column(String(40), default="free")


class User(Base):
    __tablename__ = "users"

    id: Mapped[str] = mapped_column(String(26), primary_key=True, default=new_id)
    workspace_id: Mapped[str] = mapped_column(ForeignKey("workspaces.id"), index=True)
    name: Mapped[str] = mapped_column(String(200))
    email: Mapped[str] = mapped_column(String(320), unique=True)
    role: Mapped[str] = mapped_column(String(40), default="owner")
    # SHA-256 del token de acceso; el token en claro solo se muestra al crearlo.
    token_hash: Mapped[str] = mapped_column(String(64), unique=True)


class Video(Base):
    __tablename__ = "videos"

    id: Mapped[str] = mapped_column(String(26), primary_key=True, default=new_id)
    workspace_id: Mapped[str] = mapped_column(ForeignKey("workspaces.id"), index=True)
    user_id: Mapped[str] = mapped_column(ForeignKey("users.id"))
    title: Mapped[str] = mapped_column(String(300))
    # recording -> uploading -> processing -> ready | failed
    status: Mapped[str] = mapped_column(String(20), default="recording")
    error: Mapped[str | None] = mapped_column(Text, nullable=True)
    duration_ms: Mapped[int] = mapped_column(BigInteger, default=0)
    share_token: Mapped[str] = mapped_column(String(64), unique=True)
    # link: cualquiera con el link puede verlo · private: solo el workspace
    visibility: Mapped[str] = mapped_column(String(20), default="link")
    summary: Mapped[str | None] = mapped_column(Text, nullable=True)
    # Id de la grabación en la app de escritorio: evita duplicar al reintentar el alta.
    client_id: Mapped[str | None] = mapped_column(String(100), nullable=True, index=True)
    recorded_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)

    tracks: Mapped[list["Track"]] = relationship(
        back_populates="video", cascade="all, delete-orphan", order_by="Track.kind"
    )


class Track(Base):
    __tablename__ = "tracks"
    __table_args__ = (UniqueConstraint("video_id", "kind"),)

    id: Mapped[str] = mapped_column(String(26), primary_key=True, default=new_id)
    video_id: Mapped[str] = mapped_column(ForeignKey("videos.id", ondelete="CASCADE"), index=True)
    # screen | camera | mic | system | composite
    kind: Mapped[str] = mapped_column(String(20))
    storage_key: Mapped[str] = mapped_column(String(500))
    codec: Mapped[str] = mapped_column(String(40))
    width: Mapped[int | None] = mapped_column(Integer, nullable=True)
    height: Mapped[int | None] = mapped_column(Integer, nullable=True)
    offset_ms: Mapped[int] = mapped_column(Integer, default=0)
    size_bytes: Mapped[int] = mapped_column(BigInteger, default=0)
    # Subida multipart en curso; queda en None cuando se completa.
    upload_id: Mapped[str | None] = mapped_column(String(300), nullable=True)

    video: Mapped[Video] = relationship(back_populates="tracks")
    parts: Mapped[list["UploadPart"]] = relationship(
        back_populates="track", cascade="all, delete-orphan", order_by="UploadPart.part_number"
    )


class UploadPart(Base):
    __tablename__ = "upload_parts"
    __table_args__ = (UniqueConstraint("track_id", "part_number"),)

    id: Mapped[str] = mapped_column(String(26), primary_key=True, default=new_id)
    track_id: Mapped[str] = mapped_column(ForeignKey("tracks.id", ondelete="CASCADE"), index=True)
    part_number: Mapped[int] = mapped_column(Integer)
    etag: Mapped[str] = mapped_column(String(100))
    size_bytes: Mapped[int] = mapped_column(BigInteger)
    uploaded_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)

    track: Mapped[Track] = relationship(back_populates="parts")
