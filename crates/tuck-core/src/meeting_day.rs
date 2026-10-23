//! Small calendar-occurrence and reminder policies inside the Meeting Workspace.
use crate::{
    domain::{AppError, CalendarEvent},
    ports::Result,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const SNAPSHOT_LIMIT: usize = 256;
pub fn instant(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| AppError::invalid("Calendar time must be an RFC3339 timestamp."))
}
pub fn normalized(value: &str) -> Result<String> {
    Ok(instant(value)?.to_rfc3339_opts(SecondsFormat::AutoSi, true))
}
/// An occurrence is a value: equivalent offsets denote the same occurrence.
pub fn occurrence_key(event: &CalendarEvent) -> Result<String> {
    Ok(json!([event.source, event.id, normalized(&event.start)?]).to_string())
}
pub fn upcoming_event(event: &CalendarEvent, now: DateTime<Utc>) -> Result<bool> {
    let start = instant(&event.start)?;
    let end = instant(&event.end)?;
    Ok(!event.all_day && end > start && end > now && start <= now + Duration::days(7))
}
pub fn meeting_candidate(event: &CalendarEvent, now: DateTime<Utc>) -> Result<bool> {
    Ok(upcoming_event(event, now)? && (!event.attendees.is_empty() || !event.join_url.is_empty()))
}
pub fn reminder_due(event: &CalendarEvent, now: DateTime<Utc>) -> Result<bool> {
    let start = instant(&event.start)?;
    Ok(start >= now - Duration::seconds(60) && start <= now + Duration::minutes(5))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub scope: String,
    pub refreshed_at: String,
    pub expires_at: String,
    pub events: Vec<CalendarEvent>,
}
impl CalendarSnapshot {
    pub fn replacement(scope: &str, values: &[Value], now: DateTime<Utc>) -> Result<Self> {
        if !matches!(scope, "device" | "import") || values.len() > 1000 {
            return Err(AppError::invalid(
                "Calendar snapshots support device or import and at most 1,000 input events.",
            ));
        }
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for value in values {
            let event = CalendarEvent::from_value(value)?;
            let start = instant(&event.start)?;
            if upcoming_event(&event, now)?
                && start >= now - Duration::seconds(60)
                && seen.insert(occurrence_key(&event)?)
            {
                events.push(event);
            }
        }
        events.sort_by_key(|e| instant(&e.start).expect("validated start"));
        events.truncate(SNAPSHOT_LIMIT);
        Ok(Self {
            scope: scope.into(),
            refreshed_at: now.to_rfc3339(),
            expires_at: (now
                + if scope == "device" {
                    Duration::hours(24)
                } else {
                    Duration::days(7)
                })
            .to_rfc3339(),
            events,
        })
    }
    pub fn live(&self, now: DateTime<Utc>) -> Result<bool> {
        Ok(instant(&self.expires_at)? > now)
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderState {
    pub occurrence_key: String,
    pub delivered: bool,
    pub dismissed: bool,
    pub expires_at: String,
}
