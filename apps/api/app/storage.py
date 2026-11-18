"""Almacenamiento en Amazon S3. Las pistas suben directo desde la app de
escritorio con URLs prefirmadas: los bytes nunca pasan por la API."""

from functools import lru_cache

import boto3
from botocore.config import Config

from .config import get_settings


@lru_cache
def client():
    s = get_settings()
    return boto3.client(
        "s3",
        # Endpoint regional: una URL prefirmada contra el endpoint global hace
        # que S3 redirija, y los PUT redirigidos fallan en buckets recién creados.
        endpoint_url=s.s3_endpoint_url or f"https://s3.{s.s3_region}.amazonaws.com",
        region_name=s.s3_region,
        aws_access_key_id=s.s3_access_key_id,
        aws_secret_access_key=s.s3_secret_access_key,
        config=Config(signature_version="s3v4", s3={"addressing_style": s.s3_addressing_style}),
    )


def bucket() -> str:
    return get_settings().s3_bucket


def start_upload(key: str, content_type: str) -> str:
    r = client().create_multipart_upload(Bucket=bucket(), Key=key, ContentType=content_type)
    return r["UploadId"]


def part_url(key: str, upload_id: str, part_number: int) -> str:
    return client().generate_presigned_url(
        "upload_part",
        Params={"Bucket": bucket(), "Key": key, "UploadId": upload_id, "PartNumber": part_number},
        ExpiresIn=get_settings().upload_url_ttl,
    )


def finish_upload(key: str, upload_id: str, parts: list[tuple[int, str]]) -> None:
    client().complete_multipart_upload(
        Bucket=bucket(),
        Key=key,
        UploadId=upload_id,
        MultipartUpload={"Parts": [{"PartNumber": n, "ETag": etag} for n, etag in sorted(parts)]},
    )


def abort_upload(key: str, upload_id: str) -> None:
    try:
        client().abort_multipart_upload(Bucket=bucket(), Key=key, UploadId=upload_id)
    except Exception:
        pass  # ya no existe: no hay nada que limpiar


def download_url(key: str, filename: str | None = None) -> str:
    params = {"Bucket": bucket(), "Key": key}
    if filename:
        params["ResponseContentDisposition"] = f'attachment; filename="{filename}"'
    return client().generate_presigned_url(
        "get_object", Params=params, ExpiresIn=get_settings().download_url_ttl
    )


def download_file(key: str, path: str) -> None:
    client().download_file(bucket(), key, path)


def upload_file(path: str, key: str, content_type: str) -> None:
    client().upload_file(path, bucket(), key, ExtraArgs={"ContentType": content_type})


def delete(key: str) -> None:
    try:
        client().delete_object(Bucket=bucket(), Key=key)
    except Exception:
        pass  # borrar es de limpieza: no debe frenar la operación principal
