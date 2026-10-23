use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppError {
    pub code: String,
    pub message: String,
}
impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_operation", message)
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("unavailable", message)
    }
    pub fn conflict() -> Self {
        Self::new("conflict", "This meeting changed. Reload it and try again.")
    }
}
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AppError {}
pub type Result<T> = std::result::Result<T, AppError>;
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn id() -> String {
    Uuid::new_v4().to_string()
}
pub fn text<'a>(v: &'a Value, label: &str, max: usize, empty: bool) -> Result<&'a str> {
    let s = v
        .as_str()
        .ok_or_else(|| AppError::invalid(format!("{label} must be text.")))?;
    if s.chars().count() > max || (!empty && s.trim().is_empty()) {
        return Err(AppError::invalid(format!(
            "{label} must contain {}–{max} characters.",
            if empty { 0 } else { 1 }
        )));
    }
    Ok(s)
}
pub fn seconds(v: &Value) -> Result<f64> {
    v.as_f64()
        .filter(|x| x.is_finite() && *x >= 0.)
        .ok_or_else(|| AppError::invalid("Time must be a finite, nonnegative number."))
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MeetingKind {
    #[default]
    Meeting,
    Dictation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub kind: MeetingKind,
    pub created_at: String,
    pub duration: f64,
    pub status: String,
    pub notes: String,
    pub summary: String,
    pub template: String,
    pub starred: bool,
    pub is_example: bool,
    pub notion_url: String,
    pub notion_status: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub people: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<ActionItem>,
    #[serde(default)]
    pub calendar_event: Option<CalendarEvent>,
    #[serde(default)]
    pub recovered_from: Option<String>,
    #[serde(skip)]
    pub version: i64,
}
impl Meeting {
    pub fn create(title: &str) -> Result<Self> {
        text(&json!(title), "Title", 240, false)?;
        Ok(Self {
            id: id(),
            title: title.trim().into(),
            kind: MeetingKind::Meeting,
            created_at: now(),
            duration: 0.,
            status: "draft".into(),
            notes: String::new(),
            summary: String::new(),
            template: "general".into(),
            starred: false,
            is_example: false,
            notion_url: String::new(),
            notion_status: "idle".into(),
            folder: String::new(),
            tags: Vec::new(),
            people: Vec::new(),
            tasks: Vec::new(),
            calendar_event: None,
            recovered_from: None,
            version: 0,
        })
    }
    pub fn create_dictation(title: &str) -> Result<Self> {
        let mut meeting = Self::create(title)?;
        meeting.kind = MeetingKind::Dictation;
        Ok(meeting)
    }
    pub fn is_dictation(&self) -> bool {
        self.kind == MeetingKind::Dictation
    }
    /// Seed the editable voice note once; raw segment evidence remains untouched.
    pub fn seed_dictation_notes(&mut self, speech: &str) {
        if self.is_dictation() && self.notes.is_empty() {
            // Notes have the same bounded length as manual edits. Evidence is retained in full.
            self.notes = speech.chars().take(200_000).collect();
        }
    }
    pub fn dto(&self) -> Value {
        serde_json::to_value(self).expect("meeting DTO")
    }
    pub fn update(&mut self, changes: &Value) -> Result<()> {
        let obj = changes
            .as_object()
            .ok_or_else(|| AppError::invalid("Changes must be an object."))?;
        let mut next = self.clone();
        for (key, value) in obj {
            match key.as_str() {
                "title" => next.title = text(value, "Title", 240, false)?.into(),
                "notes" => next.notes = text(value, "Notes", 200_000, true)?.into(),
                "summary" => next.summary = text(value, "Summary", 200_000, true)?.into(),
                "folder" => next.folder = text(value, "Folder", 240, true)?.trim().into(),
                "tags" => next.tags = text_list(value, "Tags", 50)?,
                "people" => next.people = text_list(value, "People", 100)?,
                "calendarEvent" => next.calendar_event = if value.is_null() { None } else { Some(CalendarEvent::from_value(value)?) },
                "template" => next.template = text(value, "Template", 240, false)?.into(),
                "starred" => {
                    next.starred = value
                        .as_bool()
                        .ok_or_else(|| AppError::invalid("Starred must be true or false."))?
                }
                _ => {
                    return Err(AppError::invalid(
                        "Only title, notes, summary, template, starred, folder, tags, people, and calendarEvent can be edited.",
                    ))
                }
            }
        }
        *self = next;
        Ok(())
    }
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "recording" | "paused" | "processing")
    }
    pub fn start(&mut self) -> Result<()> {
        if self.is_example {
            return Err(AppError::invalid(
                "Create a new meeting to record. The example is read-only for recording.",
            ));
        }
        self.transition(
            &["draft"],
            "recording",
            "Recording can only start in a new meeting.",
        )
    }
    pub fn pause(&mut self) -> Result<()> {
        self.transition(
            &["recording"],
            "paused",
            "Only an active recording can be paused.",
        )
    }
    pub fn resume(&mut self) -> Result<()> {
        self.transition(
            &["paused"],
            "recording",
            "Only a paused recording can be resumed.",
        )
    }
    pub fn process(&mut self) -> Result<()> {
        self.transition(
            &["recording", "paused", "draft"],
            "processing",
            "This meeting is not awaiting transcription.",
        )
    }
    fn transition(&mut self, from: &[&str], to: &str, message: &str) -> Result<()> {
        if !from.contains(&self.status.as_str()) {
            return Err(AppError::invalid(message));
        }
        self.status = to.into();
        Ok(())
    }
    pub fn complete(&mut self, duration: f64) -> Result<()> {
        let duration = seconds(&json!(duration))?;
        self.transition(
            &["processing"],
            "complete",
            "Complete transcription before finishing this meeting.",
        )?;
        self.duration = duration;
        Ok(())
    }
    pub fn capture_failed(&mut self, evidence: bool) {
        if self.status == "recording" {
            self.status = if evidence { "interrupted" } else { "draft" }.into();
        }
    }
    pub fn interrupt(&mut self) {
        if self.active() {
            self.status = "interrupted".into();
        }
        if self.notion_status == "syncing" {
            self.notion_status = "uncertain".into();
        }
    }
    pub fn accept(&self, s: &TranscriptSegment) -> Result<()> {
        if s.meeting_id != self.id {
            return Err(AppError::invalid(
                "The transcription result belongs to a different meeting.",
            ));
        }
        if !self.active() {
            return Err(AppError::invalid(
                "This meeting is no longer accepting transcription.",
            ));
        }
        Ok(())
    }
    pub fn may_delete(&self) -> Result<()> {
        if self.active() {
            return Err(AppError::invalid("Stop this recording before deleting it."));
        }
        Ok(())
    }
    pub fn replace_summary(&mut self, summary: &str) -> Result<()> {
        text(&json!(summary), "Summary", 200_000, false)?;
        self.summary = summary.into();
        Ok(())
    }
    pub fn begin_publication(&mut self) -> Result<()> {
        self.begin_publication_mode(false)
    }
    pub fn begin_sync_publication(&mut self) -> Result<()> {
        self.begin_publication_mode(true)
    }
    fn begin_publication_mode(&mut self, updating: bool) -> Result<()> {
        if updating && self.notion_url.is_empty() {
            return Err(AppError::invalid(
                "Publish this meeting before syncing an existing Notion page.",
            ));
        }
        if self.is_example {
            return Err(AppError::invalid("Example meetings cannot be published."));
        }
        if self.active() {
            return Err(AppError::invalid(
                "Finish the meeting before sending notes to Notion.",
            ));
        }
        if self.notion_status == "syncing" || (self.notion_status == "uncertain" && !updating) {
            return Err(AppError::invalid("Notion may have received these notes. Check Notion before attempting another upload."));
        }
        if !updating && !self.notion_url.is_empty() {
            return Err(AppError::invalid(
                "These notes have already been published. Open the existing page in Notion.",
            ));
        }
        if self.summary.trim().is_empty() && self.notes.trim().is_empty() {
            return Err(AppError::invalid(
                "Add notes or generate a summary before publishing.",
            ));
        }
        self.notion_status = "syncing".into();
        Ok(())
    }
    pub fn publication_finished(&mut self, address: &str) -> Result<()> {
        let url = url::Url::parse(address)
            .map_err(|_| AppError::invalid("Notion returned an unexpected page address."))?;
        let host = url.host_str().unwrap_or("");
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || !(host == "notion.so"
                || host == "app.notion.com"
                || host.ends_with(".notion.so")
                || host.ends_with(".notion.site"))
        {
            return Err(AppError::invalid(
                "Notion returned an unexpected page address.",
            ));
        }
        self.notion_url = address.into();
        self.notion_status = "synced".into();
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub meeting_id: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub speaker_id: String,
    pub source: String,
}
impl TranscriptSegment {
    pub fn from_dto(v: Value) -> Result<Self> {
        let start = seconds(&v["start"])?;
        let end = seconds(&v["end"])?;
        if end < start {
            return Err(AppError::invalid(
                "A transcript segment cannot end before it begins.",
            ));
        }
        if !matches!(
            v["source"].as_str(),
            Some("microphone" | "system" | "import")
        ) {
            return Err(AppError::invalid("Unknown audio source."));
        }
        for (k, label, max) in [
            ("id", "Segment ID", 128),
            ("meetingId", "Meeting ID", 128),
            ("speakerId", "Speaker ID", 128),
            ("text", "Transcript", 32_000),
        ] {
            text(&v[k], label, max, false)?;
        }
        serde_json::from_value(v).map_err(|_| AppError::invalid("Invalid transcript segment."))
    }
    pub fn dto(&self) -> Value {
        serde_json::to_value(self).expect("segment DTO")
    }
}
pub fn default_settings() -> Value {
    json!({"provider":"local","model":"qwen3:4b","localUrl":"http://127.0.0.1:11434","hostedConsent":false,"webSearch":false,"autoSummary":false,"notionAutoSync":false,"notionTarget":"","notionPropertyMapping":{},"notionContent":{"recap":true,"notes":true,"tasks":true},"threads":2,"meetingReminders":false,"keepAudio":true,"customTemplates":[],"recipes":[
        {"id":"851ac7aa-aea5-4ed0-a750-838224c1850b","name":"Decisions and next steps","prompt":"What decisions were made and which next steps were agreed? Cite evidence. Include owners and dates only when stated; separate suggestions from commitments."},
        {"id":"35ad4a91-65dc-47b8-814d-efc48d709a5c","name":"Follow-up draft","prompt":"Draft a concise follow-up message I can review. Cover the main outcome, agreed actions and unresolved questions. Preserve uncertainty and attach evidence citations. Do not send it."},
        {"id":"9051642a-114c-48c6-aeb6-847b7be6549a","name":"Risks and open questions","prompt":"Identify stated risks, blockers, dependencies and unanswered questions. Cite evidence and clearly label any inference. Do not invent risks unsupported by this meeting."}
    ],"microphoneDeviceId":"","systemDeviceId":""})
}
pub fn updated_settings(current: &Value, changes: &Value) -> Result<Value> {
    let defaults = default_settings();
    let change = changes
        .as_object()
        .ok_or_else(|| AppError::invalid("Unrecognized setting."))?;
    if change.keys().any(|k| defaults.get(k).is_none()) {
        return Err(AppError::invalid("Unrecognized setting."));
    }
    let mut result = defaults;
    for source in [current, changes] {
        if let Some(obj) = source.as_object() {
            for (k, v) in obj {
                if result.get(k).is_some() {
                    result[k] = v.clone();
                }
            }
        }
    }
    if !matches!(
        result["provider"].as_str(),
        Some("local" | "chatgpt" | "openai")
    ) {
        return Err(AppError::invalid(
            "Choose Local, ChatGPT, or OpenAI for AI processing.",
        ));
    }
    for name in [
        "hostedConsent",
        "webSearch",
        "autoSummary",
        "notionAutoSync",
        "keepAudio",
        "meetingReminders",
    ] {
        if !result[name].is_boolean() {
            return Err(AppError::invalid(format!("{name} must be true or false.")));
        }
    }
    let content = notion_content(&result)?;
    if result["notionAutoSync"] == true && !content.iter().any(|selected| *selected) {
        return Err(AppError::invalid(
            "Choose at least one item to publish before enabling automatic Notion publishing.",
        ));
    }
    if !result["threads"]
        .as_u64()
        .is_some_and(|n| (1..=8).contains(&n))
    {
        return Err(AppError::invalid(
            "Choose between one and eight inference threads.",
        ));
    }
    for key in ["microphoneDeviceId", "systemDeviceId"] {
        text(&result[key], key, 4096, true)?;
    }
    validate_saved_items(&result["customTemplates"], "instructions")?;
    validate_saved_items(&result["recipes"], "prompt")?;
    text(&result["model"], "Model", 200, true)?;
    text(&result["notionTarget"], "Notion destination", 2000, true)?;
    let mapping = result["notionPropertyMapping"]
        .as_object()
        .ok_or_else(|| AppError::invalid("Notion property mapping must be an object."))?;
    for (key, value) in mapping {
        if !matches!(
            key.as_str(),
            "date" | "folder" | "tags" | "people" | "meetingId"
        ) {
            return Err(AppError::invalid("Unrecognized Notion property mapping."));
        }
        text(value, "Notion property", 240, true)?;
    }

    let endpoint = url::Url::parse(text(&result["localUrl"], "Local AI address", 2000, false)?)
        .map_err(|_| {
            AppError::invalid("Local AI must use an HTTP loopback address on this device.")
        })?;
    if endpoint.scheme() != "http"
        || !matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
        )
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err(AppError::invalid(
            "Local AI must use an HTTP loopback address on this device.",
        ));
    }
    Ok(result)
}

