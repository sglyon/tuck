#!/usr/bin/env python3
"""Smoke-test a native runtime with an isolated library. Never records microphone audio."""

from __future__ import annotations
import argparse
import json
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
from uuid import uuid4


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=300, help="Per-command deadline, including first model preparation")
    parser.add_argument(
        "--models", type=Path, help="Existing verified model directory (no download)"
    )
    parser.add_argument(
        "--fixture", type=Path, help="Public WAV fixture to transcribe (no recording)"
    )
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="tuck-packaged-") as directory:
        data = Path(directory)
        if args.models:
            (data / "models").symlink_to(
                args.models.resolve(), target_is_directory=True
            )
        process = subprocess.Popen(
            [str(args.engine.resolve()), "--data-dir", str(data)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        output = queue.Queue()

        def read():
            for line in process.stdout:
                try:
                    output.put(json.loads(line))
                except ValueError:
                    output.put({"invalidProtocol": line[:200]})
            output.put({"exited": True})

        threading.Thread(target=read, daemon=True).start()

        def call(method, params=None):
            identifier = str(uuid4())
            process.stdin.write(
                json.dumps({"id": identifier, "method": method, "params": params or {}})
                + "\n"
            )
            process.stdin.flush()
            deadline = time.monotonic() + args.timeout
            while time.monotonic() < deadline:
                message = output.get(timeout=max(0.1, deadline - time.monotonic()))
                if "invalidProtocol" in message:
                    raise AssertionError("Worker emitted non-JSON stdout")
                if message.get("exited"):
                    raise RuntimeError(process.stderr.read()[-3000:])
                if message.get("id") == identifier:
                    if "error" in message:
                        raise RuntimeError(message["error"])
                    return message.get("result")
            raise TimeoutError(method)

        try:
            bootstrap = call("bootstrap")
            assert bootstrap["meetings"] == [], "First launch must be empty"
            assert bootstrap["status"]["activeMeetingId"] is None
            meeting = call("create_meeting", {"title": "Packaged runtime check"})
            call(
                "update_meeting",
                {"id": meeting["id"], "changes": {"notes": "Saved on this device."}},
            )
            exported = call(
                "export_meeting", {"meetingId": meeting["id"], "format": "markdown"}
            )
            assert "Saved on this device." in exported["content"]
            transcript_verified = False
            if args.fixture:
                imported = call(
                    "import_audio",
                    {
                        "path": str(args.fixture.resolve()),
                        "title": "Public verification fixture",
                    },
                )
                words = " ".join(s["text"] for s in imported["segments"])
                assert "country" in words.lower() and "ask" in words.lower(), words
                assert imported["status"] == "complete"
                transcript_verified = True
            call("shutdown")
            process.wait(timeout=30)
            assert process.returncode == 0, process.stderr.read()[-3000:]
            print(
                json.dumps(
                    {
                        "nativeRuntime": "passed",
                        "emptyStartup": True,
                        "persistenceAndExport": True,
                        "realParakeetImport": transcript_verified,
                        "cleanShutdown": True,
                    }
                )
            )
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
