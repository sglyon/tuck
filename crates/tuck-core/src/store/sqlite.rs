//! SQLite adapter with additive migrations; original meeting/evidence document layout is retained.
use crate::{
    domain::{default_settings, AppError, Meeting, TranscriptSegment},
    ports::{Repository, Result, Store},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex, time::Duration};
mod library;
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meetings (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, document TEXT NOT NULL, version INTEGER NOT NULL CHECK(version > 0));
CREATE INDEX IF NOT EXISTS meetings_timeline ON meetings(created_at DESC,id DESC);
CREATE INDEX IF NOT EXISTS meetings_recovery_status ON meetings(json_extract(document,'$.status'));
CREATE INDEX IF NOT EXISTS meetings_recovery_publication ON meetings(json_extract(document,'$.notionStatus'));
CREATE TABLE IF NOT EXISTS library_generation (id INTEGER PRIMARY KEY CHECK(id=1),generation INTEGER NOT NULL);
INSERT OR IGNORE INTO library_generation VALUES(1,0);
CREATE TABLE IF NOT EXISTS segments (meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,id TEXT NOT NULL,start REAL NOT NULL,document TEXT NOT NULL,PRIMARY KEY(meeting_id,id));
CREATE INDEX IF NOT EXISTS segments_timeline ON segments(meeting_id,start);
CREATE TABLE IF NOT EXISTS speakers (meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,id TEXT NOT NULL,name TEXT NOT NULL,color INTEGER NOT NULL,PRIMARY KEY(meeting_id,id));
CREATE TABLE IF NOT EXISTS messages (meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,id TEXT NOT NULL,created_at TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(meeting_id,id));
CREATE TABLE IF NOT EXISTS bookmarks (meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,id TEXT NOT NULL,time REAL NOT NULL,document TEXT NOT NULL,PRIMARY KEY(meeting_id,id));
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1),document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS note_revisions (meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,id TEXT NOT NULL,created_at TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(meeting_id,id));
CREATE INDEX IF NOT EXISTS revisions_meeting ON note_revisions(meeting_id,created_at DESC);
CREATE INDEX IF NOT EXISTS revisions_target ON note_revisions(meeting_id,json_extract(document,'$.target'));
CREATE TABLE IF NOT EXISTS transcript_corrections (meeting_id TEXT NOT NULL,segment_id TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(meeting_id,segment_id),FOREIGN KEY(meeting_id,segment_id) REFERENCES segments(meeting_id,id) ON DELETE CASCADE);
CREATE INDEX IF NOT EXISTS meetings_calendar_occurrence ON meetings(json_extract(document,'$.calendarEvent.source'),json_extract(document,'$.calendarEvent.id'));
CREATE TABLE IF NOT EXISTS calendar_snapshots (scope TEXT PRIMARY KEY CHECK(scope IN ('device','import')),expires_at TEXT NOT NULL,document TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meeting_reminders (occurrence_key TEXT PRIMARY KEY,expires_at TEXT NOT NULL,document TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS meeting_reminders_expiry ON meeting_reminders(expires_at);
PRAGMA user_version=4;
"#;
fn storage(_: rusqlite::Error) -> AppError {
    AppError::new("storage_error", "Tuck could not access its local library.")
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| {
        AppError::new(
            "storage_error",
            "The local library contains an unreadable record.",
        )
    })
}
pub struct SqliteStore {
    db: Mutex<Connection>,
}
impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| {
                AppError::new("storage_error", "Tuck could not create its local library.")
            })?;
        }
        let db = Connection::open(path).map_err(storage)?;
        let store = Self::initialize(db)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
                |_| AppError::new("storage_error", "Tuck could not protect its local library."),
            )?;
        }
        Ok(store)
    }
    pub fn in_memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory().map_err(storage)?)
    }
    fn initialize(db: Connection) -> Result<Self> {
        db.busy_timeout(Duration::from_secs(10)).map_err(storage)?;
        db.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(storage)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage)?;
        if version > 4 {
            return Err(AppError::invalid(
                "This library was created by a newer Tuck version. Update Tuck to open it.",
            ));
        }
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        let mut triggers = String::new();
        for table in [
            "meetings",
            "segments",
            "transcript_corrections",
            "speakers",
            "messages",
            "bookmarks",
        ] {
            for action in ["INSERT", "UPDATE", "DELETE"] {
                triggers.push_str(&format!("CREATE TRIGGER IF NOT EXISTS library_{table}_{action} AFTER {action} ON {table} BEGIN UPDATE library_generation SET generation=generation+1 WHERE id=1; END;"));
            }
        }
        db.execute_batch(&format!("BEGIN IMMEDIATE;{SCHEMA}{triggers}COMMIT;"))
            .map_err(storage)?;
        Ok(Self { db: Mutex::new(db) })
    }
}
impl Store for SqliteStore {
    fn transaction(
        &self,
        action: &mut dyn FnMut(&mut dyn Repository) -> Result<Value>,
    ) -> Result<Value> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| AppError::new("storage_error", "The local library is unavailable."))?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let result = action(&mut SqlRepository { db: &tx })?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
