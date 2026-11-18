"""Entorno de pruebas: SQLite temporal y un S3 simulado (moto) escuchando en
localhost, para que las URLs prefirmadas se puedan usar con HTTP real."""

import os
import socket
import tempfile

import pytest


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


_tmp = tempfile.mkdtemp(prefix="dualrec-tests-")
_port = _free_port()
os.environ.update(
    DUALREC_DATABASE_URL=f"sqlite:///{_tmp}/test.db".replace("\\", "/"),
    DUALREC_S3_ENDPOINT_URL=f"http://127.0.0.1:{_port}",
    DUALREC_S3_BUCKET="dualrec-test",
    DUALREC_S3_ADDRESSING_STYLE="path",
    DUALREC_S3_ACCESS_KEY_ID="test",
    DUALREC_S3_SECRET_ACCESS_KEY="test",
    DUALREC_PUBLIC_WEB_URL="https://dualrec.test",
)


@pytest.fixture(scope="session", autouse=True)
def s3_server():
    from moto.server import ThreadedMotoServer

    server = ThreadedMotoServer(ip_address="127.0.0.1", port=_port, verbose=False)
    server.start()
    from app import storage

    storage.client().create_bucket(Bucket=storage.bucket())
    yield
    server.stop()


@pytest.fixture()
def client(s3_server):
    from fastapi.testclient import TestClient

    from app.db import Base, engine
    from app.main import app

    Base.metadata.drop_all(engine)
    Base.metadata.create_all(engine)
    with TestClient(app) as c:
        yield c


def make_user(name: str, email: str) -> dict[str, str]:
    """Crea un usuario con su espacio y devuelve el encabezado de autorización."""
    from app.auth import hash_token, new_token
    from app.db import SessionLocal
    from app.models import User, Workspace

    token = new_token()
    with SessionLocal() as db:
        ws = Workspace(name=f"Espacio de {name}")
        db.add(ws)
        db.flush()
        db.add(User(workspace_id=ws.id, name=name, email=email, token_hash=hash_token(token)))
        db.commit()
    return {"Authorization": f"Bearer {token}"}


@pytest.fixture()
def auth(client):
    return make_user("Ana", "ana@example.com")
