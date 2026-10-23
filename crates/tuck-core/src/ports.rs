//! Outbound interfaces owned by the application. All methods may block.
use crate::domain::{AppError, Meeting, TranscriptSegment};
use serde_json::Value;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, AppError>;
pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;
pub type OnSegment = Arc<dyn Fn(Value) -> Result<()> + Send + Sync>;

pub trait RecordingPort: Send + Sync {
    fn status(&self) -> Value;
    /// Apply selected device IDs before capture begins. Empty IDs use system defaults.
    fn configure(&self, _settings: &Value) -> Result<()> {
        Ok(())
    }
    fn start(
        &self,
        meeting_id: &str,
        on_segment: OnSegment,
        system_audio: bool,
        microphone: bool,
        threads: u32,
        keep_audio: bool,
    ) -> Result<()>;
    fn pause(&self) -> Result<()>;
    fn resume(&self) -> Result<()>;
    /// Must drain segment callbacks before returning.
    fn stop(&self) -> Result<f64>;
    fn shutdown(&self) -> Result<()>;
    fn install_model(&self, kind: &str) -> Result<Value>;
    fn unload(&self) -> Result<()>;
    fn import_file(
        &self,
        path: &str,
        meeting_id: &str,
        on_segment: OnSegment,
        threads: u32,
    ) -> Result<f64>;
}
pub trait IntelligencePort: Send + Sync {
    fn models(&self, settings: &Value) -> Result<Vec<Value>>;
    fn complete(
        &self,
        settings: &Value,
        system: &str,
        messages: &[Value],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String>;
}
pub trait PublicationPort: Send + Sync {
    fn parents(&self, _query: &str) -> Result<Value> {
        Err(AppError::unavailable(
            "This publisher cannot find shared parent pages.",
        ))
    }
    /// With allow_create=false this must only reconcile an existing template.
    /// An ambiguous creation returns notion_setup_uncertain; it must never be retried blindly.
    fn provision(&self, _parent: &str, _allow_create: bool) -> Result<Value> {
        Err(AppError::unavailable(
            "This publisher cannot create a meeting database.",
        ))
    }
    /// Read-only verification of a page copied by the user after an uncertain
    /// initial publication. Implementations must check destination and ownership.
    fn recover(&self, _target: &str, _meeting: &Value, _url: &str) -> Result<String> {
        Err(AppError::unavailable(
            "This publisher does not support recovering an existing page.",
        ))
    }
    fn destinations(&self, _query: &str) -> Result<Value> {
        Err(AppError::unavailable(
            "This publisher does not support destination discovery.",
        ))
    }
    fn destination(&self, _target: &str) -> Result<Value> {
        Err(AppError::unavailable(
            "This publisher does not support destination inspection.",
        ))
    }
    fn publish_with_settings(
        &self,
        target: &str,
        meeting: &Value,
        _settings: &Value,
    ) -> Result<String> {
        self.publish(target, meeting)
    }

    /// Return AppError with code `publish_uncertain` if creation may have reached the server.
    fn publish(&self, target: &str, meeting: &Value) -> Result<String>;
}
pub trait SecretPort: Send + Sync {
    fn get(&self, kind: &str) -> Result<Option<String>>;
    fn set(&self, kind: &str, value: &str) -> Result<()>;
    fn delete(&self, kind: &str) -> Result<()>;
}
pub trait AccountPort: Send + Sync {
    /// Local metadata only: bootstrap must never refresh credentials or contact a server.
    fn accounts(&self) -> Result<Vec<Value>>;
    fn sign_in(&self, account_id: Option<&str>) -> Result<Vec<Value>>;
    fn select(&self, account_id: &str) -> Result<Vec<Value>>;
    fn sign_out(&self, account_id: &str) -> Result<Value>;
}
pub trait AudioArchivePort: Send + Sync {
    fn list(&self, _meeting_id: &str) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }
    fn resolve(&self, _meeting_id: &str, _audio_id: &str) -> Result<String> {
        Err(AppError::unavailable("Retained audio is unavailable."))
    }
    fn write_backup(&self, _path: &str, _manifest: &Value, _include_audio: bool) -> Result<Value> {
        Err(AppError::unavailable("Library backup is unavailable."))
    }
    fn read_backup(&self, _path: &str) -> Result<Value> {
        Err(AppError::unavailable("Library restore is unavailable."))
    }
    fn commit_restore_audio(&self, _token: &str, _id_map: &Value) -> Result<Value> {
        Ok(serde_json::json!({"audioFilesRestored":0,"warnings":[]}))
    }
    fn discard_restore(&self, _token: &str) -> Result<()> {
        Ok(())
    }

