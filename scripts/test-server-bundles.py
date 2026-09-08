#!/usr/bin/env python3
"""Exercise separately running native bundles with disposable, synthetic data.

Requires requests and Pillow. Build with `nextrs bundles build --root server`.
This checks local routing/authorization/image behavior, not live Vercel behavior.
"""

import base64
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import uuid

from PIL import Image
import requests


ROOT = Path(__file__).resolve().parents[1]
UPLOAD = "synthetic-upload-token-for-local-tests"
PROCESSOR = "synthetic-processor-token-for-local-tests"
CRON = "synthetic-cron-token-for-local-tests"
SESSION = "synthetic-session-for-local-tests"


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    checks = []
    failures = []
    processes = []
    with tempfile.TemporaryDirectory(prefix="daily-mirror-bundle-test-") as directory:
        temp = Path(directory)
        db = temp / "test.db"
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("DAILY_MIRROR_", "VERCEL", "CRON_SECRET"))}
        env.update(DAILY_MIRROR_STORAGE_BACKEND="local",
                   DAILY_MIRROR_STORAGE_DIR=str(temp / "photos"),
                   DAILY_MIRROR_DATABASE_URL=str(db),
                   DAILY_MIRROR_UPLOAD_TOKEN=UPLOAD,
                   DAILY_MIRROR_PROCESSOR_TOKEN=PROCESSOR,
                   DAILY_MIRROR_PROCESSING_PIPELINE="face-v5", CRON_SECRET=CRON,
                   NEXTRS_PUBLIC_DIR=str(ROOT / "server/public"))

        def check(label, response, expected):
            if response.status_code != expected:
                failures.append({"check": label, "expected": expected,
                                 "actual": response.status_code})
            else:
                checks.append(label)
            return response

        def call(base, method, path, expected, label, **kwargs):
            return check(label, requests.request(method, base + path, timeout=20,
                                                allow_redirects=False, **kwargs), expected)

        try:
            origins = {}
            for bundle in ["default", "images"]:
                port = free_port()
                work = ROOT / "server/.nextrs/bundles" / bundle
                with (temp / f"{bundle}.log").open("w") as log:
                    process = subprocess.Popen([str(work / "executable")], cwd=work,
                                               env={**env, "PORT": str(port)},
                                               stdout=log, stderr=log)
                processes.append(process)
                origins[bundle] = f"http://127.0.0.1:{port}"
                for attempt in range(100):
                    assert process.poll() is None, (temp / f"{bundle}.log").read_text()
                    try:
                        requests.get(origins[bundle] + "/login", timeout=0.2)
                        break
                    except requests.ConnectionError:
                        time.sleep(0.05)
                else:
                    raise AssertionError(f"{bundle} did not start")

            web, images = origins["default"], origins["images"]
            call(web, "GET", "/healthz", 200, "default owns health")
            call(images, "GET", "/healthz", 404, "images excludes health")
            call(web, "GET", "/login", 200, "default owns login")
            call(images, "GET", "/login", 404, "images excludes login")
            for token in [None, "invalid", "expired"]:
                headers = {} if token is None else {"Cookie": f"daily_mirror_session={token}"}
                call(images, "GET", "/api/photos", 401,
                     f"private photo read rejects {token}", headers=headers)

            # Seed an explicit test session in the disposable DB; password-login
            # behavior is covered by the existing auth unit tests.
            call(web, "POST", "/api/auth/login/password", 401,
                 "unknown password login rejected",
                 json={"username": "synthetic", "password": "unused-test-password"})
            with sqlite3.connect(db) as connection:
                user = str(uuid.uuid4())
                connection.execute("INSERT INTO users(id, username, display_name, password_hash) VALUES (?, ?, ?, ?)",
                                   (user, "synthetic", "Synthetic User", "unused"))
                for token, expiry in [(SESSION, int(time.time()) + 300), ("expired", 1)]:
                    digest = base64.urlsafe_b64encode(hashlib.sha256(token.encode()).digest()).decode().rstrip("=")
                    connection.execute("INSERT INTO auth_sessions(token_hash,user_id,expires_at) VALUES(?,?,?)",
                                       (digest, user, expiry))
            user_headers = {"Cookie": f"daily_mirror_session={SESSION}"}
            call(images, "GET", "/api/photos", 401, "expired stored session rejected",
                 headers={"Cookie": "daily_mirror_session=expired"})
            call(web, "GET", "/api/photos", 404, "default excludes authenticated photo listing", headers=user_headers)
            call(images, "GET", "/api/photos", 200, "images owns authenticated photo listing", headers=user_headers)
            call(images, "PUT", "/api/photos", 405, "owned route preserves method-not-allowed", headers=user_headers)
            call(images, "HEAD", "/api/photos", 200, "owned route preserves HEAD", headers=user_headers)
            call(web, "GET", "/", 200, "ordinary authenticated page", headers=user_headers)

            fixture = io.BytesIO()
            Image.new("RGB", (800, 600), (80, 120, 160)).save(fixture, format="JPEG")
            jpeg = fixture.getvalue()
            photo = "20260907T120000Z-bundle01"
            grant = {"capture_id": photo, "content_type": "image/jpeg", "content_length": len(jpeg)}
            for token in [None, "wrong"]:
                headers = {} if token is None else {"Authorization": f"Bearer {token}"}
                call(images, "POST", "/api/uploads", 401, f"upload rejects {token}", json=grant, headers=headers)
            with sqlite3.connect(db) as connection:
                tables = {r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                if "photos" in tables:
                    assert connection.execute("SELECT count(*) FROM photos").fetchone()[0] == 0
            checks.append("unauthorized uploads have no catalog side effects")
            upload_headers = {"Authorization": f"Bearer {UPLOAD}"}
            call(images, "POST", "/api/uploads", 200, "upload grant", json=grant, headers=upload_headers)
            call(images, "POST", "/api/photos", 400, "malformed JPEG rejected",
                 data=b"bad", headers={**upload_headers, "x-capture-id": photo})
            storage = temp / "photos"
            storage.mkdir(exist_ok=True)
            (storage / f"{photo}.jpg").write_bytes(jpeg)
            call(images, "POST", f"/api/uploads/{photo}", 204, "finalize uploaded fixture", headers=upload_headers)
            call(images, "POST", f"/api/uploads/{photo}", 204, "duplicate finalization", headers=upload_headers)
            thumbnail = call(images, "GET", f"/api/photos/{photo}/thumbnail", 200,
                             "thumbnail read", headers=user_headers)
            decoded = Image.open(io.BytesIO(thumbnail.content))
            assert decoded.format == "WEBP" and decoded.size == (320, 240)
            assert "private" in thumbnail.headers.get("Cache-Control", "")
            checks.append("thumbnail dimensions, format and private caching")
            call(images, "PATCH", f"/api/photos/{photo}", 400, "invalid rotation rejected",
                 json={"degrees": 13}, headers=user_headers)
            call(images, "PATCH", f"/api/photos/{photo}", 204, "rotation accepted",
                 json={"degrees": 90}, headers=user_headers)
            assert Image.open(storage / f"{photo}.jpg").size == (600, 800)
            checks.append("rotated original dimensions")
            for token in [None, "wrong"]:
                headers = {} if token is None else {"Authorization": f"Bearer {token}"}
                call(images, "GET", "/api/maintenance/reconcile", 401, f"cron rejects {token}", headers=headers)
            call(images, "GET", "/api/maintenance/reconcile", 200, "authorized reconciliation",
                 headers={"Authorization": f"Bearer {CRON}"})
            claim = {"pipeline_version": "face-v5", "worker_id": "synthetic-worker", "limit": 1}
            for token in [None, "wrong"]:
                headers = {} if token is None else {"Authorization": f"Bearer {token}"}
                call(web, "POST", "/api/processing/claim", 401, f"processor rejects {token}", json=claim, headers=headers)
            processor_headers = {"Authorization": f"Bearer {PROCESSOR}"}
            call(images, "POST", "/api/processing/claim", 404, "images excludes processor coordination",
                 json=claim, headers=processor_headers)
            lease = call(web, "POST", "/api/processing/claim", 200, "claim uploaded photo",
                         json=claim, headers=processor_headers).json()["photos"][0]
            assert lease["photo_id"] == photo
            completion = {"pipeline_version": "face-v5", "lease_token": lease["lease_token"],
                          "result": {"oriented_width": 600, "oriented_height": 800,
                                     "original_sha256": None, "processing_millis": 1, "faces": []}}
            for label in ["complete zero-face photo", "idempotent completion"]:
                call(web, "POST", f"/api/processing/photos/{photo}/complete", 204, label,
                     json=completion, headers=processor_headers)
            call(images, "PATCH", f"/api/photos/{photo}", 204, "rotation requeues completed photo",
                 json={"degrees": 90}, headers=user_headers)
            lease = call(web, "POST", "/api/processing/claim", 200, "claim rotated photo",
                         json=claim, headers=processor_headers).json()["photos"][0]
            assert lease["lease_token"] != completion["lease_token"]
            call(web, "POST", f"/api/processing/photos/{photo}/complete", 409,
                 "stale pre-rotation completion rejected", json=completion, headers=processor_headers)
            completion["lease_token"] = lease["lease_token"]
            completion["result"].update(oriented_width=800, oriented_height=600, faces=[{
                "detector_confidence": 0.98,
                "bounds": {"x": 0.25, "y": 0.2, "width": 0.25, "height": 0.4},
                "landmark_model": "synthetic-mesh", "landmark_schema": "synthetic-478",
                "landmarks": [{"x": 0.4, "y": 0.3, "z": 0.0}] * 478,
                "embedding_model": "synthetic-128", "embedding": [1.0] + [0.0] * 127,
            }])
            call(web, "POST", f"/api/processing/photos/{photo}/complete", 204,
                 "store synthetic face geometry", json=completion, headers=processor_headers)
            with sqlite3.connect(db) as connection:
                face = connection.execute("SELECT id FROM faces WHERE photo_id=?", (photo,)).fetchone()[0]
            crop_path = f"/api/admin/faces/{face}/crop"
            call(images, "GET", crop_path, 401, "face crop rejects anonymous access")
            call(web, "GET", crop_path, 404, "default excludes face crop", headers=user_headers)
            crop = call(images, "GET", crop_path, 200, "images serves face crop", headers=user_headers)
            decoded = Image.open(io.BytesIO(crop.content))
            assert decoded.format == "JPEG" and decoded.size == (384, 384)
            assert "private" in crop.headers.get("Cache-Control", "")
            checks.append("face crop dimensions, format and private caching")
            call(images, "DELETE", f"/api/photos/{photo}", 204, "delete synthetic photo", headers=user_headers)
            assert not (storage / f"{photo}.jpg").exists()
            print(json.dumps({"kind": "local-native-bundles", "passed": len(checks),
                              "checks": checks, "failures": failures}, indent=2))
        finally:
            for process in processes:
                process.terminate()
            for process in processes:
                process.wait(timeout=10)
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
