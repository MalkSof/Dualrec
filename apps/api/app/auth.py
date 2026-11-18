"""Autenticación por token de acceso (Bearer). Cada usuario tiene un token que
la app de escritorio guarda en sus ajustes."""

import hashlib
import secrets

from fastapi import Depends, Header, HTTPException
from sqlalchemy import select
from sqlalchemy.orm import Session

from .db import get_db
from .models import User


def new_token() -> str:
    return "dr_" + secrets.token_urlsafe(32)


def hash_token(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


def current_user(
    authorization: str | None = Header(default=None), db: Session = Depends(get_db)
) -> User:
    scheme, _, token = (authorization or "").partition(" ")
    if scheme.lower() != "bearer" or not token:
        raise HTTPException(401, "Falta el token de acceso.")
    user = db.scalar(select(User).where(User.token_hash == hash_token(token.strip())))
    if user is None:
        raise HTTPException(401, "Token de acceso inválido.")
    return user