    fn remove(&self, meeting_id: &str) -> Result<()>;
    fn export(&self, path: &str, content: &str) -> Result<()>;
}

/// A short, application-owned transaction. Repository operations never commit themselves.
pub trait Repository {
    fn get(&mut self, id: &str) -> Result<Meeting>;
    fn list(&mut self) -> Result<Vec<Meeting>>;
    /// Only records whose active/publication state requires crash recovery.
    fn recoverable(&mut self) -> Result<Vec<Meeting>>;
    fn meeting_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value>;
    fn library_overview(&mut self) -> Result<Value>;
    fn task_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value>;
    fn save(&mut self, meeting: &mut Meeting) -> Result<()>;
    fn delete(&mut self, meeting: &Meeting) -> Result<()>;
    fn append(&mut self, segment: &TranscriptSegment) -> Result<bool>;
    fn segments(&mut self, id: &str) -> Result<Vec<Value>>;
    fn raw_segments(&mut self, id: &str) -> Result<Vec<Value>>;
    fn corrections(&mut self, id: &str) -> Result<Vec<Value>>;
    /// None removes the correction; raw transcript evidence remains immutable.
    fn save_correction(
        &mut self,
        id: &str,
        segment_id: &str,
        correction: Option<&Value>,
    ) -> Result<()>;

    fn speakers(&mut self, id: &str) -> Result<Vec<Value>>;
    fn rename_speaker(&mut self, id: &str, speaker: &str, name: &str) -> Result<()>;
    fn save_speaker(&mut self, id: &str, speaker: &Value) -> Result<()>;
    fn messages(&mut self, id: &str) -> Result<Vec<Value>>;
    fn add_message(&mut self, id: &str, message: &Value) -> Result<()>;
    fn bookmarks(&mut self, id: &str) -> Result<Vec<Value>>;
    fn add_bookmark(&mut self, id: &str, bookmark: &Value) -> Result<()>;
    fn remove_bookmark(&mut self, id: &str, bookmark: &str) -> Result<()>;
    fn add_revision(&mut self, id: &str, revision: &Value) -> Result<()>;
    fn revisions(&mut self, id: &str) -> Result<Vec<Value>>;
    fn all_revisions(&mut self, id: &str) -> Result<Vec<Value>>;
    /// Metadata only, in durable insertion order; never load revision bodies for autosave.
    fn latest_revision_info(&mut self, id: &str, target: &str) -> Result<Option<Value>>;
    /// Return only bounded lexical evidence; never materialize the whole library transcript.
    fn search(&mut self, query: &crate::domain::SearchQuery) -> Result<Vec<Value>>;
    /// Lookup only; the application owns atomic preparation and creation.
    fn calendar_meeting(&mut self, event: &crate::domain::CalendarEvent)
        -> Result<Option<Meeting>>;
    /// Summary lookup for bounded calendar lists; must not hydrate notes/evidence.
    fn calendar_meeting_id(
        &mut self,
        event: &crate::domain::CalendarEvent,
    ) -> Result<Option<String>>;
    fn calendar_snapshots(&mut self) -> Result<Vec<crate::meeting_day::CalendarSnapshot>>;
    fn save_calendar_snapshot(
        &mut self,
        snapshot: &crate::meeting_day::CalendarSnapshot,
    ) -> Result<()>;
    fn reminder_state(&mut self, key: &str) -> Result<Option<crate::meeting_day::ReminderState>>;
    fn save_reminder_state(&mut self, state: &crate::meeting_day::ReminderState) -> Result<()>;
    fn prune_meeting_day(&mut self, now: &str) -> Result<()>;
    fn settings(&mut self) -> Result<Value>;
    fn save_settings(&mut self, settings: &Value) -> Result<()>;
}
pub trait Store: Send + Sync {
    /// Invoke once; commit iff the callback returns Ok. No external I/O inside this callback.
    fn transaction(
        &self,
        action: &mut dyn FnMut(&mut dyn Repository) -> Result<Value>,
    ) -> Result<Value>;
}
