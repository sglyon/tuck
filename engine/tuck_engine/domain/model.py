from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
import math
from typing import Any
from urllib.parse import urlparse
from uuid import uuid4


class DomainError(ValueError):
    code = "invalid_operation"


class ConflictError(DomainError):
    code = "conflict"


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def text(value: Any, label: str, maximum: int, *, empty: bool = False) -> str:
    if not isinstance(value, str):
        raise DomainError(f"{label} must be text.")
    if len(value) > maximum or (not empty and not value.strip()):
        raise DomainError(
            f"{label} must contain {'0' if empty else '1'}–{maximum:,} characters."
        )
    return value


def seconds(value: Any) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
        or value < 0
    ):
        raise DomainError("Time must be a finite, nonnegative number.")
    return float(value)


@dataclass
class Meeting:
    id: str
    title: str
    created_at: str = field(default_factory=now)
    duration: float = 0
    status: str = "draft"
    notes: str = ""
    summary: str = ""
    template: str = "general"
    starred: bool = False
    is_example: bool = False
    notion_url: str = ""
    notion_status: str = "idle"
    version: int = 0

    @classmethod
    def create(
        cls, title: str = "Untitled meeting", *, example: bool = False
    ) -> Meeting:
        return cls(
            id=str(uuid4()), title=text(title, "Title", 240).strip(), is_example=example
        )

    def update(self, changes: dict) -> None:
        allowed = {"title", "notes", "template", "starred"}
        if not isinstance(changes, dict) or set(changes) - allowed:
            raise DomainError("Only title, notes, template, and starred can be edited.")
        checked = {}
        for key, value in changes.items():
            if key == "starred":
                if not isinstance(value, bool):
                    raise DomainError("Starred must be true or false.")
                checked[key] = value
            else:
                checked[key] = text(
                    value,
                    key.title(),
                    200_000 if key == "notes" else 240,
                    empty=key == "notes",
                )
        for key, value in checked.items():
            setattr(self, key, value)

    def start(self) -> None:
        if self.is_example:
            raise DomainError(
                "Create a new meeting to record. The example is read-only for recording."
            )
        if self.status != "draft":
            raise DomainError("Recording can only start in a new meeting.")
        self.status = "recording"

    def accept_segment(self, segment: TranscriptSegment) -> None:
        if segment.meeting_id != self.id:
            raise DomainError(
                "The transcription result belongs to a different meeting."
            )
        if self.status not in {"recording", "paused", "processing"}:
            raise DomainError("This meeting is no longer accepting transcription.")

    def capture_failed(self, has_evidence: bool) -> None:
        if self.status == "recording":
            self.status = "interrupted" if has_evidence else "draft"

    def pause(self) -> None:
        if self.status != "recording":
            raise DomainError("Only an active recording can be paused.")
        self.status = "paused"

    def resume(self) -> None:
        if self.status != "paused":
            raise DomainError("Only a paused recording can be resumed.")
        self.status = "recording"

    def process(self) -> None:
        if self.status not in {"recording", "paused", "draft"}:
            raise DomainError("This meeting is not awaiting transcription.")
        self.status = "processing"

    def complete(self, duration: float) -> None:
        if self.status != "processing":
            raise DomainError("Complete transcription before finishing this meeting.")
        self.duration = seconds(duration)
        self.status = "complete"

    def interrupt(self) -> None:
        if self.status in {"recording", "paused", "processing"}:
            self.status = "interrupted"
        if self.notion_status == "syncing":
            self.notion_status = "uncertain"

    def may_delete(self) -> None:
        if self.status in {"recording", "paused", "processing"}:
            raise DomainError("Stop this recording before deleting it.")

    def replace_summary(self, summary: str) -> None:
        self.summary = text(summary, "Summary", 200_000)

    def begin_publication(self) -> None:
        if self.is_example:
            raise DomainError("Example meetings cannot be published.")
        if self.status in {"recording", "paused", "processing"}:
            raise DomainError("Finish the meeting before sending notes to Notion.")
        if self.notion_status in {"syncing", "uncertain"}:
            raise DomainError(
                "Notion may have received these notes. Check Notion before attempting another upload."
            )
        if self.notion_url:
            raise DomainError(
                "These notes have already been published. Open the existing page in Notion."
            )
        if not (self.summary.strip() or self.notes.strip()):
            raise DomainError("Add notes or generate a summary before publishing.")
        self.notion_status = "syncing"

    def publication_failed(self, uncertain: bool = False) -> None:
        self.notion_status = "uncertain" if uncertain else "error"

    def publication_finished(self, url: str) -> None:
        parsed = urlparse(url)
        if parsed.scheme != "https" or not (
            parsed.hostname == "notion.so"
            or (parsed.hostname or "").endswith(".notion.so")
            or (parsed.hostname or "").endswith(".notion.site")
        ):
            raise DomainError("Notion returned an unexpected page address.")
        self.notion_url = url
        self.notion_status = "synced"

    def dto(self) -> dict:
        return {
            "id": self.id,
            "title": self.title,
            "createdAt": self.created_at,
            "duration": self.duration,
            "status": self.status,
            "notes": self.notes,
            "summary": self.summary,
            "template": self.template,
            "starred": self.starred,
            "isExample": self.is_example,
            "notionUrl": self.notion_url,
            "notionStatus": self.notion_status,
        }

    @classmethod
    def restore(cls, data: dict, version: int) -> Meeting:
        return cls(
            id=data["id"],
            title=data["title"],
            created_at=data["createdAt"],
            duration=data["duration"],
            status=data["status"],
            notes=data["notes"],
            summary=data["summary"],
            template=data["template"],
            starred=data["starred"],
            is_example=data["isExample"],
            notion_url=data["notionUrl"],
            notion_status=data["notionStatus"],
            version=version,
        )


