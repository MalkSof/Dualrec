"""Configuración por variables de entorno (prefijo DUALREC_) o archivo .env."""

from functools import lru_cache

from pydantic_settings import BaseSettings, SettingsConfigDict
from sqlalchemy import URL


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="DUALREC_", env_file=".env", extra="ignore")

    # --- Base de datos ---
    # SQLite alcanza para desarrollo. En AWS (RDS PostgreSQL) se usan las
    # variables DB_* de abajo, que llegan separadas desde Secrets Manager.
    database_url: str = "sqlite:///./dualrec.db"
    db_host: str | None = None
    db_port: int = 5432
    db_name: str = "dualrec"
    db_user: str = "dualrec"
    db_password: str = ""

    # --- Almacenamiento: Amazon S3 ---
    s3_bucket: str = "dualrec"
    s3_region: str = "us-east-1"
    # Sin claves se usa la cadena de credenciales de AWS (el rol de IAM de la
    # tarea en ECS). Solo hacen falta fuera de AWS.
    s3_access_key_id: str | None = None
    s3_secret_access_key: str | None = None
    # Solo para desarrollo con un S3 simulado (moto, MinIO), que además
    # necesita el estilo "path". Con Amazon S3 se dejan como están.
    s3_endpoint_url: str | None = None
    s3_addressing_style: str = "virtual"

    # Dirección pública de la web, para armar el link de compartir.
    public_web_url: str = "http://localhost:5173"
    cors_origins: str = "*"
    # Prefijo bajo el que se publica la API (por ejemplo "/api" detrás de CloudFront).
    api_prefix: str = ""

    # Tamaño de cada parte de la subida multipart (S3 exige 5 MB como mínimo).
    part_size: int = 8 * 1024 * 1024
    # Vigencia de las URLs prefirmadas, en segundos.
    upload_url_ttl: int = 3600
    download_url_ttl: int = 6 * 3600

    ffmpeg_path: str = "ffmpeg"

    # Primer usuario: si están definidos, se crea al arrancar (ver `cli bootstrap`).
    bootstrap_name: str = "Admin"
    bootstrap_email: str | None = None
    bootstrap_token: str | None = None

    @property
    def sqlalchemy_url(self) -> str | URL:
        if self.db_host:
            # URL.create escapa la contraseña, que puede traer cualquier símbolo.
            return URL.create(
                "postgresql+psycopg",
                username=self.db_user,
                password=self.db_password,
                host=self.db_host,
                port=self.db_port,
                database=self.db_name,
            )
        return self.database_url


@lru_cache
def get_settings() -> Settings:
    return Settings()
