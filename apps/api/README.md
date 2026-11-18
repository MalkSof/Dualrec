# Dualrec API

Backend de Dualrec: registra los videos, coordina la **subida multipart reanudable** y sirve el **link para compartir**. Python 3.12+, FastAPI, SQLAlchemy 2 + Alembic y Amazon S3.

Los archivos nunca pasan por la API: la app de escritorio sube cada parte directo al almacenamiento con URLs prefirmadas, y el navegador los reproduce desde ahí.

## Probarla en tu máquina

No hace falta Docker ni una cuenta en la nube:

```bash
cd apps/api
uv sync
uv run python dev.py
```

Eso levanta un S3 simulado en `:9000`, aplica las migraciones sobre SQLite, crea un usuario de prueba y arranca la API en `http://127.0.0.1:8000` (documentación interactiva en `/docs`). El token queda en `apps/api/.dev-token`.

> El S3 simulado guarda todo en memoria: los videos subidos se pierden al cerrar `dev.py`.

## Tests

```bash
uv run pytest
```

Cubren el alta, la subida por partes contra un S3 simulado con HTTP real, los reintentos, el aislamiento entre espacios de trabajo, el cierre, el link público y la reproducción por rangos.

## Cómo es una subida

| Paso | Llamada | Qué hace |
| --- | --- | --- |
| 1 | `POST /videos` | Crea el video y una subida multipart por pista. Devuelve los ids, el `share_url` y el tamaño de parte (8 MB). Con `client_id` es idempotente. |
| 2 | `POST /videos/{id}/tracks/{track}/parts/{n}/url` | Devuelve la URL prefirmada para subir la parte `n`. |
| 3 | `PUT` a esa URL | La app sube los bytes directo al almacenamiento y recibe un `ETag`. |
| 4 | `PUT /videos/{id}/tracks/{track}/parts/{n}` | Registra el `ETag`. Repetirlo no duplica. |
| 5 | `POST /videos/{id}/complete` | Cierra las subidas y manda el video a procesar. |

Para retomar, `GET /videos/{id}/tracks/{track}/parts` lista las partes ya registradas.

Estados del video: `recording` → `uploading` → `processing` → `ready`, o `failed`.

El procesamiento de esta etapa reempaqueta cada pista sin recodificar: la app sube MP4 fragmentado (se puede subir mientras se graba) y el servidor lo deja como MP4 clásico con el índice al principio, para que el navegador conozca la duración y pueda saltar a cualquier minuto. En la etapa 3 este paso pasa a ser la primera tarea de la cadena de Celery.

## Otros endpoints

- `GET /share/{token}`: público, sin token. Devuelve título, autor, duración y las URLs temporales de cada pista. Un video privado responde igual que uno inexistente.
- `GET /videos`, `GET /videos/{id}`, `PATCH /videos/{id}` (título y visibilidad), `DELETE /videos/{id}`.
- `GET /me`: verifica el token. `GET /health`: estado del servicio.

Todo menos `/share` y `/health` pide `Authorization: Bearer <token>`.

## Usuarios

```bash
uv run python -m app.cli create-user "Nombre" correo@ejemplo.com   # muestra el token una sola vez
uv run python -m app.cli rotate-token correo@ejemplo.com
```

Cada usuario nace con su propio espacio de trabajo. En la base solo se guarda el hash del token.

## Producción (AWS)

Se despliega con `infra/aws/deploy.ps1`: construye esta carpeta como contenedor (`Dockerfile`) y la corre en ECS Fargate con RDS PostgreSQL y S3. Ver [infra/aws](../../infra/aws/README.md).

Al arrancar, el contenedor aplica las migraciones y crea el usuario inicial (`entrypoint.sh`). En AWS no se configuran claves de S3: la API usa el rol de IAM de la tarea.

Para correrla fuera de AWS contra recursos reales, copiar `.env.example` a `.env`.

## Estructura

```
app/
  main.py          aplicación y rutas base
  config.py        configuración por variables de entorno
  models.py        workspaces, users, videos, tracks, upload_parts
  storage.py       S3: multipart y URLs prefirmadas
  processing.py    reempaquetado de las pistas subidas
  auth.py          token de acceso
  routers/         videos y link para compartir
  cli.py           usuarios y bucket
Dockerfile         imagen para AWS (incluye ffmpeg)
entrypoint.sh      migraciones, usuario inicial y servidor
alembic/           migraciones
tests/
dev.py             entorno de desarrollo en un comando
```
