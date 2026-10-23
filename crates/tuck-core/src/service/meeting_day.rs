use super::*;
use crate::meeting_day::{self as day, CalendarSnapshot, ReminderState};
use chrono::Duration;

impl MeetingService {
    /// Lookup and creation share one transaction, including concurrent callers and restarts.
    pub fn prepare_calendar_meeting(&self, value: &Value) -> Result<Value> {
        let event = domain::CalendarEvent::from_value(value)?;
        let result = self.transaction(|r| {
            let m = match r.calendar_meeting(&event)? {
                Some(m) => m,
                None => {
                    let mut m = Meeting::create(&truncate(&event.title, 240))?;
                    m.update(&json!({"calendarEvent":event,"people":event.attendees}))?;
                    r.save(&mut m)?;
                    m
                }
            };
            Self::view(r, m)
        })?;
        (self.emit)("meeting_updated", json!({"meeting":result}));
        Ok(result)
    }
    /// Host-only clock input; the public RPC always uses the device clock.
    pub fn replace_calendar_snapshot(
        &self,
        scope: &str,
        events: &[Value],
        now: &str,
    ) -> Result<Value> {
        let snapshot = CalendarSnapshot::replacement(scope, events, day::instant(now)?)?;
        self.transaction(|r| {
            r.prune_meeting_day(now)?;
            r.save_calendar_snapshot(&snapshot)?;
            Ok(Value::Null)
        })?;
        self.get_meeting_day_at(now)
    }
    pub fn meeting_day_suppressed(&self) -> Result<bool> {
        Ok(lock(&self.state)?.active.is_some())
    }
    pub fn meeting_reminders_enabled(&self) -> Result<bool> {
        Ok(self.transaction(|r| r.settings())?["meetingReminders"] == true)
    }
    pub fn set_meeting_reminders_enabled(&self, enabled: bool) -> Result<()> {
        self.transaction(|r| {
            let settings =
                domain::updated_settings(&r.settings()?, &json!({"meetingReminders":enabled}))?;
            r.save_settings(&settings)?;
            Ok(Value::Null)
        })?;
        Ok(())
    }
    /// Revoke freshness without discarding the bounded visible cache.
    pub fn invalidate_calendar_snapshot(&self, scope: &str) -> Result<()> {
        if !matches!(scope, "device" | "import") {
            return Err(AppError::invalid("Invalid calendar scope."));
        }
        self.transaction(|r| {
            for mut snapshot in r.calendar_snapshots()? {
                if snapshot.scope == scope {
                    // Imported snapshots have no live connection to revoke.
                    if scope == "device" {
                        snapshot.refreshed_at = "1970-01-01T00:00:00Z".into();
                    } else {
                        snapshot.expires_at = "1970-01-01T00:00:00Z".into();
                    }
                    r.save_calendar_snapshot(&snapshot)?;
                }
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }
    fn meeting_day_view(
        &self,
        r: &mut dyn Repository,
        now: &str,
        suppressed: bool,
        claim: bool,
    ) -> Result<Value> {
        let time = day::instant(now)?;
        r.prune_meeting_day(now)?;
        let enabled = r.settings()?["meetingReminders"] == true;
        if claim && (!enabled || suppressed) {
            return Ok(json!([]));
        }
        let mut snapshots = Vec::new();
        let mut events = Vec::new();
        let mut reminders = Vec::new();
        let mut seen = HashSet::new();
        for snapshot in r.calendar_snapshots()? {
            snapshots.push(json!({"scope":snapshot.scope,"refreshedAt":snapshot.refreshed_at,"expiresAt":snapshot.expires_at}));
            if !snapshot.live(time)? {
                continue;
            }
            let fresh = snapshot.scope == "import"
                || day::instant(&snapshot.refreshed_at)? >= time - Duration::minutes(5);
            for event in snapshot.events {
                if !day::upcoming_event(&event, time)? {
                    continue;
                }
                let key = day::occurrence_key(&event)?;
                if !seen.insert(key.clone()) {
                    continue;
                }
                let meeting_id = r.calendar_meeting_id(&event)?;
                let dto = json!({"occurrenceKey":key,"event":event,"meetingId":meeting_id});
                events.push(dto.clone());
                if !enabled
                    || suppressed
                    || !fresh
                    || !day::meeting_candidate(&event, time)?
                    || !day::reminder_due(&event, time)?
                {
                    continue;
                }
                let mut state = r.reminder_state(&key)?.unwrap_or(ReminderState {
                    occurrence_key: key,
                    delivered: false,
                    dismissed: false,
                    expires_at: (day::instant(&event.start)? + Duration::days(8)).to_rfc3339(),
                });
                if state.dismissed || (claim && state.delivered) {
                    continue;
                }
                if claim {
                    state.delivered = true;
                    r.save_reminder_state(&state)?;
                }
                reminders.push(dto);
            }
        }
        let sort = |a: &Value, b: &Value| {
            a["event"]["start"]
                .as_str()
                .and_then(|s| day::instant(s).ok())
                .cmp(
                    &b["event"]["start"]
                        .as_str()
                        .and_then(|s| day::instant(s).ok()),
                )
                .then_with(|| {
                    a["occurrenceKey"]
                        .as_str()
                        .cmp(&b["occurrenceKey"].as_str())
                })
        };
        events.sort_by(sort);
        reminders.sort_by(sort);
        if claim {
            Ok(json!(reminders))
        } else {
            Ok(
                json!({"events":events,"snapshots":snapshots,"reminders":reminders,"enabled":enabled,"suppressed":suppressed}),
            )
        }
    }
    pub fn get_meeting_day_at(&self, now: &str) -> Result<Value> {
        let suppressed = self.meeting_day_suppressed()?;
        self.transaction(|r| self.meeting_day_view(r, now, suppressed, false))
    }
    /// Persist the delivery claim before the host attempts an OS notification.
    /// A crash or OS failure may omit a notification; it must never replay old ones.
    pub fn claim_meeting_reminders_at(&self, now: &str) -> Result<Value> {
        // A host clock captured before a long capture operation must not queue
        // and become an old notification after the operation completes.
        let _life = match self.lifecycle.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(json!([])),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(AppError::unavailable(
                    "Meeting reminders are unavailable. Restart Tuck and try again.",
                ))
            }
        };
        let suppressed = self.meeting_day_suppressed()?;
        self.transaction(|r| self.meeting_day_view(r, now, suppressed, true))
    }
    pub(super) fn dismiss_meeting_reminder(&self, key: &str) -> Result<Value> {
        domain::text(&json!(key), "Reminder", 16_384, false)?;
        let now = domain::now();
        self.transaction(|r| {
            let snapshots = r.calendar_snapshots()?;
            let event = snapshots
                .iter()
                .flat_map(|s| &s.events)
                .find(|e| day::occurrence_key(e).is_ok_and(|k| k == key))
                .ok_or_else(|| AppError::invalid("This reminder has expired."))?;
            let mut state = r.reminder_state(key)?.unwrap_or(ReminderState {
                occurrence_key: key.into(),
                delivered: false,
                dismissed: false,
                expires_at: (day::instant(&event.start)? + Duration::days(8)).to_rfc3339(),
            });
            state.dismissed = true;
            r.save_reminder_state(&state)?;
            r.prune_meeting_day(&now)?;
            Ok(Value::Null)
        })
    }
}