@dataclass(frozen=True)
class TranscriptSegment:
    id: str
    meeting_id: str
    start: float
    end: float
    text: str
    speaker_id: str
    source: str

    @classmethod
    def from_dto(cls, data: dict) -> TranscriptSegment:
        start, end = seconds(data["start"]), seconds(data["end"])
        if end < start:
            raise DomainError("A transcript segment cannot end before it begins.")
        if data["source"] not in {"microphone", "system", "import"}:
            raise DomainError("Unknown audio source.")
        return cls(
            text(data["id"], "Segment ID", 128),
            text(data["meetingId"], "Meeting ID", 128),
            start,
            end,
            text(data["text"], "Transcript", 32_000),
            text(data["speakerId"], "Speaker ID", 128),
            data["source"],
        )

    def dto(self) -> dict:
        return {
            "id": self.id,
            "meetingId": self.meeting_id,
            "start": self.start,
            "end": self.end,
            "text": self.text,
            "speakerId": self.speaker_id,
            "source": self.source,
        }


DEFAULT_SETTINGS = {
    "provider": "local",
    "model": "qwen3:4b",
    "localUrl": "http://127.0.0.1:11434",
    "hostedConsent": False,
    "webSearch": False,
    "autoSummary": False,
    "notionAutoSync": False,
    "notionTarget": "",
    "threads": 2,
    "keepAudio": True,
}


def updated_settings(current: dict, changes: dict) -> dict:
    if not isinstance(changes, dict) or set(changes) - set(DEFAULT_SETTINGS):
        raise DomainError("Unrecognized setting.")
    result = {**DEFAULT_SETTINGS, **current, **changes}
    if result["provider"] not in {"local", "chatgpt", "openai"}:
        raise DomainError("Choose Local, ChatGPT, or OpenAI for AI processing.")
    for name in (
        "hostedConsent",
        "webSearch",
        "autoSummary",
        "notionAutoSync",
        "keepAudio",
    ):
        if not isinstance(result[name], bool):
            raise DomainError(f"{name} must be true or false.")
    if (
        isinstance(result["threads"], bool)
        or not isinstance(result["threads"], int)
        or not 1 <= result["threads"] <= 8
    ):
        raise DomainError("Choose between one and eight inference threads.")
    text(result["model"], "Model", 200, empty=True)
    text(result["notionTarget"], "Notion destination", 2000, empty=True)
    endpoint = urlparse(text(result["localUrl"], "Local AI address", 2000))
    if (
        endpoint.scheme != "http"
        or endpoint.hostname not in {"127.0.0.1", "localhost", "::1"}
        or endpoint.username
        or endpoint.password
    ):
        raise DomainError("Local AI must use an HTTP loopback address on this device.")
    return result
