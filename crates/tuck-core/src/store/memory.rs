//! In-memory contract adapter, also useful to embed a temporary workspace.
use crate::{
    domain::{default_settings, AppError, Meeting, TranscriptSegment},
    ports::{Repository, Result, Store},
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
#[derive(Clone, Default)]
struct Data {
    generation: i64,
    meetings: HashMap<String, Meeting>,
    segments: HashMap<String, Vec<Value>>,
    speakers: HashMap<String, Vec<Value>>,
    messages: HashMap<String, Vec<Value>>,
    bookmarks: HashMap<String, Vec<Value>>,
    revisions: HashMap<String, Vec<Value>>,
    corrections: HashMap<String, Vec<Value>>,
    settings: Option<Value>,
    calendar_snapshots: HashMap<String, crate::meeting_day::CalendarSnapshot>,
    reminder_states: HashMap<String, crate::meeting_day::ReminderState>,
}
#[derive(Default)]
pub struct MemoryStore {
    data: Mutex<Data>,
}
impl Store for MemoryStore {
    fn transaction(
        &self,
        action: &mut dyn FnMut(&mut dyn Repository) -> Result<Value>,
    ) -> Result<Value> {
        let mut data = self
            .data
            .lock()
            .map_err(|_| AppError::unavailable("The library is unavailable."))?;
        let mut candidate = data.clone();
        let result = action(&mut candidate)?;
        *data = candidate;
        Ok(result)
    }
}
impl Repository for Data {
    fn get(&mut self, id: &str) -> Result<Meeting> {
        self.meetings
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::invalid("This meeting no longer exists."))
    }
    fn list(&mut self) -> Result<Vec<Meeting>> {
        let mut meetings: Vec<_> = self.meetings.values().cloned().collect();
        meetings.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(meetings)
    }
    fn recoverable(&mut self) -> Result<Vec<Meeting>> {
        Ok(self
            .meetings
            .values()
            .filter(|m| {
                matches!(m.status.as_str(), "recording" | "paused" | "processing")
                    || m.notion_status == "syncing"
            })
            .cloned()
            .collect())
    }
    fn meeting_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value> {
        let position = query.position(self.generation)?;
        let mut matches: Vec<_> = self
            .meetings
            .values()
            .filter(|m| {
                query.matches(m)
                    && (query.query.is_empty()
                        || [&m.title, &m.notes, &m.summary]
                            .iter()
                            .any(|v| v.to_ascii_lowercase().contains(&query.query))
                        || self.segments.get(&m.id).into_iter().flatten().any(|s| {
                            let correction = self
                                .corrections
                                .get(&m.id)
                                .and_then(|v| v.iter().find(|c| c["segmentId"] == s["id"]));
                            correction.unwrap_or(s)["text"]
                                .as_str()
                                .unwrap_or("")
                                .to_ascii_lowercase()
                                .contains(&query.query)
                        }))
            })
            .collect();
        matches.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        let total = matches.len();
        let mut meetings: Vec<_> = matches
            .into_iter()
            .filter(|m| position.is_none_or(|p| [&m.created_at, &m.id] < [&p[0], &p[1]]))
            .take(query.limit + 1)
            .map(|m| {
                let mut h = crate::library::header(m);
                for (key, count) in [
                    ("segmentCount", self.segments.get(&m.id).map_or(0, Vec::len)),
                    ("speakerCount", self.speakers.get(&m.id).map_or(0, Vec::len)),
                    ("messageCount", self.messages.get(&m.id).map_or(0, Vec::len)),
                    (
                        "bookmarkCount",
                        self.bookmarks.get(&m.id).map_or(0, Vec::len),
                    ),
                ] {
                    h[key] = json!(count);
                }
                h
            })
            .collect();
        let next = if meetings.len() > query.limit {
            meetings.pop();
            let last = meetings.last().expect("nonempty page");
            query.next_cursor(
                self.generation,
                vec![
                    last["createdAt"].as_str().unwrap().into(),
                    last["id"].as_str().unwrap().into(),
                ],
            )
        } else {
            Value::Null
        };
        Ok(json!({"meetings":meetings,"nextCursor":next,"total":total}))
    }
    fn library_overview(&mut self) -> Result<Value> {
        use std::collections::BTreeSet;
        let mut folders = BTreeSet::new();
        let mut tags = BTreeSet::new();
        let mut people = BTreeSet::new();
        let mut active: Vec<_> = Vec::new();
        for m in self.meetings.values() {
            if !m.folder.is_empty() {
                folders.insert(m.folder.clone());
            }
            tags.extend(m.tags.iter().cloned());
            people.extend(m.people.iter().cloned());
            if matches!(m.status.as_str(), "recording" | "paused" | "processing") {
                active.push(m);
            }
        }
        active.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        let max = crate::library::FACET_LIMIT;
        Ok(
            json!({"meetingCount":self.meetings.len(),"starredCount":self.meetings.values().filter(|m|m.starred).count(),"openTaskCount":self.meetings.values().flat_map(|m| &m.tasks).filter(|t| !t.done).count(),"activeMeetingId":active.first().map(|m| &m.id),"facetsTruncated":{"folders":folders.len()>max,"tags":tags.len()>max,"people":people.len()>max},"folders":folders.into_iter().take(max).collect::<Vec<_>>(),"tags":tags.into_iter().take(max).collect::<Vec<_>>(),"people":people.into_iter().take(max).collect::<Vec<_>>()}),
        )
    }
    fn task_page(&mut self, query: &crate::library::LibraryQuery) -> Result<Value> {
        let position = query.position(self.generation)?;
        let mut tasks: Vec<_> = self
            .meetings
            .values()
            .filter(|m| query.scope.matches(m))
            .flat_map(|m| {
                m.tasks
                    .iter()
                    .filter(|t| query.include_done || !t.done)
                    .map(|t| {
                        let mut value = serde_json::to_value(t).expect("task DTO");
                        value["meetingId"] = json!(m.id);
                        value["meetingTitle"] = json!(m.title);
                        value
                    })
            })
            .collect();
        tasks.sort_by_key(crate::library::task_position);
        let total = tasks.len();
        let mut tasks: Vec<_> = tasks
            .into_iter()
            .filter(|t| position.is_none_or(|p| crate::library::task_position(t).as_slice() > p))
            .take(query.limit + 1)
            .collect();
        let next = if tasks.len() > query.limit {
            tasks.pop();
            query.next_cursor(
                self.generation,
                crate::library::task_position(tasks.last().expect("nonempty page")),
            )
        } else {
            Value::Null
        };
        Ok(json!({"tasks":tasks,"total":total,"nextCursor":next}))
    }
    fn save(&mut self, m: &mut Meeting) -> Result<()> {
        match self.meetings.get(&m.id) {
            Some(current) if current.version != m.version => return Err(AppError::conflict()),
            None if m.version != 0 => return Err(AppError::conflict()),
            _ => {}
        }
        m.version += 1;
        self.meetings.insert(m.id.clone(), m.clone());
        self.generation += 1;
        Ok(())
    }
    fn delete(&mut self, m: &Meeting) -> Result<()> {
        if self.get(&m.id)?.version != m.version {
            return Err(AppError::conflict());
        }
        self.meetings.remove(&m.id);
        self.generation += 1;
        self.segments.remove(&m.id);
        self.speakers.remove(&m.id);
        self.messages.remove(&m.id);
        self.bookmarks.remove(&m.id);
        self.revisions.remove(&m.id);
        self.corrections.remove(&m.id);
        Ok(())
    }
    fn append(&mut self, s: &TranscriptSegment) -> Result<bool> {
        self.get(&s.meeting_id)?;
        let values = self.segments.entry(s.meeting_id.clone()).or_default();
        if values.iter().any(|v| v["id"] == s.id) {
            return Ok(false);
        }
        values.push(s.dto());
        self.generation += 1;
        values.sort_by(|a, b| {
            a["start"]
                .as_f64()
                .unwrap_or(0.)
                .total_cmp(&b["start"].as_f64().unwrap_or(0.))
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        let speakers = self.speakers.entry(s.meeting_id.clone()).or_default();
        if !speakers.iter().any(|v| v["id"] == s.speaker_id) {
            let name = if s.speaker_id == "you" {
                "You".into()
            } else {
                format!(
                    "Speaker {}",
                    s.speaker_id
                        .strip_prefix("speaker-")
                        .unwrap_or(&s.speaker_id)
                )
            };
            speakers.push(json!({"id":s.speaker_id,"name":name,"color":speakers.len()%6}));
        }
        Ok(true)
    }
    fn raw_segments(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self.segments.get(id).cloned().unwrap_or_default())
    }
    fn segments(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self
            .segments
            .get(id)
            .into_iter()
            .flatten()
            .map(|s| {
                crate::domain::corrected_segment(
                    s.clone(),
                    self.corrections
                        .get(id)
                        .and_then(|cs| cs.iter().find(|c| c["segmentId"] == s["id"])),
                )
            })
            .collect())
    }
    fn corrections(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self.corrections.get(id).cloned().unwrap_or_default())
    }
    fn save_correction(
        &mut self,
        id: &str,
        segment_id: &str,
        correction: Option<&Value>,
    ) -> Result<()> {
        self.get(id)?;
        if !self
            .segments
            .get(id)
            .is_some_and(|s| s.iter().any(|v| v["id"] == segment_id))
        {
            return Err(AppError::invalid(
                "This transcript segment no longer exists.",
            ));
        }
        let values = self.corrections.entry(id.into()).or_default();
        values.retain(|c| c["segmentId"] != segment_id);
        if let Some(c) = correction {
            values.push(c.clone());
        }
        self.generation += 1;
        Ok(())
    }
    fn speakers(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self.speakers.get(id).cloned().unwrap_or_default())
    }
    fn rename_speaker(&mut self, id: &str, speaker: &str, name: &str) -> Result<()> {
        let value = self
            .speakers
            .entry(id.into())
            .or_default()
            .iter_mut()
            .find(|v| v["id"] == speaker)
            .ok_or_else(|| AppError::invalid("This speaker is not part of the meeting."))?;
        value["name"] = json!(name);
        self.generation += 1;
        Ok(())
    }
    fn save_speaker(&mut self, id: &str, speaker: &Value) -> Result<()> {
        self.get(id)?;
        let values = self.speakers.entry(id.into()).or_default();
        if let Some(v) = values.iter_mut().find(|v| v["id"] == speaker["id"]) {
            *v = speaker.clone();
        } else {
            values.push(speaker.clone());
        }
        self.generation += 1;
        Ok(())
    }
    fn messages(&mut self, id: &str) -> Result<Vec<Value>> {
        let mut v = self.messages.get(id).cloned().unwrap_or_default();
        v.sort_by(|a, b| a["createdAt"].as_str().cmp(&b["createdAt"].as_str()));
        Ok(v)
    }
    fn add_message(&mut self, id: &str, m: &Value) -> Result<()> {
        self.get(id)?;
        let v = self.messages.entry(id.into()).or_default();
        if v.iter().any(|x| x["id"] == m["id"]) {
            return Err(AppError::conflict());
        }
        v.push(m.clone());
        self.generation += 1;
        Ok(())
    }
    fn bookmarks(&mut self, id: &str) -> Result<Vec<Value>> {
        let mut values = self.bookmarks.get(id).cloned().unwrap_or_default();
        values.sort_by(|a, b| {
            a["time"]
                .as_f64()
                .unwrap_or(0.)
                .total_cmp(&b["time"].as_f64().unwrap_or(0.))
        });
        Ok(values)
    }
    fn add_bookmark(&mut self, id: &str, b: &Value) -> Result<()> {
        self.get(id)?;
        let v = self.bookmarks.entry(id.into()).or_default();
        if v.iter().any(|x| x["id"] == b["id"]) {
            return Err(AppError::conflict());
        }
        v.push(b.clone());
        self.generation += 1;
        Ok(())
    }
    fn remove_bookmark(&mut self, id: &str, b: &str) -> Result<()> {
        self.bookmarks
            .entry(id.into())
            .or_default()
            .retain(|v| v["id"] != b);
        self.generation += 1;
        Ok(())
    }
    fn add_revision(&mut self, id: &str, revision: &Value) -> Result<()> {
        self.get(id)?;
        self.revisions
            .entry(id.into())
            .or_default()
            .push(revision.clone());
        Ok(())
    }
    fn revisions(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self
            .revisions
            .get(id)
            .map(|v| v.iter().rev().take(100).cloned().collect())
            .unwrap_or_default())
    }
    fn latest_revision_info(&mut self, id: &str, target: &str) -> Result<Option<Value>> {
        Ok(self.revisions.get(id).and_then(|values|values.iter().rev().find(|v|v["target"]==target)).map(|v|json!({"createdAt":v["createdAt"],"kind":v["kind"].as_str().unwrap_or("replacement")})))
    }
    fn all_revisions(&mut self, id: &str) -> Result<Vec<Value>> {
        Ok(self.revisions.get(id).cloned().unwrap_or_default())
    }
    fn search(&mut self, query: &crate::domain::SearchQuery) -> Result<Vec<Value>> {
        let mut hits = Vec::new();
        for m in self.meetings.values().filter(|m| query.matches(m)) {
            for (source, body) in [
                ("title", &m.title),
                ("notes", &m.notes),
                ("summary", &m.summary),
            ] {
                let score = query.score(body);
                if score > 0 {
                    hits.push((score, query.hit(&m.id, &m.title, None, None, source, body)));
                }
            }
            for s in self.segments.get(&m.id).into_iter().flatten() {
                let correction = self
                    .corrections
                    .get(&m.id)
                    .and_then(|cs| cs.iter().find(|c| c["segmentId"] == s["id"]));
                let body = correction.unwrap_or(s)["text"].as_str().unwrap_or("");
                let score = query.score(body);
                if score > 0 {
                    let mut hit = query.hit(
                        &m.id,
                        &m.title,
                        s["id"].as_str(),
                        s["start"].as_f64(),
                        "transcript",
                        body,
                    );
                    if correction.is_some() {
                        hit["edited"] = json!(true);
                        hit["text"] = json!(format!(
                            "[User-corrected transcript] {}",
                            hit["text"].as_str().unwrap_or("")
                        )
                        .chars()
                        .take(1200)
                        .collect::<String>());
                    }
                    hits.push((score, hit));
                }
                // Retain only a bounded best set, even for a large temporary library.
                hits.sort_by(|a, b| {
                    b.0.cmp(&a.0)
                        .then_with(|| a.1["meetingId"].as_str().cmp(&b.1["meetingId"].as_str()))
                        .then_with(|| a.1["segmentId"].as_str().cmp(&b.1["segmentId"].as_str()))
                });
                hits.truncate(query.limit);
            }
            hits.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1["meetingId"].as_str().cmp(&b.1["meetingId"].as_str()))
                    .then_with(|| a.1["segmentId"].as_str().cmp(&b.1["segmentId"].as_str()))
            });
            hits.truncate(query.limit);
        }
        Ok(hits.into_iter().map(|(_, hit)| hit).collect())
    }
    fn calendar_meeting(
        &mut self,
        event: &crate::domain::CalendarEvent,
    ) -> Result<Option<Meeting>> {
        let key = crate::meeting_day::occurrence_key(event)?;
        let mut matches: Vec<_> = self
            .meetings
            .values()
            .filter(|m| {
                m.calendar_event
                    .as_ref()
                    .is_some_and(|e| crate::meeting_day::occurrence_key(e).is_ok_and(|k| k == key))
            })
            .cloned()
            .collect();
        matches.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(matches.into_iter().next())
    }
    fn calendar_meeting_id(
        &mut self,
        event: &crate::domain::CalendarEvent,
    ) -> Result<Option<String>> {
        let key = crate::meeting_day::occurrence_key(event)?;
        Ok(self
            .meetings
            .values()
            .filter(|m| {
                m.calendar_event
                    .as_ref()
                    .is_some_and(|e| crate::meeting_day::occurrence_key(e).is_ok_and(|k| k == key))
            })
            .min_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| a.id.cmp(&b.id))
            })
            .map(|m| m.id.clone()))
    }
    fn calendar_snapshots(&mut self) -> Result<Vec<crate::meeting_day::CalendarSnapshot>> {
        let mut values: Vec<_> = self.calendar_snapshots.values().cloned().collect();
        values.sort_by(|a, b| a.scope.cmp(&b.scope));
        Ok(values)
    }
    fn save_calendar_snapshot(
        &mut self,
        snapshot: &crate::meeting_day::CalendarSnapshot,
    ) -> Result<()> {
        self.calendar_snapshots
            .insert(snapshot.scope.clone(), snapshot.clone());
        Ok(())
    }
    fn reminder_state(&mut self, key: &str) -> Result<Option<crate::meeting_day::ReminderState>> {
        Ok(self.reminder_states.get(key).cloned())
    }
    fn save_reminder_state(&mut self, state: &crate::meeting_day::ReminderState) -> Result<()> {
        self.reminder_states
            .insert(state.occurrence_key.clone(), state.clone());
        Ok(())
    }
    fn prune_meeting_day(&mut self, now: &str) -> Result<()> {
        let now = crate::meeting_day::instant(now)?;
        for snapshot in self.calendar_snapshots.values_mut() {
            if !snapshot.live(now)? {
                snapshot.events.clear();
            }
        }
        self.reminder_states
            .retain(|_, s| crate::meeting_day::instant(&s.expires_at).is_ok_and(|v| v > now));
        Ok(())
    }
    fn settings(&mut self) -> Result<Value> {
        let mut settings = default_settings();
        if let Some(obj) = self.settings.as_ref().and_then(Value::as_object) {
            for (k, v) in obj {
                if settings.get(k).is_some() {
                    settings[k] = v.clone();
                }
            }
        }
        Ok(settings)
    }
    fn save_settings(&mut self, s: &Value) -> Result<()> {
        self.settings = Some(s.clone());
        Ok(())
    }
}