struct SqlRepository<'a> {
    db: &'a Connection,
}
impl SqlRepository<'_> {
    fn documents(&self, sql: &str, id: &str) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare(sql).map_err(storage)?;
        let rows = statement
            .query_map([id], |r| r.get::<_, String>(0))
            .map_err(storage)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(decode(&row.map_err(storage)?)?);
        }
        Ok(result)
    }
    fn restore(document: String, version: i64) -> Result<Meeting> {
        let mut meeting: Meeting = decode(&document)?;
        meeting.version = version;
        Ok(meeting)
    }
}
impl Repository for SqlRepository<'_> {
    fn get(&mut self, id: &str) -> Result<Meeting> {
        let row: Option<(String, i64)> = self
            .db
            .query_row(
                "SELECT document,version FROM meetings WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        let (document, version) =
            row.ok_or_else(|| AppError::invalid("This meeting no longer exists."))?;
        Self::restore(document, version)
    }
    fn list(&mut self) -> Result<Vec<Meeting>> {
        let mut statement = self
            .db
            .prepare("SELECT document,version FROM meetings ORDER BY created_at DESC,id DESC")
            .map_err(storage)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(storage)?;
        let mut result = Vec::new();
        for row in rows {
            let (d, v) = row.map_err(storage)?;
            result.push(Self::restore(d, v)?);
        }
        Ok(result)
    }
    fn recoverable(&mut self) -> Result<Vec<Meeting>> {
        self.recoverable_meetings()
    }
    fn meeting_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value> {
        self.page_meetings(query)
    }
    fn library_overview(&mut self) -> Result<Value> {
        self.overview()
    }
    fn task_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value> {
        self.page_tasks(query)
    }
    fn save(&mut self, m: &mut Meeting) -> Result<()> {
        let document = m.dto().to_string();
        if m.version == 0 {
            let n=self.db.execute("INSERT OR IGNORE INTO meetings(id,created_at,document,version) VALUES(?,?,?,1)",params![m.id,m.created_at,document]).map_err(storage)?;
            if n != 1 {
                return Err(AppError::conflict());
            }
        } else {
            let n = self
                .db
                .execute(
                    "UPDATE meetings SET document=?,version=version+1 WHERE id=? AND version=?",
                    params![document, m.id, m.version],
                )
                .map_err(storage)?;
            if n != 1 {
                return Err(AppError::conflict());
            }
        }
        m.version += 1;
        Ok(())
    }
    fn delete(&mut self, m: &Meeting) -> Result<()> {
        if self
            .db
            .execute(
                "DELETE FROM meetings WHERE id=? AND version=?",
                params![m.id, m.version],
            )
            .map_err(storage)?
            != 1
        {
            return Err(AppError::conflict());
        }
        Ok(())
    }
    fn append(&mut self, s: &TranscriptSegment) -> Result<bool> {
        let n = self
            .db
            .execute(
                "INSERT OR IGNORE INTO segments(meeting_id,id,start,document) VALUES(?,?,?,?)",
                params![s.meeting_id, s.id, s.start, s.dto().to_string()],
            )
            .map_err(storage)?;
        if n == 0 {
            return Ok(false);
        }
        let count: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM speakers WHERE meeting_id=?",
                [&s.meeting_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let name = if s.speaker_id == "you" {
            "You".into()
        } else if s.speaker_id == "unassigned" {
            "Unassigned".into()
        } else {
            format!(
                "Speaker {}",
                s.speaker_id
                    .strip_prefix("speaker-")
                    .unwrap_or(&s.speaker_id)
            )
        };
        self.db
            .execute(
                "INSERT OR IGNORE INTO speakers(meeting_id,id,name,color) VALUES(?,?,?,?)",
                params![s.meeting_id, s.speaker_id, name, count % 6],
            )
            .map_err(storage)?;
        Ok(true)
    }
    fn raw_segments(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents(
            "SELECT document FROM segments WHERE meeting_id=? ORDER BY start,id",
            id,
        )
    }
    fn segments(&mut self, id: &str) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare("SELECT s.document,c.document FROM segments s LEFT JOIN transcript_corrections c ON c.meeting_id=s.meeting_id AND c.segment_id=s.id WHERE s.meeting_id=? ORDER BY s.start,s.id").map_err(storage)?;
        let rows = statement
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .map_err(storage)?;
        let mut result = Vec::new();
        for row in rows {
            let (raw, correction) = row.map_err(storage)?;
            let c: Option<Value> = correction.map(|s| decode(&s)).transpose()?;
            result.push(crate::domain::corrected_segment(decode(&raw)?, c.as_ref()));
        }
        Ok(result)
    }
    fn recent_segments(&mut self, id: &str, limit: usize) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare("SELECT s.document,c.document FROM segments s LEFT JOIN transcript_corrections c ON c.meeting_id=s.meeting_id AND c.segment_id=s.id WHERE s.meeting_id=? ORDER BY s.start DESC,s.id DESC LIMIT ?").map_err(storage)?;
        let rows = statement
            .query_map(params![id, limit.min(64) as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .map_err(storage)?;
        let mut result = Vec::new();
        for row in rows {
            let (raw, correction) = row.map_err(storage)?;
            let c: Option<Value> = correction.map(|s| decode(&s)).transpose()?;
            result.push(crate::domain::corrected_segment(decode(&raw)?, c.as_ref()));
        }
        result.reverse();
        Ok(result)
    }
    fn corrections(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents(
            "SELECT document FROM transcript_corrections WHERE meeting_id=? ORDER BY segment_id",
            id,
        )
    }
    fn save_correction(
        &mut self,
        id: &str,
        segment_id: &str,
        correction: Option<&Value>,
    ) -> Result<()> {
        let exists: bool = self
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM segments WHERE meeting_id=? AND id=?)",
                params![id, segment_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !exists {
            return Err(AppError::invalid(
                "This transcript segment no longer exists.",
            ));
        }
        if let Some(c) = correction {
            self.db.execute("INSERT INTO transcript_corrections(meeting_id,segment_id,document) VALUES(?,?,?) ON CONFLICT(meeting_id,segment_id) DO UPDATE SET document=excluded.document",params![id,segment_id,c.to_string()]).map_err(storage)?;
        } else {
            self.db
                .execute(
                    "DELETE FROM transcript_corrections WHERE meeting_id=? AND segment_id=?",
                    params![id, segment_id],
                )
                .map_err(storage)?;
        }
        Ok(())
    }
    fn speakers(&mut self, id: &str) -> Result<Vec<Value>> {
        let mut statement = self
            .db
            .prepare("SELECT id,name,color FROM speakers WHERE meeting_id=? ORDER BY rowid")
            .map_err(storage)?;
        let rows=statement.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"color":r.get::<_,i64>(2)?}))).map_err(storage)?;
        rows.map(|r| r.map_err(storage)).collect()
    }
    fn rename_speaker(&mut self, id: &str, speaker: &str, name: &str) -> Result<()> {
        if self
            .db
            .execute(
                "UPDATE speakers SET name=? WHERE meeting_id=? AND id=?",
                params![name, id, speaker],
            )
            .map_err(storage)?
            != 1
        {
            return Err(AppError::invalid(
                "This speaker is not part of the meeting.",
            ));
        }
        Ok(())
    }
    fn rename_remembered_speaker(&mut self,speaker:&str,name:&str)->Result<()> {
        self.db.execute("UPDATE speakers SET name=? WHERE id=?",params![name,speaker]).map_err(storage)?;Ok(())
    }
    fn save_speaker(&mut self, id: &str, speaker: &Value) -> Result<()> {
        self.db.execute("INSERT INTO speakers(meeting_id,id,name,color) VALUES(?,?,?,?) ON CONFLICT(meeting_id,id) DO UPDATE SET name=excluded.name,color=excluded.color",params![id,speaker["id"].as_str(),speaker["name"].as_str(),speaker["color"].as_i64()]).map_err(storage)?;
        Ok(())
    }
    fn messages(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents(
            "SELECT document FROM messages WHERE meeting_id=? ORDER BY created_at,rowid",
            id,
        )
    }
    fn add_message(&mut self, id: &str, m: &Value) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO messages(meeting_id,id,created_at,document) VALUES(?,?,?,?)",
                params![id, m["id"].as_str(), m["createdAt"].as_str(), m.to_string()],
            )
            .map_err(storage)?;
        Ok(())
    }
    fn bookmarks(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents(
            "SELECT document FROM bookmarks WHERE meeting_id=? ORDER BY time,rowid",
            id,
        )
    }
    fn add_bookmark(&mut self, id: &str, b: &Value) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO bookmarks(meeting_id,id,time,document) VALUES(?,?,?,?)",
                params![id, b["id"].as_str(), b["time"].as_f64(), b.to_string()],
            )
            .map_err(storage)?;
        Ok(())
    }
    fn remove_bookmark(&mut self, id: &str, b: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM bookmarks WHERE meeting_id=? AND id=?",
                params![id, b],
            )
            .map_err(storage)?;
        Ok(())
    }
    fn add_revision(&mut self, id: &str, revision: &Value) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO note_revisions(meeting_id,id,created_at,document) VALUES(?,?,?,?)",
                params![
                    id,
                    revision["id"].as_str(),
                    revision["createdAt"].as_str(),
                    revision.to_string()
                ],
            )
            .map_err(storage)?;
        Ok(())
    }
    fn revisions(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents("SELECT document FROM note_revisions WHERE meeting_id=? ORDER BY created_at DESC,rowid DESC LIMIT 100", id)
    }
    fn latest_revision_info(&mut self, id: &str, target: &str) -> Result<Option<Value>> {
        self.db.query_row("SELECT created_at,coalesce(json_extract(document,'$.kind'),'replacement') FROM note_revisions WHERE meeting_id=? AND json_extract(document,'$.target')=? ORDER BY rowid DESC LIMIT 1",params![id,target],|r| Ok(json!({"createdAt":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?}))).optional().map_err(storage)
    }
    fn all_revisions(&mut self, id: &str) -> Result<Vec<Value>> {
        self.documents(
            "SELECT document FROM note_revisions WHERE meeting_id=? ORDER BY created_at,rowid",
            id,
        )
    }
    fn search(&mut self, query: &crate::domain::SearchQuery) -> Result<Vec<Value>> {
        // SQLite scans lexical evidence in-process and returns a bounded ranked set.
        // Parameterized terms cannot become SQL, LIKE wildcards, or FTS operators.
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        let mut bind = |s: &str| {
            values.push(s.to_owned().into());
            format!("?{}", values.len())
        };
        let score = query
            .terms
            .iter()
            .map(|t| format!("(instr(lower(body),{})>0)", bind(t)))
            .collect::<Vec<_>>()
            .join("+");
        let mut filters = Vec::new();
        if let Some(folder) = &query.folder {
            filters.push(format!(
                "coalesce(json_extract(m.document,'$.folder'),'')={}",
                bind(folder)
            ));
        }
        for (key, names) in [("tags", &query.tags), ("people", &query.people)] {
            for name in names {
                filters.push(format!(
                    "EXISTS(SELECT 1 FROM json_each(m.document,'$.{key}') WHERE value={})",
                    bind(name)
                ));
            }
        }
        if !query.meeting_ids.is_empty() {
            filters.push(format!(
                "m.id IN ({})",
                query
                    .meeting_ids
                    .iter()
                    .map(|id| bind(id))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        let filter = if filters.is_empty() {
            "1".into()
        } else {
            filters.join(" AND ")
        };
        let sql = format!("WITH evidence AS (
            SELECT id AS meeting_id,NULL AS segment_id,NULL AS start,'title' AS source,json_extract(document,'$.title') AS body,0 AS edited FROM meetings
            UNION ALL SELECT id,NULL,NULL,'notes',json_extract(document,'$.notes'),0 FROM meetings
            UNION ALL SELECT id,NULL,NULL,'summary',json_extract(document,'$.summary'),0 FROM meetings
            UNION ALL SELECT s.meeting_id,s.id,s.start,'transcript',coalesce(json_extract(c.document,'$.text'),json_extract(s.document,'$.text')),c.segment_id IS NOT NULL FROM segments s LEFT JOIN transcript_corrections c ON c.meeting_id=s.meeting_id AND c.segment_id=s.id
        ), ranked AS (SELECT e.*,json_extract(m.document,'$.title') AS title,({score}) AS score FROM evidence e JOIN meetings m ON m.id=e.meeting_id WHERE {filter})
        SELECT meeting_id,title,segment_id,start,source,body,edited FROM ranked WHERE score>0 ORDER BY score DESC,meeting_id,segment_id LIMIT {}",query.limit);
        let mut statement = self.db.prepare(&sql).map_err(storage)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, bool>(6)?,
                ))
            })
            .map_err(storage)?;
        let mut hits = Vec::new();
        for row in rows {
            let (id, title, segment, start, source, body, edited) = row.map_err(storage)?;
            let mut hit = query.hit(&id, &title, segment.as_deref(), start, &source, &body);
            if edited {
                hit["edited"] = json!(true);
                hit["text"] = json!(format!(
                    "[User-corrected transcript] {}",
                    hit["text"].as_str().unwrap_or("")
                )
                .chars()
                .take(1200)
                .collect::<String>());
            }
            hits.push(hit);
        }
        Ok(hits)
    }
    fn calendar_meeting(
        &mut self,
        event: &crate::domain::CalendarEvent,
    ) -> Result<Option<Meeting>> {
        let key = crate::meeting_day::occurrence_key(event)?;
        let mut statement=self.db.prepare("SELECT document,version FROM meetings WHERE json_extract(document,'$.calendarEvent.source')=? AND json_extract(document,'$.calendarEvent.id')=? ORDER BY created_at,id").map_err(storage)?;
        let rows = statement
            .query_map(params![event.source, event.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(storage)?;
        for row in rows {
            let (doc, version) = row.map_err(storage)?;
            let m = Self::restore(doc, version)?;
            if m.calendar_event
                .as_ref()
                .is_some_and(|e| crate::meeting_day::occurrence_key(e).is_ok_and(|k| k == key))
            {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }
    fn calendar_meeting_id(
        &mut self,
        event: &crate::domain::CalendarEvent,
    ) -> Result<Option<String>> {
        let start = crate::meeting_day::instant(&event.start)?;
        let mut statement=self.db.prepare("SELECT id,json_extract(document,'$.calendarEvent.start') FROM meetings WHERE json_extract(document,'$.calendarEvent.source')=? AND json_extract(document,'$.calendarEvent.id')=? ORDER BY created_at,id").map_err(storage)?;
        let rows = statement
            .query_map(params![event.source, event.id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        for row in rows {
            let (id, saved_start) = row.map_err(storage)?;
            if crate::meeting_day::instant(&saved_start).is_ok_and(|v| v == start) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
    fn calendar_snapshots(&mut self) -> Result<Vec<crate::meeting_day::CalendarSnapshot>> {
        let mut s = self
            .db
            .prepare("SELECT document FROM calendar_snapshots ORDER BY scope")
            .map_err(storage)?;
        let rows = s
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage)?;
        rows.map(|r| decode(&r.map_err(storage)?)).collect()
    }
    fn save_calendar_snapshot(
        &mut self,
        snapshot: &crate::meeting_day::CalendarSnapshot,
    ) -> Result<()> {
        self.db.execute("INSERT INTO calendar_snapshots VALUES(?,?,?) ON CONFLICT(scope) DO UPDATE SET expires_at=excluded.expires_at,document=excluded.document",params![snapshot.scope,crate::meeting_day::normalized(&snapshot.expires_at)?,json!(snapshot).to_string()]).map_err(storage)?;
        Ok(())
    }
    fn reminder_state(&mut self, key: &str) -> Result<Option<crate::meeting_day::ReminderState>> {
        self.db
            .query_row(
                "SELECT document FROM meeting_reminders WHERE occurrence_key=?",
                [key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(storage)?
            .map(|s| decode(&s))
            .transpose()
    }
    fn save_reminder_state(&mut self, state: &crate::meeting_day::ReminderState) -> Result<()> {
        self.db.execute("INSERT INTO meeting_reminders VALUES(?,?,?) ON CONFLICT(occurrence_key) DO UPDATE SET expires_at=excluded.expires_at,document=excluded.document",params![state.occurrence_key,crate::meeting_day::normalized(&state.expires_at)?,json!(state).to_string()]).map_err(storage)?;
        Ok(())
    }
    fn prune_meeting_day(&mut self, now: &str) -> Result<()> {
        let now = crate::meeting_day::normalized(now)?;
        for mut snapshot in self.calendar_snapshots()? {
            if !snapshot.live(crate::meeting_day::instant(&now)?)? && !snapshot.events.is_empty() {
                snapshot.events.clear();
                self.save_calendar_snapshot(&snapshot)?;
            }
        }
        self.db
            .execute(
                "DELETE FROM meeting_reminders WHERE julianday(expires_at)<=julianday(?)",
                [&now],
            )
            .map_err(storage)?;
        Ok(())
    }
    fn settings(&mut self) -> Result<Value> {
        let row: Option<String> = self
            .db
            .query_row("SELECT document FROM settings WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(storage)?;
        let mut settings = default_settings();
        if let Some(row) = row {
            let value: Value = decode(&row)?;
            if let Some(obj) = value.as_object() {
                for (k, v) in obj {
                    if settings.get(k).is_some() || k == "_notionSetup" {
                        settings[k] = v.clone();
                    }
                }
            }
        }
        Ok(settings)
    }
    fn save_settings(&mut self, s: &Value) -> Result<()> {
        self.db.execute("INSERT INTO settings(id,document) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET document=excluded.document",[s.to_string()]).map_err(storage)?;
        Ok(())
    }
}
