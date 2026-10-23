"""SQLite implementation of core-owned ports. Transaction ownership stays with use cases."""

from __future__ import annotations
import json
from pathlib import Path
import shutil
import sqlite3
from typing import Callable

from tuck_engine.domain.model import (
    Meeting,
    TranscriptSegment,
    ConflictError,
    DomainError,
    DEFAULT_SETTINGS,
)


SCHEMA = """
CREATE TABLE IF NOT EXISTS meetings (
 id TEXT PRIMARY KEY, created_at TEXT NOT NULL, document TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version > 0)
);
CREATE TABLE IF NOT EXISTS segments (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 id TEXT NOT NULL, start REAL NOT NULL, document TEXT NOT NULL,
 PRIMARY KEY(meeting_id,id)
);
CREATE INDEX IF NOT EXISTS segments_timeline ON segments(meeting_id,start);
CREATE TABLE IF NOT EXISTS speakers (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 id TEXT NOT NULL, name TEXT NOT NULL, color INTEGER NOT NULL,
 PRIMARY KEY(meeting_id,id)
);
CREATE TABLE IF NOT EXISTS messages (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 id TEXT NOT NULL, created_at TEXT NOT NULL, document TEXT NOT NULL,
 PRIMARY KEY(meeting_id,id)
);
CREATE TABLE IF NOT EXISTS bookmarks (
 meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
 id TEXT NOT NULL, time REAL NOT NULL, document TEXT NOT NULL,
 PRIMARY KEY(meeting_id,id)
);
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), document TEXT NOT NULL);
PRAGMA user_version=1;
"""


def _connect(path: Path) -> sqlite3.Connection:
    db = sqlite3.connect(path, timeout=10)
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA foreign_keys=ON")
    db.execute("PRAGMA busy_timeout=10000")
    return db


class SqliteMeetingRepository:
    def __init__(self, db: sqlite3.Connection):
        self.db = db

    def get(self, meeting_id: str) -> Meeting:
        row = self.db.execute(
            "SELECT document,version FROM meetings WHERE id=?", (meeting_id,)
        ).fetchone()
        if not row:
            raise DomainError("This meeting no longer exists.")
        return Meeting.restore(json.loads(row["document"]), row["version"])

    def list(self) -> list[Meeting]:
        rows = self.db.execute(
            "SELECT document,version FROM meetings ORDER BY created_at DESC"
        ).fetchall()
        return [Meeting.restore(json.loads(r["document"]), r["version"]) for r in rows]

    def save(self, meeting: Meeting) -> None:
        document = json.dumps(meeting.dto(), ensure_ascii=False)
        if meeting.version == 0:
            try:
                self.db.execute(
                    "INSERT INTO meetings(id,created_at,document,version) VALUES(?,?,?,1)",
                    (meeting.id, meeting.created_at, document),
                )
            except sqlite3.IntegrityError as exc:
                raise ConflictError("This meeting was already created.") from exc
        else:
            cursor = self.db.execute(
                "UPDATE meetings SET document=?,version=version+1 WHERE id=? AND version=?",
                (document, meeting.id, meeting.version),
            )
            if cursor.rowcount != 1:
                raise ConflictError("This meeting changed. Reload it and try again.")
        meeting.version += 1

    def delete(self, meeting: Meeting) -> None:
        cursor = self.db.execute(
            "DELETE FROM meetings WHERE id=? AND version=?",
            (meeting.id, meeting.version),
        )
        if cursor.rowcount != 1:
            raise ConflictError("This meeting changed before it could be deleted.")


