#!/bin/sh
# Arranque del contenedor: migraciones, usuario inicial y servidor.
set -e

alembic upgrade head
python -m app.cli bootstrap

exec uvicorn app.main:app --host 0.0.0.0 --port 8000 --proxy-headers --forwarded-allow-ips "*"
