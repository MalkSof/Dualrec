# Imagen de la API de Dualrec. La base sale del espejo público de AWS para no
# depender de los límites de descarga de Docker Hub.
FROM public.ecr.aws/docker/library/python:3.12-slim

# ffmpeg: reempaqueta las pistas subidas para que el navegador pueda reproducirlas.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg \
    && rm -rf /var/lib/apt/lists/*

ENV PYTHONUNBUFFERED=1 \
    UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    PATH="/app/.venv/bin:$PATH"

WORKDIR /app

# Primero las dependencias: esta capa se reutiliza mientras no cambie uv.lock.
COPY pyproject.toml uv.lock ./
RUN pip install --no-cache-dir uv && uv sync --frozen --no-dev

COPY alembic.ini entrypoint.sh ./
COPY alembic ./alembic
COPY app ./app

RUN useradd --create-home dualrec && chown -R dualrec /app
USER dualrec

EXPOSE 8000
CMD ["sh", "entrypoint.sh"]