class SqliteEvidenceRepository:
    def __init__(self, db: sqlite3.Connection):
        self.db = db

    def append(self, segment: TranscriptSegment) -> bool:
        if self.db.execute(
            "SELECT 1 FROM segments WHERE meeting_id=? AND id=?",
            (segment.meeting_id, segment.id),
        ).fetchone():
            return False
        self.db.execute(
            "INSERT INTO segments(meeting_id,id,start,document) VALUES(?,?,?,?)",
            (
                segment.meeting_id,
                segment.id,
                segment.start,
                json.dumps(segment.dto(), ensure_ascii=False),
            ),
        )
        speaker_count = self.db.execute(
            "SELECT COUNT(*) FROM speakers WHERE meeting_id=?", (segment.meeting_id,)
        ).fetchone()[0]
        name = (
            "You"
            if segment.speaker_id == "you"
            else "Speaker " + segment.speaker_id.removeprefix("speaker-")
        )
        self.db.execute(
            "INSERT OR IGNORE INTO speakers(meeting_id,id,name,color) VALUES(?,?,?,?)",
            (segment.meeting_id, segment.speaker_id, name, speaker_count % 6),
        )
        return True

    def segments(self, meeting_id: str) -> list[dict]:
        return self._documents("segments", meeting_id, "start,id")

    def speakers(self, meeting_id: str) -> list[dict]:
        return [
            dict(row)
            for row in self.db.execute(
                "SELECT id,name,color FROM speakers WHERE meeting_id=? ORDER BY rowid",
                (meeting_id,),
            )
        ]

    def rename_speaker(self, meeting_id: str, speaker_id: str, name: str) -> None:
        cursor = self.db.execute(
            "UPDATE speakers SET name=? WHERE meeting_id=? AND id=?",
            (name, meeting_id, speaker_id),
        )
        if cursor.rowcount != 1:
            raise DomainError("This speaker is not part of the meeting.")

    def messages(self, meeting_id: str) -> list[dict]:
        return self._documents("messages", meeting_id, "created_at,rowid")

    def add_message(self, meeting_id: str, message: dict) -> None:
        self.db.execute(
            "INSERT INTO messages(meeting_id,id,created_at,document) VALUES(?,?,?,?)",
            (
                meeting_id,
                message["id"],
                message["createdAt"],
                json.dumps(message, ensure_ascii=False),
            ),
        )

    def bookmarks(self, meeting_id: str) -> list[dict]:
        return self._documents("bookmarks", meeting_id, "time,rowid")

    def add_bookmark(self, meeting_id: str, bookmark: dict) -> None:
        self.db.execute(
            "INSERT INTO bookmarks(meeting_id,id,time,document) VALUES(?,?,?,?)",
            (
                meeting_id,
                bookmark["id"],
                bookmark["time"],
                json.dumps(bookmark, ensure_ascii=False),
            ),
        )

    def remove_bookmark(self, meeting_id: str, bookmark_id: str) -> None:
        self.db.execute(
            "DELETE FROM bookmarks WHERE meeting_id=? AND id=?",
            (meeting_id, bookmark_id),
        )

    def _documents(self, table: str, meeting_id: str, ordering: str) -> list[dict]:
        # Table and ordering are private constants, never caller input.
        return [
            json.loads(row[0])
            for row in self.db.execute(
                f"SELECT document FROM {table} WHERE meeting_id=? ORDER BY {ordering}",
                (meeting_id,),
            )
        ]


class SqliteSettingsRepository:
    def __init__(self, db: sqlite3.Connection):
        self.db = db

    def get(self) -> dict:
        row = self.db.execute("SELECT document FROM settings WHERE id=1").fetchone()
        return {**DEFAULT_SETTINGS, **(json.loads(row[0]) if row else {})}

    def save(self, values: dict) -> None:
        self.db.execute(
            "INSERT INTO settings(id,document) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET document=excluded.document",
            (json.dumps(values),),
        )


class SqliteUnitOfWork:
    def __init__(self, path: Path):
        self.path = path

    def __enter__(self):
        self.db = _connect(self.path)
        self.db.execute("BEGIN IMMEDIATE")
        self.meetings = SqliteMeetingRepository(self.db)
        self.evidence = SqliteEvidenceRepository(self.db)
        self.settings = SqliteSettingsRepository(self.db)
        return self

    def commit(self) -> None:
        self.db.commit()

    def __exit__(self, *args) -> None:
        self.db.rollback()
        self.db.close()


def sqlite_factory(data_dir: Path) -> Callable[[], SqliteUnitOfWork]:
    data_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = data_dir / "tuck.sqlite3"
    db = _connect(path)
    version = db.execute("PRAGMA user_version").fetchone()[0]
    if version > 1:
        db.close()
        raise DomainError(
            "This library was created by a newer Tuck version. Update Tuck to open it."
        )
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript(SCHEMA)
    db.close()
    try:
        path.chmod(0o600)
    except OSError:
        pass
    return lambda: SqliteUnitOfWork(path)


class LocalAudioArchive:
    def __init__(self, data_dir: Path):
        self.root = (data_dir / "audio").resolve()

    def remove(self, meeting_id: str) -> None:
        from uuid import UUID

        try:
            UUID(meeting_id)
        except ValueError as exc:
            raise DomainError("Invalid meeting identifier.") from exc
        path = self.root / meeting_id
        if path.is_symlink():
            path.unlink()
        elif path.is_dir():
            shutil.rmtree(path)

    def export(self, path: str, content: str) -> None:
        target = Path(path)
        if (
            not target.is_absolute()
            or target.suffix.lower() not in {".md", ".json", ".srt"}
            or target.is_symlink()
        ):
            raise DomainError("Choose a Markdown, JSON, or SRT file using Save As.")
        target.write_text(content, encoding="utf-8")
