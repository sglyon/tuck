#!/usr/bin/env python3
"""Explicit speech smoke test. --install downloads models; --download-fixture fetches public audio."""

from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "engine"))
from tuck_engine.adapters.capture import CaptureEngine

FIXTURE_URL = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/test_wavs/en.wav"
FIXTURE_SHA = "148b936b43ce7c546a866e64da059f0458aee2d65e617f16e9d94f06e8d99ed6"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument(
        "--install",
        action="store_true",
        help="Explicitly download Parakeet, VAD and speaker models (~518MB)",
    )
    parser.add_argument(
        "--download-fixture",
        action="store_true",
        help="Fetch the publisher's 185KB public English example",
    )
    args = parser.parse_args()
    args.data_dir.mkdir(parents=True, exist_ok=True)
    engine = CaptureEngine(args.data_dir, lambda *args: None)
    if args.install:
        if not engine.models.installed():
            engine.install_model("parakeet")
        if not engine.models.installed("speakers"):
            engine.install_model("speakers")
    fixture = args.fixture or args.data_dir / "en.wav"
    if args.download_fixture:
        import httpx

        response = httpx.get(FIXTURE_URL, follow_redirects=True, timeout=60)
        response.raise_for_status()
        fixture.write_bytes(response.content)
    if hashlib.sha256(fixture.read_bytes()).hexdigest() != FIXTURE_SHA:
        raise SystemExit(
            "Use the publisher fixture with the pinned hash for this verification."
        )
    segments = []
    speaker_extractor_used = False
    original = engine.runtime.transcribe

    def transcribe(*args, **kwargs):
        nonlocal speaker_extractor_used
        speaker_extractor_used = engine.runtime.extractor is not None
        return original(*args, **kwargs)

    engine.runtime.transcribe = transcribe
    started = time.perf_counter()
    duration = engine.import_file(
        str(fixture), "smoke-" + str(uuid4()), segments.append, threads=2
    )
    elapsed = time.perf_counter() - started
    transcript = " ".join(segment["text"] for segment in segments)
    assert "country" in transcript.lower() and "ask" in transcript.lower(), transcript
    assert all(
        0 <= segment["start"] < segment["end"] <= duration + 0.002
        for segment in segments
    )
    assert engine.runtime.recognizer is None, "The model must unload after an import."
    result = {
        "audioSeconds": duration,
        "coldImportSeconds": round(elapsed, 3),
        "threads": 2,
        "speakerExtractorUsed": speaker_extractor_used,
        "modelUnloadedAfterImport": True,
        "transcript": transcript,
        "segments": segments,
    }
    try:
        import resource

        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        result["peakRSSBytes"] = peak if sys.platform == "darwin" else peak * 1024
    except ImportError:
        pass
    print(json.dumps(result, indent=2))
    engine.shutdown()


if __name__ == "__main__":
    main()
