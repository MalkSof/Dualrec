import shutil
import subprocess

import httpx
import pytest

from tests.conftest import make_user

MB = 1024 * 1024
TRACKS = [
    {"kind": "screen", "codec": "h264", "width": 1920, "height": 1080},
    {"kind": "mic", "codec": "aac"},
]


def create(client, auth, **extra):
    r = client.post("/videos", json={"title": "Demo", "tracks": TRACKS, **extra}, headers=auth)
    assert r.status_code in (200, 201), r.text
    return r.json()


def upload_part(client, auth, video, track, number, data: bytes):
    """Sube una parte como lo hace la app: pide la URL, hace PUT y registra el ETag."""
    base = f"/videos/{video['id']}/tracks/{track['id']}/parts/{number}"
    url = client.post(f"{base}/url", headers=auth).json()["url"]
    put = httpx.put(url, content=data)
    assert put.status_code == 200, put.text
    r = client.put(base, json={"etag": put.headers["ETag"], "size_bytes": len(data)}, headers=auth)
    assert r.status_code == 200, r.text


def fragmented_mp4(tmp_path, audio: bool) -> bytes:
    """Un MP4 fragmentado real, como los que escribe la app al grabar."""
    out = tmp_path / ("a.m4a" if audio else "v.mp4")
    source = (
        ["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac"]
        if audio
        else ["-f", "lavfi", "-i", "testsrc=size=320x240:rate=30:duration=2", "-c:v", "libx264", "-pix_fmt", "yuv420p"]
    )
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *source,
         "-movflags", "+frag_keyframe+empty_moov+default_base_moof", "-f", "mp4", str(out)],
        check=True,
    )
    return out.read_bytes()


def test_requires_token(client):
    assert client.get("/videos").status_code == 401
    assert client.get("/videos", headers={"Authorization": "Bearer nope"}).status_code == 401


def test_create_returns_share_link_and_one_upload_per_track(client, auth):
    video = create(client, auth)
    assert video["status"] == "recording"
    assert video["share_url"] == f"https://dualrec.test/v/{video['share_token']}"
    assert video["part_size"] == 8 * MB
    assert [t["kind"] for t in video["tracks"]] == ["mic", "screen"]
    assert all(not t["uploaded"] and t["url"] is None for t in video["tracks"])


def test_create_is_idempotent_per_client_id(client, auth):
    first = create(client, auth, client_id="2026-11-18_18-00-00")
    again = create(client, auth, client_id="2026-11-18_18-00-00")
    assert again["id"] == first["id"]
    assert len(client.get("/videos", headers=auth).json()) == 1


def test_duplicate_track_kinds_are_rejected(client, auth):
    r = client.post("/videos", json={"tracks": [TRACKS[0], TRACKS[0]]}, headers=auth)
    assert r.status_code == 422


def test_resume_lists_registered_parts_and_reupload_overwrites(client, auth):
    video = create(client, auth)
    screen = next(t for t in video["tracks"] if t["kind"] == "screen")
    upload_part(client, auth, video, screen, 1, b"a" * (5 * MB))
    upload_part(client, auth, video, screen, 1, b"b" * (5 * MB))  # reintento de la misma parte
    upload_part(client, auth, video, screen, 2, b"c" * 100)

    parts = client.get(f"/videos/{video['id']}/tracks/{screen['id']}/parts", headers=auth).json()
    assert [(p["part_number"], p["size_bytes"]) for p in parts] == [(1, 5 * MB), (2, 100)]
    assert client.get(f"/videos/{video['id']}", headers=auth).json()["status"] == "uploading"


def test_other_workspaces_cannot_touch_a_video(client, auth):
    video = create(client, auth)
    track = video["tracks"][0]
    intruder = make_user("Beto", "beto@example.com")
    base = f"/videos/{video['id']}"
    assert client.get(base, headers=intruder).status_code == 404
    assert client.post(f"{base}/tracks/{track['id']}/parts/1/url", headers=intruder).status_code == 404
    assert client.post(f"{base}/complete", json={"duration_ms": 1}, headers=intruder).status_code == 404
    assert client.delete(base, headers=intruder).status_code == 404
    assert client.get("/videos", headers=intruder).json() == []


@pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="necesita ffmpeg")
def test_full_flow_upload_complete_share_and_play(client, auth, tmp_path):
    video = create(client, auth)
    by_kind = {t["kind"]: t for t in video["tracks"]}
    screen_bytes = fragmented_mp4(tmp_path, audio=False)
    mic_bytes = fragmented_mp4(tmp_path, audio=True)
    upload_part(client, auth, video, by_kind["screen"], 1, screen_bytes)
    upload_part(client, auth, video, by_kind["mic"], 1, mic_bytes)

    # Antes de cerrar, el link existe pero todavía no entrega pistas.
    shared = client.get(f"/share/{video['share_token']}").json()
    assert shared["status"] == "uploading" and shared["tracks"] == []

    done = client.post(f"/videos/{video['id']}/complete", json={"duration_ms": 2000}, headers=auth)
    assert done.status_code == 200, done.text
    # Cerrar dos veces no rompe nada.
    assert client.post(f"/videos/{video['id']}/complete", json={"duration_ms": 2000}, headers=auth).status_code == 200

    # El procesamiento en segundo plano ya corrió (TestClient lo ejecuta antes de devolver).
    shared = client.get(f"/share/{video['share_token']}").json()
    assert shared["status"] == "ready", shared
    assert shared["duration_ms"] == 2000
    assert shared["author"] == "Ana"
    assert {t["kind"] for t in shared["tracks"]} == {"screen", "mic"}

    for track in shared["tracks"]:
        # El navegador pide por rangos: tiene que responder 206.
        r = httpx.get(track["url"], headers={"Range": "bytes=0-99"})
        assert r.status_code == 206
        body = httpx.get(track["url"]).content
        assert len(body) == track["size_bytes"]
        # Reempaquetado: el índice (moov) quedó antes que los datos (mdat) y ya no hay fragmentos.
        assert body.index(b"moov") < body.index(b"mdat")
        assert b"moof" not in body

    detail = client.get(f"/videos/{video['id']}", headers=auth).json()
    assert detail["error"] is None
    assert all(t["uploaded"] and "/raw/" not in t["url"] for t in detail["tracks"])


@pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="necesita ffmpeg")
def test_duration_is_read_from_the_file_when_the_app_could_not_report_it(client, auth, tmp_path):
    # La app se cerró a mitad de la grabación: al retomar la subida no sabe cuánto duró.
    video = create(client, auth)
    screen = next(t for t in video["tracks"] if t["kind"] == "screen")
    upload_part(client, auth, video, screen, 1, fragmented_mp4(tmp_path, audio=False))
    client.post(f"/videos/{video['id']}/complete", json={"duration_ms": 0}, headers=auth)
    detail = client.get(f"/videos/{video['id']}", headers=auth).json()
    assert detail["status"] == "ready"
    assert 1900 <= detail["duration_ms"] <= 2100


def test_empty_tracks_are_dropped_on_complete(client, auth):
    video = create(client, auth)
    screen = next(t for t in video["tracks"] if t["kind"] == "screen")
    upload_part(client, auth, video, screen, 1, b"no es un mp4")
    done = client.post(f"/videos/{video['id']}/complete", json={"duration_ms": 10}, headers=auth).json()
    # El micrófono no subió nada: se descarta. La pantalla no se pudo reempaquetar
    # (no es un MP4 real), así que se conserva el archivo subido y queda el aviso.
    detail = client.get(f"/videos/{done['id']}", headers=auth).json()
    assert [t["kind"] for t in detail["tracks"]] == ["screen"]
    assert detail["status"] == "ready"
    assert "screen" in (detail["error"] or "")


def test_private_and_unknown_links_look_the_same(client, auth):
    video = create(client, auth)
    assert client.get(f"/share/{video['share_token']}").status_code == 200
    client.patch(f"/videos/{video['id']}", json={"visibility": "private"}, headers=auth)
    private = client.get(f"/share/{video['share_token']}")
    unknown = client.get("/share/no-existe")
    assert private.status_code == unknown.status_code == 404
    assert private.json() == unknown.json()


def test_delete_removes_video_and_its_link(client, auth):
    video = create(client, auth)
    assert client.delete(f"/videos/{video['id']}", headers=auth).status_code == 204
    assert client.get(f"/videos/{video['id']}", headers=auth).status_code == 404
    assert client.get(f"/share/{video['share_token']}").status_code == 404
