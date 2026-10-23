//! Corrections and portable library operations protect evidence and current work.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct BackupMeeting {
    meeting: Meeting,
    segments: Vec<Value>,
    #[serde(default)]
    corrections: Vec<Value>,
    speakers: Vec<Value>,
    messages: Vec<Value>,
    bookmarks: Vec<Value>,
    #[serde(default)]
    revisions: Vec<Value>,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(rename = "createdAt")]
    created_at: String,
    settings: Value,
    meetings: Vec<BackupMeeting>,
}
fn invalid_backup() -> AppError {
    AppError::invalid("This backup contains invalid or unsupported library data.")
}
fn timestamp_valid(value: &Value) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(domain::text(value, "Timestamp", 128, false)?)
        .map_err(|_| invalid_backup())?;
    Ok(())
}
fn record_id(value: &Value) -> Result<&str> {
    domain::text(value, "ID", 128, false)
}

fn validate_records(
    records: &[Value],
    mut validate: impl FnMut(&Value) -> Result<()>,
) -> Result<()> {
    let mut ids = HashSet::new();
    for record in records {
        if !ids.insert(record_id(&record["id"])?) {
            return Err(invalid_backup());
        }
        validate(record)?;
    }
    Ok(())
}
fn portable_settings(settings: &Value) -> Result<Value> {
    let defaults = domain::default_settings();
    let mut changes = json!({});
    for key in defaults.as_object().expect("settings object").keys() {
        if let Some(value) = settings.get(key) {
            changes[key] = value.clone();
        }
    }
    for (key, value) in [
        ("provider", json!("local")),
        ("hostedConsent", json!(false)),
        ("webSearch", json!(false)),
        ("autoSummary", json!(false)),
        ("notionAutoSync", json!(false)),
        ("meetingReminders", json!(false)),
        ("notionTarget", json!("")),
        ("notionPropertyMapping", json!({})),
        ("microphoneDeviceId", json!("")),
        ("systemDeviceId", json!("")),
    ] {
        changes[key] = value;
    }
    domain::updated_settings(&defaults, &changes)
}
fn validate_manifest(value: &Value) -> Result<Manifest> {
    let mut manifest: Manifest =
        serde_json::from_value(value.clone()).map_err(|_| invalid_backup())?;
    if manifest.format != "tuck-library"
        || manifest.version != 1
        || manifest.meetings.len() > 10_000
    {
        return Err(invalid_backup());
    }
    timestamp_valid(&json!(manifest.created_at))?;
    manifest.settings = portable_settings(&manifest.settings)?;
    let mut meetings = HashSet::new();
    for entry in &manifest.meetings {
        let m = &entry.meeting;
        record_id(&json!(m.id))?;
        if !meetings.insert(&m.id)
            || !matches!(
                m.status.as_str(),
                "draft" | "recording" | "paused" | "processing" | "complete" | "interrupted"
            )
        {
            return Err(invalid_backup());
        }
        timestamp_valid(&json!(m.created_at))?;
        domain::seconds(&json!(m.duration))?;
        let mut checked = m.clone();
        checked.update(&json!({"title":m.title,"notes":m.notes,"summary":m.summary,"template":m.template,"folder":m.folder,"tags":m.tags,"people":m.people,"calendarEvent":m.calendar_event}))?;
        if m.tasks.len() > 1000 {
            return Err(invalid_backup());
        }
        let mut tasks = HashSet::new();
        for task in &m.tasks {
            record_id(&json!(task.id))?;
            if !tasks.insert(&task.id) {
                return Err(invalid_backup());
            }
            timestamp_valid(&json!(task.created_at))?;
            task.clone().update(&json!({"title":task.title,"owner":task.owner,"dueDate":task.due_date,"done":task.done}))?;
        }
        if let Some(source) = &m.recovered_from {
            record_id(&json!(source))?;
        }
        validate_records(&entry.segments, |v| {
            let s = TranscriptSegment::from_dto(v.clone())?;
            if s.meeting_id != m.id {
                return Err(invalid_backup());
            }
            Ok(())
        })?;
        let segment_ids: HashSet<_> = entry
            .segments
            .iter()
            .filter_map(|s| s["id"].as_str())
            .collect();
        let mut corrected = HashSet::new();
        for c in &entry.corrections {
            let segment = domain::text(&c["segmentId"], "Segment ID", 128, false)?;
            if !segment_ids.contains(segment) || !corrected.insert(segment) {
                return Err(invalid_backup());
            }
            domain::text(&c["text"], "Correction", 32_000, false)?;
            timestamp_valid(&c["updatedAt"])?;
        }
        validate_records(&entry.speakers, |s| {
            domain::text(&s["name"], "Speaker name", 120, false)?;
            if !s["color"].as_u64().is_some_and(|c| c < 6) {
                return Err(invalid_backup());
            }
            Ok(())
        })?;
        validate_records(&entry.messages, |v| {
            if !matches!(v["role"].as_str(), Some("user" | "assistant")) {
                return Err(invalid_backup());
            }
            domain::text(&v["content"], "Message", 200_000, false)?;
            timestamp_valid(&v["createdAt"])
        })?;
        validate_records(&entry.bookmarks, |v| {
            domain::seconds(&v["time"])?;
            domain::text(&v["label"], "Bookmark", 240, false)?;
            Ok(())
        })?;
        validate_records(&entry.revisions, |v| {
            if v.get("kind")
                .is_some_and(|kind| !matches!(kind.as_str(), Some("manual" | "replacement")))
            {
                return Err(invalid_backup());
            }
            if !matches!(v["target"].as_str(), Some("notes" | "summary")) {
                return Err(invalid_backup());
            }
            domain::text(&v["text"], "Revision", 200_000, true)?;
            timestamp_valid(&v["createdAt"])
        })?;
    }
    Ok(manifest)
}
impl MeetingService {
    pub(super) fn correct_transcript(&self, p: &Value, reset: bool) -> Result<Value> {
        let id = string(p, "meetingId")?;
        let segment = string(p, "segmentId")?;
        let base = domain::text(&p["baseText"], "Original text", 32_000, false)?;
        let text = if reset {
            None
        } else {
            Some(domain::text(
                &p["text"],
                "Corrected transcript",
                32_000,
                false,
            )?)
        };
        self.transaction(|r| {
            let m = r.get(id)?;
            if m.active() {
                return Err(AppError::invalid(
                    "Finish recording before correcting the transcript.",
                ));
            }
            let current = r
                .segments(id)?
                .into_iter()
                .find(|s| s["id"] == segment)
                .ok_or_else(|| AppError::invalid("This transcript segment no longer exists."))?;
            if current["text"] != base {
                return Err(AppError::conflict());
            }
            let original = current.get("originalText").unwrap_or(&current["text"]);
            let correction = text
                .filter(|text| original != *text)
                .map(|text| json!({"segmentId":segment,"text":text,"updatedAt":domain::now()}));
            if correction.is_some() || current["edited"] == true {
                r.save_correction(id, segment, correction.as_ref())?;
            }
            Ok(Value::Null)
        })?;
        self.changed(id)
    }
    fn audio_meeting(&self, id: &str) -> Result<Meeting> {
        let mut found = None;
        self.transaction(|r| {
            let m = r.get(id)?;
            if m.active() {
                return Err(AppError::invalid(
                    "Finish recording before opening retained audio.",
                ));
            }
            found = Some(m);
            Ok(Value::Null)
        })?;
        found.ok_or_else(|| AppError::invalid("This meeting no longer exists."))
    }
    pub(super) fn meeting_audio(&self, p: &Value, resolve: bool) -> Result<Value> {
        let _life = lock(&self.lifecycle)?;
        let id = string(p, "meetingId")?;
        self.audio_meeting(id)?;
        if resolve {
            Ok(json!({"path":self.adapters.archive.resolve(id,string(p,"audioId")?)?}))
        } else {
            Ok(json!(self.adapters.archive.list(id)?))
        }
    }
    pub(super) fn recover_audio(self: &Arc<Self>, p: &Value) -> Result<Value> {
        let id = string(p, "meetingId")?;
        let _op = self.operation(id, "recovery")?;
        let original = self.audio_meeting(id)?;
        if original.is_example {
            return Err(AppError::invalid(
                "Example meetings have no retained audio to recover.",
            ));
        }
        let path = self.adapters.archive.resolve(id, string(p, "audioId")?)?;
        self.import_audio_with_source(
            &path,
            &truncate(&format!("Recovered · {}", original.title), 240),
            Some(&original),
        )
    }
    pub(super) fn backup_library(&self, p: &Value) -> Result<Value> {
        let _life = lock(&self.lifecycle)?;
        self.assert_idle()?;
        let path = string(p, "path")?;
        let include_audio = boolean(p, "includeAudio", true)?;
        let manifest = self.transaction(|r| {
            let mut meetings = Vec::new();
            for m in r.list()? {
                let id = &m.id;
                meetings.push(BackupMeeting {
                    segments: r.raw_segments(id)?,
                    corrections: r.corrections(id)?,
                    speakers: r.speakers(id)?,
                    messages: r.messages(id)?,
                    bookmarks: r.bookmarks(id)?,
                    revisions: r.all_revisions(id)?,
                    meeting: m,
                });
            }
            let manifest = Manifest {
                format: "tuck-library".into(),
                version: 1,
                created_at: domain::now(),
                settings: portable_settings(&r.settings()?)?,
                meetings,
            };
            serde_json::to_value(manifest).map_err(|_| invalid_backup())
        })?;
        let mut report = self
            .adapters
            .archive
            .write_backup(path, &manifest, include_audio)?;
        if !report.is_object() {
            return Err(AppError::new(
                "archive_error",
                "The backup adapter returned an invalid report.",
            ));
        }
        report["path"] = json!(path);
        report["meetingCount"] = json!(manifest["meetings"].as_array().map_or(0, Vec::len));
        report["audioIncluded"] = json!(include_audio);
        if !report["warnings"].is_array() {
            report["warnings"] = json!([]);
        }
        Ok(report)
    }
    pub(super) fn restore_library(&self, p: &Value, preview: bool) -> Result<Value> {
        let _life = lock(&self.lifecycle)?;
        self.assert_idle()?;
        let _op = self.operation("library", "restore")?;
        let staged = self.adapters.archive.read_backup(string(p, "path")?)?;
        let token =
            domain::text(&staged["stagingToken"], "Restore staging token", 128, false)?.to_owned();
        let outcome = self.restore_staged(p, &staged, preview);
        let cleanup = self.adapters.archive.discard_restore(&token);
        match outcome {
            Ok(mut report) => {
                if let Err(error) = cleanup {
                    report["warnings"]
                        .as_array_mut()
                        .expect("warnings")
                        .push(json!(format!(
                            "Temporary restore files could not be removed: {}",
                            error.message
                        )));
                }
                Ok(report)
            }
            Err(error) => Err(error),
        }
    }
    fn restore_staged(&self, p: &Value, staged: &Value, preview: bool) -> Result<Value> {
        let fingerprint = domain::text(&staged["fingerprint"], "Backup fingerprint", 64, false)?;
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid_backup());
        }
        if !preview
            && domain::text(&p["expectedFingerprint"], "Preview fingerprint", 64, false)?
                != fingerprint
        {
            return Err(AppError::new(
                "conflict",
                "This backup changed after preview. Preview it again before restoring.",
            ));
        }
        let manifest = validate_manifest(&staged["manifest"])?;
        let mut warnings = staged["warnings"].as_array().cloned().unwrap_or_default();
        let mut imported = Vec::new();
        let mut skipped = 0;
        let mut settings_mode = "saved-writing-merged";
        self.transaction(|r| {
            let existing:HashSet<_>=r.list()?.into_iter().map(|m|m.id).collect();
            if existing.is_empty() { settings_mode="safe-preferences"; }
            let candidates:Vec<_>=manifest.meetings.iter().filter(|e|!existing.contains(&e.meeting.id)).collect();
            skipped=manifest.meetings.len()-candidates.len();
            imported=candidates.iter().map(|e|e.meeting.id.clone()).collect();
            if preview { return Ok(Value::Null); }
            for entry in candidates {
                let mut m=entry.meeting.clone(); m.version=0; m.interrupt(); m.notion_url.clear(); m.notion_status="idle".into();
                r.save(&mut m)?;
                for s in &entry.segments { r.append(&TranscriptSegment::from_dto(s.clone())?)?; }
                if m.is_dictation() && m.notes.is_empty() {
                    Self::seed_dictation_notes(r, &mut m)?;
                    if !m.notes.is_empty() { r.save(&mut m)?; }
                }
                for c in &entry.corrections { r.save_correction(&m.id,c["segmentId"].as_str().expect("validated correction"),Some(c))?; }
                for s in &entry.speakers { r.save_speaker(&m.id,s)?; }
                for v in &entry.messages { r.add_message(&m.id,v)?; }
                for v in &entry.bookmarks { r.add_bookmark(&m.id,v)?; }
                for v in &entry.revisions { r.add_revision(&m.id,v)?; }
            }
            let mut settings=if existing.is_empty() { manifest.settings.clone() } else { r.settings()? };
            if !existing.is_empty() {
                for field in ["customTemplates","recipes"] {
                    let mut items=settings[field].as_array().cloned().unwrap_or_default();
                    for item in manifest.settings[field].as_array().into_iter().flatten() {
                        if !items.iter().any(|i|i["id"]==item["id"]) {
                            if items.len()<100 { items.push(item.clone()); } else { warnings.push(json!(format!("Some {field} could not be merged because this library already has 100 saved items."))); break; }
                        }
                    }
                    settings[field]=json!(items);
                }
            }
            r.save_settings(&settings)?;
            Ok(Value::Null)
        })?;
        if preview {
            return Ok(
                json!({"meetingCount":manifest.meetings.len(),"restorable":imported.len(),"skipped":skipped,"audioFiles":staged["audioCount"].as_u64().unwrap_or(0),"warnings":warnings,"settingsMode":settings_mode,"fingerprint":fingerprint}),
            );
        }
        let map: serde_json::Map<String, Value> =
            imported.iter().map(|id| (id.clone(), json!(id))).collect();
        let audio = self.adapters.archive.commit_restore_audio(
            staged["stagingToken"].as_str().expect("validated token"),
            &Value::Object(map),
        );
        let mut audio_files = 0;
        match audio {
            Ok(report) => {
                audio_files = report["audioFilesRestored"].as_u64().unwrap_or(0);
                if let Some(w) = report["warnings"].as_array() {
                    warnings.extend(w.iter().cloned());
                }
            }
            Err(error) => warnings.push(json!(format!(
                "Your meeting data was restored, but some retained audio could not be restored: {}",
                error.message
            ))),
        }
        for id in &imported {
            if let Err(error) = self.changed(id) {
                warnings.push(json!(format!(
                    "The restored meeting is saved, but its view could not refresh: {}",
                    error.message
                )));
            }
        }
        Ok(
            json!({"restored":imported.len(),"skipped":skipped,"audioFilesRestored":audio_files,"warnings":warnings,"settingsMode":settings_mode}),
        )
    }
}
