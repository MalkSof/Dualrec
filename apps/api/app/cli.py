"""Tareas de administración.

    uv run python -m app.cli create-user "Nombre" correo@ejemplo.com
    uv run python -m app.cli rotate-token correo@ejemplo.com
    uv run python -m app.cli create-bucket
    uv run python -m app.cli bootstrap
"""

import argparse
import sys

from sqlalchemy import select

from . import storage
from .auth import hash_token, new_token
from .db import SessionLocal
from .models import User, Workspace


def create_user(name: str, email: str) -> int:
    with SessionLocal() as db:
        if db.scalar(select(User).where(User.email == email)):
            print(f"Ya existe un usuario con el correo {email}.", file=sys.stderr)
            return 1
        workspace = Workspace(name=f"Espacio de {name}")
        db.add(workspace)
        db.flush()
        token = new_token()
        user = User(workspace_id=workspace.id, name=name, email=email, token_hash=hash_token(token))
        db.add(user)
        db.flush()
        workspace.owner_id = user.id
        db.commit()
    print(f"Usuario creado: {name} <{email}>")
    print(f"Token de acceso (se muestra una sola vez): {token}")
    return 0


def rotate_token(email: str) -> int:
    with SessionLocal() as db:
        user = db.scalar(select(User).where(User.email == email))
        if user is None:
            print(f"No existe un usuario con el correo {email}.", file=sys.stderr)
            return 1
        token = new_token()
        user.token_hash = hash_token(token)
        db.commit()
    print(f"Nuevo token de acceso (el anterior dejó de funcionar): {token}")
    return 0


def bootstrap() -> int:
    """Crea el primer usuario con el token dado por configuración
    (DUALREC_BOOTSTRAP_EMAIL y DUALREC_BOOTSTRAP_TOKEN). No hace nada si ya existe.
    En AWS el token lo genera Secrets Manager y se corre al arrancar el contenedor."""
    from .config import get_settings

    s = get_settings()
    if not s.bootstrap_email or not s.bootstrap_token:
        return 0
    with SessionLocal() as db:
        if db.scalar(select(User).where(User.email == s.bootstrap_email)):
            return 0
        workspace = Workspace(name=f"Espacio de {s.bootstrap_name}")
        db.add(workspace)
        db.flush()
        user = User(
            workspace_id=workspace.id,
            name=s.bootstrap_name,
            email=s.bootstrap_email,
            token_hash=hash_token(s.bootstrap_token),
        )
        db.add(user)
        db.flush()
        workspace.owner_id = user.id
        db.commit()
    print(f"Usuario inicial creado: {s.bootstrap_email}")
    return 0


def create_bucket() -> int:
    """Crea el bucket si no existe (útil en desarrollo con moto o MinIO)."""
    client, name = storage.client(), storage.bucket()
    if name not in [b["Name"] for b in client.list_buckets().get("Buckets", [])]:
        region = client.meta.region_name
        extra = {} if region == "us-east-1" else {"CreateBucketConfiguration": {"LocationConstraint": region}}
        client.create_bucket(Bucket=name, **extra)
        print(f"Bucket creado: {name}")
    # El reproductor web pide los videos por rangos desde otro origen.
    client.put_bucket_cors(
        Bucket=name,
        CORSConfiguration={
            "CORSRules": [
                {
                    "AllowedMethods": ["GET", "HEAD", "PUT"],
                    "AllowedOrigins": ["*"],
                    "AllowedHeaders": ["*"],
                    "ExposeHeaders": ["ETag", "Content-Length", "Content-Range"],
                }
            ]
        },
    )
    print(f"CORS configurado en {name}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(prog="app.cli")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("create-user", help="crea un usuario con su espacio y muestra el token")
    p.add_argument("name")
    p.add_argument("email")
    p = sub.add_parser("rotate-token", help="reemplaza el token de un usuario")
    p.add_argument("email")
    sub.add_parser("create-bucket", help="crea el bucket y configura CORS")
    sub.add_parser("bootstrap", help="crea el usuario inicial definido por configuración")
    args = parser.parse_args()
    if args.command == "create-user":
        return create_user(args.name, args.email)
    if args.command == "rotate-token":
        return rotate_token(args.email)
    if args.command == "bootstrap":
        return bootstrap()
    return create_bucket()


if __name__ == "__main__":
    raise SystemExit(main())
