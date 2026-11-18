from collections.abc import Iterator

from sqlalchemy import URL, create_engine
from sqlalchemy.orm import DeclarativeBase, Session, sessionmaker

from .config import get_settings


class Base(DeclarativeBase):
    pass


def make_engine(url: str | URL):
    # SQLite se usa desde varios hilos (peticiones y tareas en segundo plano).
    sqlite = str(url).startswith("sqlite")
    return create_engine(
        url, connect_args={"check_same_thread": False} if sqlite else {}, pool_pre_ping=True
    )


engine = make_engine(get_settings().sqlalchemy_url)
SessionLocal = sessionmaker(bind=engine, expire_on_commit=False)


def get_db() -> Iterator[Session]:
    with SessionLocal() as session:
        yield session
