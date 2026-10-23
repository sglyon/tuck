//! Bounded read-only platform calendar bridge and portable ICS snapshot import.
use super::speech::helper_path;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tuck_core::{ports::Result, AppError};
fn error(message: impl Into<String>) -> AppError {
    AppError::new("calendar_error", message)
}
const LIMIT: u64 = 2 * 1024 * 1024;

/// A child can never hold an RPC worker indefinitely. Reader threads prevent pipe deadlock,
/// and the bounded reader prevents an accidentally noisy helper consuming unbounded memory.
pub fn run_helper(verb: &str, args: &[String], timeout: Duration) -> Result<Value> {
    run_helper_at(
        &helper_path("tuck-calendar", "TUCK_CALENDAR_HELPER")?,
        verb,
        args,
        timeout,
    )
}
fn run_helper_at(helper: &Path, verb: &str, args: &[String], timeout: Duration) -> Result<Value> {
    let mut child = Command::new(helper)
        .arg(verb)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            error("The Calendar helper could not start. Reinstall Tuck or import an ICS file.")
        })?;
    let stdout = child.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut bytes = vec![];
        stdout
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(error(
                    "Calendar did not respond in time. Try again or import an ICS file.",
                ));
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| error("Calendar response could not be read."))?
        .map_err(|_| error("Calendar response could not be read."))?;
    if !status.success() || bytes.len() as u64 > LIMIT {
        return Err(error("Calendar returned an invalid response."));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| error("Calendar returned an invalid response."))?;
    if let Some(message) = value["error"].as_str() {
        return Err(error(message));
    }
    Ok(value)
}
pub fn status() -> Result<Value> {
    if cfg!(target_os = "macos") {
        run_helper("status", &[], Duration::from_secs(4)).or_else(|_|Ok(json!({"available":false,"connected":false,"source":"ics-import","message":"Apple Calendar helper unavailable. Import a local ICS file instead."})))
    } else {
        Ok(
            json!({"available":false,"connected":false,"source":"ics-import","message":"Import a local ICS snapshot. Google and Outlook OAuth sync is not configured; recurring ICS rules are not expanded."}),
        )
    }
}
pub fn connect() -> Result<Value> {
    if !cfg!(target_os = "macos") {
        return status();
    }
    run_helper("connect", &[], Duration::from_secs(60))
}
pub fn events(from: &str, to: &str) -> Result<Value> {
    let from =
        DateTime::parse_from_rfc3339(from).map_err(|_| error("Choose a valid start date."))?;
    let to = DateTime::parse_from_rfc3339(to).map_err(|_| error("Choose a valid end date."))?;
    if to <= from || (to - from).num_seconds() > 31 * 86400 {
        return Err(error("Choose a date range of at most 31 days."));
    }
    if !cfg!(target_os = "macos") {
        return Ok(json!([]));
    }
    run_helper(
        "events",
        &[
            from.with_timezone(&Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            to.with_timezone(&Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ],
        Duration::from_secs(10),
    )
}
pub fn import(path: &Path) -> Result<Value> {
    if path
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("ics"))
    {
        return Err(error("Choose an .ics calendar file."));
    }
    let file = File::open(path).map_err(|_| error("The calendar file could not be opened."))?;
    if !file
        .metadata()
        .map_err(|_| error("The calendar file could not be read."))?
        .is_file()
    {
        return Err(error("Choose a regular .ics calendar file."));
    }
    let mut bytes = vec![];
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("The calendar file could not be read."))?;
    if bytes.len() as u64 > LIMIT {
        return Err(error("Choose a calendar file smaller than 2 MB."));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| error("The calendar file must use UTF-8 text."))?;
    parse_ics(text)
}
fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                result.push(if next == 'n' || next == 'N' {
                    '\n'
                } else {
                    next
                });
            }
        } else {
            result.push(c);
        }
    }
    result
}
fn date(value: &str, properties: &str) -> Result<DateTime<Utc>> {
    if value.ends_with('Z') {
        return NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
            .map(|v| v.and_utc())
            .map_err(|_| error("An ICS event has an invalid UTC date."));
    }
    let naive = if value.len() == 8 {
        NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .and_then(|v| v.and_hms_opt(0, 0, 0))
    } else {
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()
    }
    .ok_or_else(|| error("An ICS event has an invalid date."))?;
    let tz = properties
        .split(';')
        .find_map(|part| part.strip_prefix("TZID="))
        .map(|v| v.trim_matches('"'));
    if let Some(tz) = tz {
        let tz: chrono_tz::Tz = tz.parse().map_err(|_|error(format!("The ICS timezone {tz} is unsupported. Export the calendar with UTC or IANA timezone dates.")))?;
        return tz
            .from_local_datetime(&naive)
            .single()
            .map(|v| v.with_timezone(&Utc))
            .ok_or_else(|| {
                error(
                    "An ICS event has an ambiguous daylight-saving time. Export this event in UTC.",
                )
            });
    }
    // Floating and date-only values follow the computer's local calendar timezone.
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|v| v.with_timezone(&Utc))
        .ok_or_else(|| {
            error("An ICS date is ambiguous. Export the calendar using explicit timezones.")
        })
}
fn duration(value: &str) -> Result<chrono::Duration> {
    let invalid = || error("An ICS event has an unsupported duration.");
    let text = value
        .strip_prefix('+')
        .unwrap_or(value)
        .strip_prefix('P')
        .ok_or_else(invalid)?;
    let mut number = String::new();
    let mut seconds: i64 = 0;
    let mut time = false;
    let mut units = 0;
    for c in text.chars() {
        if c == 'T' && !time && number.is_empty() {
            time = true;
            continue;
        }
        if c.is_ascii_digit() {
            number.push(c);
            if number.len() > 9 {
                return Err(invalid());
            }
            continue;
        }
        let multiplier = match (c, time) {
            ('W', false) => 604800,
            ('D', false) => 86400,
            ('H', true) => 3600,
            ('M', true) => 60,
            ('S', true) => 1,
            _ => return Err(invalid()),
        };
        let n: i64 = number.parse().map_err(|_| invalid())?;
        number.clear();
        seconds = seconds
            .checked_add(n.checked_mul(multiplier).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        units += 1;
    }
    if !number.is_empty() || units == 0 || seconds > 366 * 86400 {
        return Err(invalid());
    }
    Ok(chrono::Duration::seconds(seconds))
}
/// Snapshot import: properties are read as listed; RRULE is deliberately not expanded.
pub fn parse_ics(text: &str) -> Result<Value> {
    if text.len() as u64 > LIMIT {
        return Err(error("Choose a calendar file smaller than 2 MB."));
    }
    if !text.contains("BEGIN:VCALENDAR") || !text.contains("END:VCALENDAR") {
        return Err(error("This is not a complete ICS calendar."));
    }
    let mut lines: Vec<String> = vec![];
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(&line[1..]);
            }
        } else {
            lines.push(line.trim_end_matches('\r').to_owned());
        }
    }
    let mut inside = false;
    let mut fields: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut result = vec![];
    for line in lines {
        if line == "BEGIN:VEVENT" {
            if inside {
                return Err(error("The ICS calendar contains nested events."));
            }
            inside = true;
            fields.clear();
            continue;
        }
        if line == "END:VEVENT" && inside {
            inside = false;
            let first = |key: &str| fields.get(key).and_then(|v| v.first());
            if first("STATUS").is_some_and(|(_, v)| v.eq_ignore_ascii_case("CANCELLED")) {
                continue;
            }
            let Some((sp, start)) = first("DTSTART") else {
                continue;
            };
            let date_only = start.len() == 8;
            let start = date(start, sp)?;
            let end = if let Some((ep, end)) = first("DTEND") {
                date(end, ep)?
            } else if let Some((_, value)) = first("DURATION") {
                start + duration(value)?
            } else {
                start
                    + if date_only {
                        chrono::Duration::days(1)
                    } else {
                        chrono::Duration::zero()
                    }
            };
            if end < start {
                return Err(error("An ICS event ends before it starts."));
            }
            let title = first("SUMMARY")
                .map(|(_, v)| unescape(v))
                .unwrap_or_else(|| "Untitled event".into());
            let uid = first("UID")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| format!("{}:{}", start.timestamp(), title));
            let candidates = ["URL", "LOCATION", "DESCRIPTION"]
                .into_iter()
                .filter_map(|k| first(k).map(|(_, v)| unescape(v)))
                .collect::<Vec<_>>();
            let join = candidates
                .iter()
                .flat_map(|v| v.split_whitespace())
                .map(|s| s.trim_matches(|c: char| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')')))
                .find(|s| url::Url::parse(s).is_ok_and(|u| matches!(u.scheme(), "https" | "http")))
                .unwrap_or("");
            let attendees: Vec<String> = fields
                .get("ATTENDEE")
                .into_iter()
                .flatten()
                .take(100)
                .map(|(p, v)| {
                    p.split(';')
                        .find_map(|s| s.strip_prefix("CN="))
                        .map(|s| unescape(s.trim_matches('"')))
                        .unwrap_or_else(|| v.strip_prefix("mailto:").unwrap_or(v).to_owned())
                })
                .collect();
            result.push(json!({"id":format!("ics-import:{uid}:{}",start.timestamp()),"source":"ics-import","title":title,"start":start.to_rfc3339(),"end":end.to_rfc3339(),"joinUrl":join,"attendees":attendees}));
            if result.len() > 1000 {
                return Err(error("Import at most 1,000 calendar entries at a time."));
            }
            continue;
        }
        if inside {
            if let Some((properties, value)) = line.split_once(':') {
                let key = properties
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase();
                fields
                    .entry(key)
                    .or_default()
                    .push((properties.to_owned(), value.to_owned()));
            }
        }
    }
    if inside {
        return Err(error("The ICS calendar contains an unfinished event."));
    }
    result.sort_by(|a, b| a["start"].as_str().cmp(&b["start"].as_str()));
    Ok(json!(result))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unfolds_and_converts_timezones() {
        let v=parse_ics("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:demo\r\nSUMMARY:Weekly\\, review\r\n continued\r\nDTSTART;TZID=America/New_York:20261023T090000\r\nDTEND:20261023T140000Z\r\nATTENDEE;CN=Alice:mailto:alice@example.test\r\nURL:https://example.test/join\r\nEND:VEVENT\r\nEND:VCALENDAR").unwrap();
        assert_eq!(v[0]["start"], "2026-10-23T13:00:00+00:00");
        assert_eq!(v[0]["title"], "Weekly, reviewcontinued");
        assert_eq!(v[0]["attendees"][0], "Alice");
        assert_eq!(v[0]["joinUrl"], "https://example.test/join");
        assert!(v[0]["id"].as_str().unwrap().starts_with("ics-import:"));
    }
    #[test]
    fn rejects_incomplete_or_invalid_calendar() {
        assert!(parse_ics("BEGIN:VCALENDAR").is_err());
        assert!(
            parse_ics("BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:bad\nEND:VEVENT\nEND:VCALENDAR")
                .is_err()
        );
        assert!(parse_ics("BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261023T140000Z\nDTEND:20261023T130000Z\nEND:VEVENT\nEND:VCALENDAR").is_err());
    }
    #[test]
    fn durations_and_cancelled_events_are_honest() {
        assert_eq!(duration("P1DT30M").unwrap().num_minutes(), 1470);
        assert!(duration("P999999999W").is_err());
        let v=parse_ics("BEGIN:VCALENDAR\nBEGIN:VEVENT\nSTATUS:CANCELLED\nDTSTART:20261023T130000Z\nEND:VEVENT\nBEGIN:VEVENT\nDTSTART:20261023T140000Z\nDURATION:PT30M\nEND:VEVENT\nEND:VCALENDAR").unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["end"], "2026-10-23T14:30:00+00:00");
    }
    #[cfg(unix)]
    #[test]
    fn fake_helper_is_bounded_and_reports_errors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper");
        std::fs::write(&path,"#!/bin/sh\ncase \"$1\" in\nstatus) printf '%s' '{\"available\":true,\"connected\":false,\"source\":\"fixture\"}';;\nerror) printf '%s' '{\"error\":\"Permission fixture\"}';;\nwait) exec sleep 2;;\nesac\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            run_helper_at(&path, "status", &[], Duration::from_secs(1)).unwrap()["source"],
            "fixture"
        );
        assert!(run_helper_at(&path, "error", &[], Duration::from_secs(1))
            .unwrap_err()
            .message
            .contains("Permission fixture"));
        let started = Instant::now();
        assert!(run_helper_at(&path, "wait", &[], Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
