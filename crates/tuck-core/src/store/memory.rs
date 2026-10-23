//! In-memory contract adapter, also useful to embed a temporary workspace.
use crate::{
    domain::{default_settings, AppError, Meeting, TranscriptSegment},
    ports::{Repository, Result, Store},
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
#[derive(Clone, Default)]
struct Data {
    meetings: HashMap<String, Meeting>,
    segments: HashMap<String, Vec<Value>>,
    speakers: HashMap<String, Vec<Value>>,
    messages: HashMap<String, Vec<Value>>,
    bookmarks: HashMap<String, Vec<Value>>,
    revisions: HashMap<String, Vec<Value>>,
    corrections: HashMap<String, Vec<Value>>,
    settings: Option<Value>,
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
        meetings.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(meetings)
    }
    fn save(&mut self, m: &mut Meeting) -> Result<()> {
        match self.meetings.get(&m.id) {
            Some(current) if current.version != m.version => return Err(AppError::conflict()),
            None if m.version != 0 => return Err(AppError::conflict()),
            _ => {}
        }
        m.version += 1;
        self.meetings.insert(m.id.clone(), m.clone());
        Ok(())
    }
    fn delete(&mut self, m: &Meeting) -> Result<()> {
        if self.get(&m.id)?.version != m.version {
            return Err(AppError::conflict());
        }
        self.meetings.remove(&m.id);
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
        Ok(())
    }
    fn remove_bookmark(&mut self, id: &str, b: &str) -> Result<()> {
        self.bookmarks
            .entry(id.into())
            .or_default()
            .retain(|v| v["id"] != b);
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
    fn settings(&mut self) -> Result<Value> {
        Ok(self.settings.clone().unwrap_or_else(default_settings))
    }
    fn save_settings(&mut self, s: &Value) -> Result<()> {
        self.settings = Some(s.clone());
        Ok(())
    }
}
