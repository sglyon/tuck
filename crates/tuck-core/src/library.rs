//! Validated library selections and lightweight read DTOs within the meeting context.
use crate::domain::{self, AppError, Meeting, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const FACET_LIMIT: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    pub folder: String,
    pub tags: Vec<String>,
    pub people: Vec<String>,
    pub meeting_ids: Vec<String>,
}
impl Scope {
    fn parse(value: &Value) -> Result<Self> {
        if !value.is_null() && !value.is_object() {
            return Err(AppError::invalid("Library scope must be an object."));
        }
        let list = |key: &str| -> Result<Vec<String>> {
            let mut values = value
                .get(key)
                .map(|v| domain::text_list(v, key, 100))
                .transpose()?
                .unwrap_or_default();
            values.sort();
            values.dedup();
            Ok(values)
        };
        Ok(Self {
            folder: value
                .get("folder")
                .map(|v| domain::text(v, "Folder", 240, true).map(str::to_owned))
                .transpose()?
                .unwrap_or_default(),
            tags: list("tags")?,
            people: list("people")?,
            meeting_ids: list("meetingIds")?,
        })
    }
    pub fn matches(&self, m: &Meeting) -> bool {
        (self.folder.is_empty() || self.folder == m.folder)
            && self.tags.iter().all(|v| m.tags.contains(v))
            && self.people.iter().all(|v| m.people.contains(v))
            && (self.meeting_ids.is_empty() || self.meeting_ids.contains(&m.id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    kind: String,
    query: String,
    starred: Option<bool>,
    scope: Scope,
    include_done: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    generation: i64,
    selection: Selection,
    position: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct LibraryQuery {
    pub limit: usize,
    pub query: String,
    pub starred: Option<bool>,
    pub scope: Scope,
    pub include_done: bool,
    selection: Selection,
    cursor: Option<Cursor>,
}
impl LibraryQuery {
    pub fn from_value(value: &Value, tasks: bool) -> Result<Self> {
        let limit = value
            .get("limit")
            .map(|v| {
                v.as_u64()
                    .filter(|n| (1..=100).contains(n))
                    .ok_or_else(|| AppError::invalid("Page limit must be between 1 and 100."))
            })
            .transpose()?
            .unwrap_or(50) as usize;
        let query = if tasks {
            String::new()
        } else {
            value
                .get("query")
                .map(|v| {
                    domain::text(v, "Search", 2000, true).map(|s| s.trim().to_ascii_lowercase())
                })
                .transpose()?
                .unwrap_or_default()
        };
        let starred = if tasks {
            None
        } else {
            value
                .get("starred")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| AppError::invalid("Starred must be true or false."))
                })
                .transpose()?
        };
        let scope = Scope::parse(&value["scope"])?;
        let include_done = if tasks {
            value
                .get("includeDone")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| AppError::invalid("Include done must be true or false."))
                })
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
        let selection = Selection {
            kind: if tasks { "tasks" } else { "meetings" }.into(),
            query: query.clone(),
            starred,
            scope: scope.clone(),
            include_done,
        };
        // Bound the serialized form, including escaping, so every generated cursor
        // can be replayed and a filter cannot dominate a lightweight response.
        if serde_json::to_vec(&selection)
            .expect("library selection")
            .len()
            > 32_000
        {
            return Err(AppError::invalid(
                "Library filters are too large. Choose fewer values.",
            ));
        }
        let cursor = value
            .get("cursor")
            .filter(|v| !v.is_null())
            .map(|v| {
                let text = domain::text(v, "Cursor", 128_000, false)?;
                let cursor: Cursor = serde_json::from_str(text)
                    .map_err(|_| AppError::invalid("The library cursor is invalid."))?;
                if cursor.version != 1
                    || cursor.selection != selection
                    || cursor.position.len() != if tasks { 6 } else { 2 }
                    || cursor.position.iter().any(|s| s.len() > 4096)
                {
                    return Err(AppError::invalid(
                        "The library cursor does not match this selection.",
                    ));
                }
                Ok(cursor)
            })
            .transpose()?;
        Ok(Self {
            limit,
            query,
            starred,
            scope,
            include_done,
            selection,
            cursor,
        })
    }
    pub fn position(&self, generation: i64) -> Result<Option<&[String]>> {
        if self
            .cursor
            .as_ref()
            .is_some_and(|c| c.generation != generation)
        {
            return Err(AppError::new(
                "cursor_stale",
                "Your library changed. Reload the first page.",
            ));
        }
        Ok(self.cursor.as_ref().map(|c| c.position.as_slice()))
    }
    pub fn next_cursor(&self, generation: i64, position: Vec<String>) -> Value {
        json!(serde_json::to_string(&Cursor {
            version: 1,
            generation,
            selection: self.selection.clone(),
            position
        })
        .expect("library cursor"))
    }
    pub fn matches(&self, m: &Meeting) -> bool {
        self.scope.matches(m) && self.starred.is_none_or(|v| v == m.starred)
    }
}

pub fn header(m: &Meeting) -> Value {
    json!({"id":m.id,"title":m.title,"kind":m.kind,"createdAt":m.created_at,"duration":m.duration,"status":m.status,"template":m.template,"starred":m.starred,"isExample":m.is_example,"notionUrl":m.notion_url,"notionStatus":m.notion_status,"folder":m.folder,"tags":m.tags,"people":m.people,"calendarEvent":m.calendar_event,"recoveredFrom":m.recovered_from,"taskCount":m.tasks.len(),"openTaskCount":m.tasks.iter().filter(|t| !t.done).count(),"hasNotes":!m.notes.is_empty(),"hasSummary":!m.summary.is_empty()})
}
pub fn task_position(v: &Value) -> Vec<String> {
    vec![
        if v["done"].as_bool().unwrap_or(false) {
            "1"
        } else {
            "0"
        }
        .into(),
        if v["dueDate"].as_str().unwrap_or("").is_empty() {
            "1"
        } else {
            "0"
        }
        .into(),
        v["dueDate"].as_str().unwrap_or("").into(),
        v["createdAt"].as_str().unwrap_or("").into(),
        v["meetingId"].as_str().unwrap_or("").into(),
        v["id"].as_str().unwrap_or("").into(),
    ]
}
