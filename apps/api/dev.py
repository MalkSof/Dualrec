"""Entorno de desarrollo en un solo comando, sin Docker ni cuentas en la nube:

    uv run python dev.py

Levanta un S3 simulado (moto) en :9000, aplica las migraciones sobre SQLite,
crea un usuario de prueba y arranca la API en :8000.

El almacenamiento simulado vive en memoria: los videos subidos se pierden al
cerrar este proceso. Para algo persistente, configurar un S3 real en `.env`.
"""

import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent
S3_PORT = int(os.environ.get("DUALREC_DEV_S3_PORT", "9000"))
API_PORT = int(os.environ.get("DUALREC_DEV_API_PORT", "8000"))
TOKEN_FILE = HERE / ".dev-token"

os.environ.setdefault("DUALREC_DATABASE_URL", f"sqlite:///{(HERE / 'dualrec-dev.db').as_posix()}")
os.environ.setdefault("DUALREC_S3_ENDPOINT_URL", f"http://127.0.0.1:{S3_PORT}")
os.environ.setdefault("DUALREC_S3_BUCKET", "dualrec-dev")
os.environ.setdefault("DUALREC_S3_ADDRESSING_STYLE", "path")
os.environ.setdefault("DUALREC_S3_ACCESS_KEY_ID", "dev")
os.environ.setdefault("DUALREC_S3_SECRET_ACCESS_KEY", "dev")
os.environ.setdefault("DUALREC_PUBLIC_WEB_URL", "http://localhost:5173")


def main() -> int:
    from moto.server import ThreadedMotoServer

    s3 = ThreadedMotoServer(ip_address="127.0.0.1", port=S3_PORT, verbose=False)
    s3.start()
    print(f"S3 simulado en http://127.0.0.1:{S3_PORT} (en memoria)")

    subprocess.run([sys.executable, "-m", "alembic", "upgrade", "head"], cwd=HERE, check=True)

    from sqlalchemy import select

    from app import cli
    from app.auth import hash_token, new_token
    from app.db import SessionLocal
    from app.models import User, Workspace

    cli.create_bucket()

    # Los videos de corridas anteriores apuntan a archivos que ya no existen.
    from app.models import Video

    with SessionLocal() as db:
        for video in db.scalars(select(Video)):
            db.delete(video)
        user = db.scalar(select(User).where(User.email == "dev@dualrec.local"))
        if user is None or not TOKEN_FILE.exists():
            token = new_token()
            if user is None:
                ws = Workspace(name="Desarrollo")
                db.add(ws)
                db.flush()
                user = User(workspace_id=ws.id, name="Dev", email="dev@dualrec.local", token_hash=hash_token(token))
                db.add(user)
            else:
                user.token_hash = hash_token(token)
            TOKEN_FILE.write_text(token, encoding="utf-8")
        db.commit()

    print(f"API en http://127.0.0.1:{API_PORT}  ·  documentación en /docs")
    print(f"Token de desarrollo: {TOKEN_FILE.read_text(encoding='utf-8')}")

    import uvicorn

    uvicorn.run("app.main:app", host="127.0.0.1", port=API_PORT, log_level="warning")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