/// Selection is explicit; older saved preferences retain the previous three-part export.
pub fn notion_content(settings: &Value) -> Result<[bool; 3]> {
    let Some(value) = settings.get("notionContent") else {
        return Ok([true; 3]);
    };
    let fields = value
        .as_object()
        .ok_or_else(|| AppError::invalid("Choose the content to publish to Notion."))?;
    if fields.len() != 3
        || fields
            .keys()
            .any(|key| !matches!(key.as_str(), "recap" | "notes" | "tasks"))
    {
        return Err(AppError::invalid(
            "Choose recap, personal notes and action items for Notion publishing.",
        ));
    }
    let mut selected = [false; 3];
    for (index, key) in ["recap", "notes", "tasks"].iter().enumerate() {
        selected[index] = fields[*key]
            .as_bool()
            .ok_or_else(|| AppError::invalid("Notion content choices must be true or false."))?;
    }
    Ok(selected)
}

/// Only publication metadata and the user's selected text cross the publishing port.
/// Speech evidence, chat and audio never belong to the publication payload.
pub fn publication_snapshot(meeting: &Value, settings: &Value) -> Result<Value> {
    let [recap, notes, tasks] = notion_content(settings)?;
    if ![recap, notes, tasks].iter().any(|selected| *selected) {
        return Err(AppError::invalid(
            "Choose at least one item to publish in Notion settings.",
        ));
    }
    let mut snapshot = json!({});
    for field in [
        "id",
        "title",
        "createdAt",
        "folder",
        "tags",
        "people",
        "notionUrl",
        "notionStatus",
    ] {
        if let Some(value) = meeting.get(field) {
            snapshot[field] = value.clone();
        }
    }
    snapshot["summary"] = if recap {
        meeting.get("summary").cloned().unwrap_or(json!(""))
    } else {
        json!("")
    };
    snapshot["notes"] = if notes {
        meeting.get("notes").cloned().unwrap_or(json!(""))
    } else {
        json!("")
    };
    snapshot["tasks"] = if tasks {
        meeting.get("tasks").cloned().unwrap_or(json!([]))
    } else {
        json!([])
    };
    Ok(snapshot)
}

