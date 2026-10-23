"""Thin JSON-lines inbound adapter and desktop composition root."""

from __future__ import annotations
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import signal
import sys
import threading

from tuck_engine.application.service import MeetingService
from tuck_engine.domain.model import DomainError
from tuck_engine.adapters.sqlite_store import sqlite_factory, LocalAudioArchive

MAX_LINE = 1_048_576
METHODS = frozenset(
    {
        "bootstrap",
        "create_meeting",
        "update_meeting",
        "delete_meeting",
        "rename_speaker",
        "add_bookmark",
        "remove_bookmark",
        "load_example",
        "start_recording",
        "pause_recording",
        "resume_recording",
        "stop_recording",
        "import_audio",
        "install_model",
        "unload_model",
        "get_status",
        "get_settings",
        "update_settings",
        "save_secret",
        "chat",
        "summarize",
        "publish_notion",
        "chatgpt_sign_in",
        "chatgpt_accounts",
        "chatgpt_select",
        "chatgpt_sign_out",
        "list_models",
        "export_meeting",
    }
)


class JsonLines:
    def __init__(self, output=None):
        self.output = output or sys.stdout
        self.lock = threading.Lock()

    def write(self, data: dict) -> None:
        encoded = json.dumps(
            data, ensure_ascii=False, separators=(",", ":"), allow_nan=False
        )
        with self.lock:
            self.output.write(encoded + "\n")
            self.output.flush()

    def event(self, event: str, payload: dict) -> None:
        self.write({"event": event, "payload": payload})

    def dispatch(self, service: MeetingService, request: dict) -> None:
        request_id = request.get("id") if isinstance(request, dict) else None
        try:
            if (
                not isinstance(request, dict)
                or not isinstance(request_id, str)
                or len(request_id) > 128
            ):
                raise DomainError("Invalid request identifier.")
            method, params = request.get("method"), request.get("params", {})
            if method not in METHODS or not isinstance(params, dict):
                raise DomainError("Unknown command or invalid arguments.")
            result = getattr(service, method)(**params)
            self.write({"id": request_id, "result": result})
        except Exception as exc:
            known = isinstance(exc, DomainError) or type(exc).__name__ in {
                "IntelligenceError",
                "AccountError",
                "CredentialStoreUnavailable",
                "PublishError",
                "PublishUncertain",
                "CaptureError",
                "ModelError",
                "ModelDownloadError",
                "ModelNotInstalled",
                "CaptureUnavailable",
            }
            if isinstance(exc, TypeError):
                message, code = (
                    "This command's arguments are invalid.",
                    "invalid_request",
                )
            elif known:
                message, code = str(exc)[:600], getattr(exc, "code", "operation_failed")
            else:
                message, code = (
                    "This operation could not finish. Your saved meeting data is preserved.",
                    "operation_failed",
                )
            self.write({"id": request_id, "error": {"code": code, "message": message}})


def compose(data_dir: Path, emit) -> MeetingService:
    from tuck_engine.adapters.capture import CaptureEngine
    from tuck_engine.adapters.secrets import SecretStore
    from tuck_engine.adapters.chatgpt import ChatGPTAuth
    from tuck_engine.adapters.llm import Intelligence
    from tuck_engine.adapters.notion import NotionPublisher

    secrets = SecretStore()
    auth = ChatGPTAuth(data_dir, secrets)
    return MeetingService(
        sqlite_factory(data_dir),
        CaptureEngine(data_dir, emit),
        Intelligence(auth, secrets),
        NotionPublisher(secrets),
        secrets,
        auth,
        LocalAudioArchive(data_dir),
        emit,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Tuck local meeting runtime")
    parser.add_argument("--data-dir", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    protocol = JsonLines()
    service = compose(args.data_dir.resolve(), protocol.event)

    # The native host closes stdin for an orderly exit. OS termination also drains capture.
    def terminate(_signum, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, terminate)
    executor = ThreadPoolExecutor(max_workers=4, thread_name_prefix="tuck-command")
    pending_slots = threading.BoundedSemaphore(16)
    try:
        while True:
            line = sys.stdin.readline(MAX_LINE + 1)
            if not line:
                break
            if len(line) > MAX_LINE:
                while line and not line.endswith("\n"):
                    line = sys.stdin.readline(MAX_LINE + 1)
                protocol.write(
                    {
                        "id": None,
                        "error": {
                            "code": "request_too_large",
                            "message": "The command is too large.",
                        },
                    }
                )
                continue
            try:
                request = json.loads(line)
            except (ValueError, RecursionError):
                protocol.write(
                    {
                        "id": None,
                        "error": {
                            "code": "invalid_json",
                            "message": "The command is not valid JSON.",
                        },
                    }
                )
                continue
            if isinstance(request, dict) and request.get("method") == "shutdown":
                service.shutdown()
                protocol.write({"id": request.get("id"), "result": None})
                break
            if not pending_slots.acquire(blocking=False):
                protocol.write(
                    {
                        "id": request.get("id") if isinstance(request, dict) else None,
                        "error": {
                            "code": "busy",
                            "message": "Tuck is busy. Wait for the current operation to finish.",
                        },
                    }
                )
                continue
            future = executor.submit(protocol.dispatch, service, request)
            future.add_done_callback(lambda _future: pending_slots.release())
    except (KeyboardInterrupt, BrokenPipeError):
        pass
    finally:
        # Close recording sources before waiting for any in-flight hosted AI requests.
        service.shutdown()
        executor.shutdown(wait=True, cancel_futures=True)


if __name__ == "__main__":
    main()
