from fastapi import APIRouter, Depends, FastAPI
from fastapi.middleware.cors import CORSMiddleware

from .auth import current_user
from .config import get_settings
from .models import User
from .routers import share, videos

settings = get_settings()
prefix = settings.api_prefix.rstrip("/")

app = FastAPI(
    title="Dualrec API",
    version="0.1.0",
    docs_url=f"{prefix}/docs",
    openapi_url=f"{prefix}/openapi.json",
)
app.add_middleware(
    CORSMiddleware,
    allow_origins=[o.strip() for o in settings.cors_origins.split(",") if o.strip()],
    allow_methods=["*"],
    allow_headers=["*"],
)

api = APIRouter(prefix=prefix)
api.include_router(videos.router)
api.include_router(share.router)


@api.get("/health", tags=["sistema"])
def health():
    return {"status": "ok"}


@api.get("/me", tags=["sistema"])
def me(user: User = Depends(current_user)):
    """La app de escritorio lo usa para verificar la URL y el token."""
    return {"id": user.id, "name": user.name, "email": user.email, "workspace_id": user.workspace_id}


app.include_router(api)
