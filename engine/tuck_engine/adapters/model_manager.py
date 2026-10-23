"""Explicit, checksum-verified model downloads. No network at construction/status."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import threading
import time
from typing import Callable

PARAKEET_NAME = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"
PARAKEET_FILES = (
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
)
SPEAKER_FILE = "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx"
BASE = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
# Pinned from publisher's release asset SHA-256 and speaker checksum.txt, 2026-10-22.
ASSETS = {
    "parakeet": (
        f"{BASE}/asr-models/{PARAKEET_NAME}.tar.bz2",
        "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        487170055,
    ),
    "vad": (
        f"{BASE}/asr-models/silero_vad.onnx",
        "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        643854,
    ),
    "speakers": (
        f"{BASE}/speaker-recongition-models/{SPEAKER_FILE}",
        "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
        29596978,
    ),
}


class ModelError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def extract_parakeet(archive: Path, destination: Path) -> None:
    """Extract only known regular files, never paths, links, devices or archive extras."""
    with tarfile.open(archive, "r:bz2") as package:
        for filename in PARAKEET_FILES:
            member = package.getmember(f"{PARAKEET_NAME}/{filename}")
            if not member.isfile() or member.size <= 0 or member.size > 1_500_000_000:
                raise ModelError("The speech package contains an invalid model file.")
            source = package.extractfile(member)
            if source is None:
                raise ModelError("The speech package is incomplete.")
            with source, (destination / filename).open("wb") as target:
                shutil.copyfileobj(source, target, length=1024 * 1024)


class ModelManager:
    def __init__(self, data_dir: Path, emit: Callable[[str, dict], None]):
        self.root = Path(data_dir) / "models"
        self.parakeet_dir = self.root / "parakeet-v3-int8"
        self.vad_path = self.parakeet_dir / "silero_vad.onnx"
        self.speaker_path = self.root / SPEAKER_FILE
        self.emit = emit
        self._lock = threading.Lock()
        self.downloading = False
        self.progress = 0.0
        self._last_progress_at = 0.0
        self._last_stage = ""

    def installed(self, kind="parakeet") -> bool:
        if kind == "speakers":
            return (
                self.speaker_path.is_file()
                and self.speaker_path.stat().st_size == ASSETS["speakers"][2]
            )
        return (self.parakeet_dir / "verified.json").is_file() and all(
            (self.parakeet_dir / f).is_file()
            and (self.parakeet_dir / f).stat().st_size > 0
            for f in (*PARAKEET_FILES, "silero_vad.onnx")
        )

    def verify(self) -> None:
        if not self.installed():
            raise ModelError(
                "Install Parakeet in Settings before recording or importing audio."
            )
        try:
            expected = json.loads((self.parakeet_dir / "verified.json").read_text())
            for name in (*PARAKEET_FILES, "silero_vad.onnx"):
                if sha256_file(self.parakeet_dir / name) != expected[name]:
                    raise ModelError(
                        "A local model failed its integrity check. Reinstall Parakeet in Settings."
                    )
        except (OSError, ValueError, KeyError) as exc:
            raise ModelError(
                "The local speech package is incomplete. Reinstall Parakeet in Settings."
            ) from exc

    def _progress(self, kind: str, progress: float, stage: str):
        self.progress = round(max(0, min(1, progress)), 4)
        now = time.monotonic()
        if (
            stage != self._last_stage
            or self.progress == 1
            or now - self._last_progress_at >= 0.1
        ):
            self._last_progress_at, self._last_stage = now, stage
            self.emit(
                "model_progress",
                {"kind": kind, "progress": self.progress, "stage": stage},
            )

    def _download(
        self, asset: str, destination: Path, kind: str, lower: float, upper: float
    ):
        import httpx

        url, expected, size = ASSETS[asset]
        digest = hashlib.sha256()
        received = 0
        try:
            with httpx.stream(
                "GET", url, follow_redirects=True, timeout=httpx.Timeout(60, connect=20)
            ) as response:
                response.raise_for_status()
                with destination.open("wb") as target:
                    for block in response.iter_bytes(256 * 1024):
                        received += len(block)
                        if received > size:
                            raise ModelError(
                                "The model download does not match its published size."
                            )
                        target.write(block)
                        digest.update(block)
                        self._progress(
                            kind,
                            lower + (upper - lower) * received / size,
                            "Downloading",
                        )
            if received != size or digest.hexdigest() != expected:
                raise ModelError(
                    "The model download failed its integrity check. Try installing again."
                )
        except httpx.HTTPError as exc:
            raise ModelError(
                "The model could not be downloaded. Check your connection and try again."
            ) from exc

    def install(self, kind: str) -> dict:
        if kind not in ("parakeet", "speakers"):
            raise ValueError("Choose Parakeet or speaker recognition.")
        if not self._lock.acquire(blocking=False):
            raise ModelError("A model download is already in progress.")
        self.downloading = True
        self.root.mkdir(parents=True, exist_ok=True, mode=0o700)
        try:
            self._progress(kind, 0, "Preparing")
            with tempfile.TemporaryDirectory(prefix=".install-", dir=self.root) as temp:
                staging = Path(temp)
                if kind == "speakers":
                    output = staging / SPEAKER_FILE
                    self._download("speakers", output, kind, 0, 0.99)
                    os.replace(output, self.speaker_path)
                else:
                    archive = staging / "model.tar.bz2"
                    self._download("parakeet", archive, kind, 0, 0.94)
                    model = staging / "model"
                    model.mkdir()
                    self._progress(kind, 0.94, "Unpacking")
                    extract_parakeet(archive, model)
                    self._download("vad", model / "silero_vad.onnx", kind, 0.95, 0.99)
                    manifest = {
                        name: sha256_file(model / name)
                        for name in (*PARAKEET_FILES, "silero_vad.onnx")
                    }
                    (model / "verified.json").write_text(json.dumps(manifest))
                    if self.parakeet_dir.exists():
                        shutil.rmtree(self.parakeet_dir)
                    os.replace(model, self.parakeet_dir)
            self._progress(kind, 1, "Installed")
            return {"kind": kind, "installed": True}
        except Exception:
            self._progress(kind, self.progress, "Failed")
            raise
        finally:
            self.downloading = False
            self._lock.release()