pub fn text_list(value: &Value, label: &str, maximum: usize) -> Result<Vec<String>> {
    let values = value
        .as_array()
        .filter(|v| v.len() <= maximum)
        .ok_or_else(|| {
            AppError::invalid(format!(
                "{label} must be a list of at most {maximum} names."
            ))
        })?;
    let mut result = Vec::new();
    for value in values {
        let item = text(value, label, 240, false)?.trim().to_owned();
        if !result.contains(&item) {
            result.push(item);
        }
    }
    Ok(result)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    #[serde(default)]
    pub all_day: bool,
    pub id: String,
    pub source: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub join_url: String,
    pub attendees: Vec<String>,
}
impl CalendarEvent {
    pub fn from_value(value: &Value) -> Result<Self> {
        let event: Self = serde_json::from_value(value.clone())
            .map_err(|_| AppError::invalid("Invalid calendar event."))?;
        for key in ["id", "source", "title"] {
            text(&value[key], key, 4096, false)?;
        }
        let start = chrono::DateTime::parse_from_rfc3339(&event.start)
            .map_err(|_| AppError::invalid("Calendar start must be an RFC3339 timestamp."))?;
        let end = chrono::DateTime::parse_from_rfc3339(&event.end)
            .map_err(|_| AppError::invalid("Calendar end must be an RFC3339 timestamp."))?;
        if end < start || !event.id.contains(':') {
            return Err(AppError::invalid(
                "Calendar events need a source-qualified ID and an end after their start.",
            ));
        }
        text(&value["joinUrl"], "Join URL", 4096, true)?;
        if !event.join_url.is_empty() {
            let url = url::Url::parse(&event.join_url)
                .map_err(|_| AppError::invalid("Invalid meeting join URL."))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(AppError::invalid("Meeting links must use HTTP or HTTPS."));
            }
        }
        text_list(&value["attendees"], "Attendees", 100)?;
        Ok(event)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: String,
    pub title: String,
    pub owner: String,
    pub due_date: String,
    pub done: bool,
    pub created_at: String,
}
impl ActionItem {
    pub fn create(title: &Value, owner: &Value, due_date: &Value) -> Result<Self> {
        let mut task = Self {
            id: id(),
            title: String::new(),
            owner: String::new(),
            due_date: String::new(),
            done: false,
            created_at: now(),
        };
        task.update(&json!({"title":title,"owner":owner,"dueDate":due_date}))?;
        Ok(task)
    }
    pub fn update(&mut self, changes: &Value) -> Result<()> {
        let mut next = self.clone();
        for (key, value) in changes
            .as_object()
            .ok_or_else(|| AppError::invalid("Task changes must be an object."))?
        {
            match key.as_str() {
                "title" => next.title = text(value, "Task title", 2000, false)?.trim().into(),
                "owner" => next.owner = text(value, "Task owner", 240, true)?.trim().into(),
                "dueDate" => {
                    let date = text(value, "Due date", 10, true)?;
                    if !date.is_empty()
                        && (date.len() != 10
                            || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err())
                    {
                        return Err(AppError::invalid("Due date must be empty or YYYY-MM-DD."));
                    }
                    next.due_date = date.into();
                }
                "done" => {
                    next.done = value
                        .as_bool()
                        .ok_or_else(|| AppError::invalid("Done must be true or false."))?
                }
                _ => {
                    return Err(AppError::invalid(
                        "Only task title, owner, dueDate and done can be edited.",
                    ))
                }
            }
        }
        *self = next;
        Ok(())
    }
}
pub fn validate_saved_items(value: &Value, body: &str) -> Result<()> {
    let items = value
        .as_array()
        .filter(|v| v.len() <= 100)
        .ok_or_else(|| AppError::invalid("Save at most 100 templates or recipes."))?;
    let mut ids = std::collections::HashSet::new();
    for item in items {
        let id = text(&item["id"], "ID", 128, false)?;
        if Uuid::parse_str(id).is_err() || !ids.insert(id) {
            return Err(AppError::invalid("Saved items require unique UUIDs."));
        }
        text(&item["name"], "Name", 240, false)?;
        text(&item[body], body, 12_000, false)?;
    }
    Ok(())
}
pub fn template_instructions(template: &str, settings: &Value) -> String {
    if let Some(item) = settings["customTemplates"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["id"] == template))
    {
        return item["instructions"].as_str().unwrap_or("").into();
    }
    match template {
        "sales" => "Structure as Customer goals, Current workflow and pain, Fit and objections, Buying process, Decisions, and Follow-up. Keep commitments distinct from possibilities.",
        "standup" => "Structure by participant: Progress, Next work, Blockers. Then list dependencies and action items. Do not manufacture progress updates.",
        "interview" => "Structure as Role and context, Questions and evidence-backed responses, Demonstrated strengths, Areas to probe, and Next steps. Avoid unsupported hiring judgments.",
        "one-on-one" | "one_on_one" | "1on1" => "Structure as Check-in, Progress, Challenges, Feedback, Growth goals, and Agreed next steps. Preserve the participant's perspective and avoid inferring emotions.",
        "planning" | "project" => "Structure as Goal, Scope, Decisions and tradeoffs, Milestones, Risks and dependencies, and Actions with stated owners and dates.",
        _ => "Structure as Overview, Decisions, Action items with stated owners and dates, and Open questions.",
    }.into()
}
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub people: Vec<String>,
    pub meeting_ids: Vec<String>,
    pub limit: usize,
}
impl SearchQuery {
    pub fn from_value(value: &Value) -> Result<Self> {
        let query = text(&value["query"], "Search", 2000, false)?;
        let terms: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .take(12)
            .map(str::to_ascii_lowercase)
            .collect();
        if terms.is_empty() {
            return Err(AppError::invalid("Search for at least one word."));
        }
        let list = |key| {
            value
                .get(key)
                .map(|v| text_list(v, key, 100))
                .transpose()
                .map(Option::unwrap_or_default)
        };
        let folder = value
            .get("folder")
            .map(|v| text(v, "Folder", 240, true).map(str::to_owned))
            .transpose()?;
        let limit = match value.get("limit") {
            None => 12,
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=30).contains(n))
                .ok_or_else(|| AppError::invalid("Search limit must be between 1 and 30."))?
                as usize,
        };
        Ok(Self {
            terms,
            folder,
            tags: list("tags")?,
            people: list("people")?,
            meeting_ids: list("meetingIds")?,
            limit,
        })
    }
    pub fn matches(&self, m: &Meeting) -> bool {
        self.folder.as_ref().is_none_or(|f| &m.folder == f)
            && self.tags.iter().all(|t| m.tags.contains(t))
            && self.people.iter().all(|p| m.people.contains(p))
            && (self.meeting_ids.is_empty() || self.meeting_ids.contains(&m.id))
    }
    pub fn score(&self, text: &str) -> usize {
        let lower = text.to_ascii_lowercase();
        self.terms
            .iter()
            .filter(|t| lower.contains(t.as_str()))
            .count()
    }
    pub fn hit(
        &self,
        meeting_id: &str,
        title: &str,
        segment_id: Option<&str>,
        start: Option<f64>,
        source: &str,
        text: &str,
    ) -> Value {
        // Excerpts stay bounded and center on a real matching term, including Unicode text.
        let chars: Vec<char> = text.chars().collect();
        let lower = text.to_ascii_lowercase();
        let matched = self
            .terms
            .iter()
            .filter_map(|t| lower.find(t))
            .min()
            .unwrap_or(0);
        let offset = lower[..matched]
            .chars()
            .count()
            .saturating_sub(180)
            .min(chars.len());
        let excerpt: String = chars.iter().skip(offset).take(1200).collect();
        let mut url =
            url::Url::parse(&format!("tuck://meeting/{meeting_id}")).expect("meeting URL");
        {
            let mut q = url.query_pairs_mut();
            if let Some(id) = segment_id {
                q.append_pair("segment", id);
            }
            if let Some(s) = start {
                q.append_pair("start", &s.to_string());
            }
            q.append_pair("source", source);
        }
        let mut hit = json!({"meetingId":meeting_id,"meetingTitle":title,"source":source,"text":excerpt,"url":url.as_str()});
        if let Some(segment) = segment_id {
            hit["segmentId"] = json!(segment);
        }
        if let Some(start) = start {
            hit["start"] = json!(start);
        }
        hit
    }
}

/// Compose an effective evidence view without mutating the original speech record.
pub fn corrected_segment(mut raw: Value, correction: Option<&Value>) -> Value {
    if let Some(object) = raw.as_object_mut() {
        object.remove("originalText");
        object.remove("correctedAt");
    }
    raw["edited"] = json!(correction.is_some());
    if let Some(c) = correction {
        raw["originalText"] = raw["text"].clone();
        raw["text"] = c["text"].clone();
        raw["correctedAt"] = c["updatedAt"].clone();
    }
    raw
}

#[cfg(test)]
mod workflow_domain_tests {
    use super::*;
    #[test]
    fn metadata_evidence_has_no_invented_transcript_position() {
        let query = SearchQuery::from_value(&json!({"query":"budget"})).unwrap();
        let hit = query.hit(
            "meeting-id",
            "Budget",
            None,
            None,
            "notes",
            "Budget approved",
        );
        assert!(hit.get("segmentId").is_none());
        assert!(hit.get("start").is_none());
        assert!(!hit["url"].as_str().unwrap().contains("start="));
    }
}
