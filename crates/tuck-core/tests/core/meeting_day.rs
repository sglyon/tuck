use super::*;
const NOW: &str = "2030-06-01T12:00:00Z";
fn event() -> Value {
    json!({"id":"device:weekly","source":"device","title":"Planning","start":"2030-06-01T12:05:00Z","end":"2030-06-01T13:00:00Z","joinUrl":"https://example.com/meeting","attendees":["Ada"]})
}
#[test]
fn calendar_prepare_is_atomic_occurrence_scoped_and_preserves_workspace() {
    for store in stores() {
        let r = rig(store.clone());
        let original = call(
            &r.service,
            "prepare_calendar_meeting",
            json!({"event":event()}),
        );
        let id = original["id"].as_str().unwrap();
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Keep my agenda","title":"My title"}}),
        );
        let mut changed = event();
        changed["title"] = json!("Renamed by organizer");
        changed["start"] = json!("2030-06-01T08:05:00-04:00");
        let resumed = rig(store.clone());
        let found = call(
            &resumed.service,
            "prepare_calendar_meeting",
            json!({"event":changed}),
        );
        assert_eq!(found["id"], id);
        assert_eq!(found["title"], "My title");
        assert_eq!(found["notes"], "Keep my agenda");
        assert_eq!(found["calendarEvent"], original["calendarEvent"]);
        let mut next = event();
        next["start"] = json!("2030-06-02T12:05:00Z");
        next["end"] = json!("2030-06-02T13:00:00Z");
        assert_ne!(
            call(
                &r.service,
                "prepare_calendar_meeting",
                json!({"event":next})
            )["id"],
            id
        );
        let mut different = event();
        different["source"] = json!("another");
        assert_ne!(
            call(
                &r.service,
                "prepare_calendar_meeting",
                json!({"event":different})
            )["id"],
            id
        );
        let mut threads = Vec::new();
        for _ in 0..8 {
            let s = r.service.clone();
            threads.push(std::thread::spawn(move || {
                s.prepare_calendar_meeting(&event()).unwrap()["id"].clone()
            }));
        }
        for thread in threads {
            assert_eq!(thread.join().unwrap(), id);
        }
        assert!(r.capture.id.lock().unwrap().is_empty());
        assert!(r.ai.calls.lock().unwrap().is_empty());
    }
}
#[test]
fn calendar_snapshots_are_bounded_isolated_replaced_and_expire() {
    for store in stores() {
        let r = rig(store.clone());
        r.service
            .replace_calendar_snapshot("device", &[event()], NOW)
            .unwrap();
        let mut imported = event();
        imported["id"] = json!("ics:other");
        imported["source"] = json!("ics");
        r.service
            .replace_calendar_snapshot("import", &[imported.clone()], NOW)
            .unwrap();
        r.service
            .replace_calendar_snapshot("device", &[], NOW)
            .unwrap();
        let s = rig(store.clone()).service;
        let day = s.get_meeting_day_at(NOW).unwrap();
        assert_eq!(day["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            day["events"][0]["event"],
            json!({"allDay":false,"id":"ics:other","source":"ics","title":"Planning","start":"2030-06-01T12:05:00Z","end":"2030-06-01T13:00:00Z","joinUrl":"https://example.com/meeting","attendees":["Ada"]})
        );
        assert!(s
            .replace_calendar_snapshot("import", &vec![event(); 1001], NOW)
            .is_err());
        let mut bad = event();
        bad["start"] = json!("invalid");
        assert!(s.replace_calendar_snapshot("import", &[bad], NOW).is_err());
        assert_eq!(
            s.get_meeting_day_at(NOW).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            s.get_meeting_day_at("2030-06-09T12:00:00Z").unwrap()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        tx(&*store, |repo| {
            assert!(repo
                .calendar_snapshots()?
                .iter()
                .all(|s| s.events.is_empty()));
            Ok(Value::Null)
        })
        .unwrap();
    }
}
#[test]
fn reminders_use_fixed_window_durable_dedup_and_dismissal() {
    for store in stores() {
        let r = rig(store.clone());
        let s = &r.service;
        s.replace_calendar_snapshot("import", &[event()], NOW)
            .unwrap();
        assert!(!s.meeting_reminders_enabled().unwrap());
        assert_eq!(s.claim_meeting_reminders_at(NOW).unwrap(), json!([]));
        s.set_meeting_reminders_enabled(true).unwrap();
        assert_eq!(
            s.claim_meeting_reminders_at("2030-06-01T11:59:59Z")
                .unwrap(),
            json!([])
        );
        let claimed = s.claim_meeting_reminders_at(NOW).unwrap();
        assert_eq!(claimed.as_array().unwrap().len(), 1);
        assert_eq!(
            rig(store.clone())
                .service
                .claim_meeting_reminders_at(NOW)
                .unwrap(),
            json!([])
        );
        assert_eq!(
            s.get_meeting_day_at(NOW).unwrap()["reminders"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        call(
            s,
            "dismiss_meeting_reminder",
            json!({"occurrenceKey":claimed[0]["occurrenceKey"]}),
        );
        assert_eq!(
            rig(store.clone()).service.get_meeting_day_at(NOW).unwrap()["reminders"],
            json!([])
        );
        let mut late = event();
        late["id"] = json!("device:late");
        s.replace_calendar_snapshot("import", &[late.clone()], NOW)
            .unwrap();
        assert_eq!(
            s.claim_meeting_reminders_at("2030-06-01T12:06:01Z")
                .unwrap(),
            json!([])
        );
        assert_eq!(
            s.claim_meeting_reminders_at("2030-06-01T12:06:00Z")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
#[test]
fn reminder_freshness_recording_suppression_and_candidate_policy() {
    for store in stores() {
        let r = rig(store.clone());
        let s = &r.service;
        s.set_meeting_reminders_enabled(true).unwrap();
        s.replace_calendar_snapshot("device", &[event()], NOW)
            .unwrap();
        assert_eq!(
            s.claim_meeting_reminders_at("2030-06-01T12:05:01Z")
                .unwrap(),
            json!([])
        );
        assert_eq!(
            s.get_meeting_day_at(NOW).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        s.invalidate_calendar_snapshot("device").unwrap();
        assert_eq!(s.claim_meeting_reminders_at(NOW).unwrap(), json!([]));
        assert_eq!(
            s.get_meeting_day_at(NOW).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        s.replace_calendar_snapshot("device", &[event()], NOW)
            .unwrap();
        let id = create(&r);
        s.start_recording(&id, true, true).unwrap();
        assert_eq!(s.claim_meeting_reminders_at(NOW).unwrap(), json!([]));
        assert_eq!(s.get_meeting_day_at(NOW).unwrap()["suppressed"], true);
        call(s, "pause_recording", json!({"meetingId":id}));
        assert_eq!(s.claim_meeting_reminders_at(NOW).unwrap(), json!([]));
        s.stop_recording(&id).unwrap();
        assert_eq!(
            s.claim_meeting_reminders_at(NOW)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut solo = event();
        solo["id"] = json!("device:solo");
        solo["attendees"] = json!([]);
        solo["joinUrl"] = json!("");
        let mut all = event();
        all["id"] = json!("device:all");
        all["allDay"] = json!(true);
        let mut zero = event();
        zero["id"] = json!("device:zero");
        zero["end"] = zero["start"].clone();
        s.replace_calendar_snapshot("device", &[solo, all, zero], NOW)
            .unwrap();
        assert_eq!(
            s.get_meeting_day_at(NOW).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(s.claim_meeting_reminders_at(NOW).unwrap(), json!([]));
    }
}
#[test]
fn old_preferences_and_backups_do_not_enable_or_export_cached_calendars() {
    for store in stores() {
        tx(&*store, |repo| {
            repo.save_settings(&json!({"keepAudio":false}))?;
            assert_eq!(repo.settings()?["meetingReminders"], false);
            Ok(Value::Null)
        })
        .unwrap();
        let r = rig(store.clone());
        r.service.set_meeting_reminders_enabled(true).unwrap();
        r.service
            .replace_calendar_snapshot("import", &[event()], NOW)
            .unwrap();
        call(
            &r.service,
            "backup_library",
            json!({"path":"fixture","includeAudio":false}),
        );
        let manifest = r.archive.backup.lock().unwrap().clone().unwrap();
        assert_eq!(manifest["settings"]["meetingReminders"], false);
        assert!(!manifest.to_string().contains("device:weekly"));
    }
}
#[test]
fn sqlite_independent_connections_prepare_once_and_reopen_claims() {
    let directory = std::env::temp_dir().join(format!("tuck-calendar-{}", domain::id()));
    let path = directory.join("library.sqlite");
    let first = rig(Arc::new(SqliteStore::open(&path).unwrap()));
    let second = rig(Arc::new(SqliteStore::open(&path).unwrap()));
    let a = first.service.clone();
    let b = second.service.clone();
    let one = std::thread::spawn(move || a.prepare_calendar_meeting(&event()).unwrap());
    let two = std::thread::spawn(move || b.prepare_calendar_meeting(&event()).unwrap());
    let id = one.join().unwrap()["id"].clone();
    assert_eq!(two.join().unwrap()["id"], id);
    first.service.set_meeting_reminders_enabled(true).unwrap();
    first
        .service
        .replace_calendar_snapshot("import", &[event()], NOW)
        .unwrap();
    let a = first.service.clone();
    let b = second.service.clone();
    let one = std::thread::spawn(move || {
        a.claim_meeting_reminders_at(NOW)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
    });
    let two = std::thread::spawn(move || {
        b.claim_meeting_reminders_at(NOW)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
    });
    assert_eq!(one.join().unwrap() + two.join().unwrap(), 1);
    drop(first);
    drop(second);
    let reopened = rig(Arc::new(SqliteStore::open(&path).unwrap()));
    assert_eq!(
        reopened.service.prepare_calendar_meeting(&event()).unwrap()["id"],
        id
    );
    assert_eq!(
        reopened.service.claim_meeting_reminders_at(NOW).unwrap(),
        json!([])
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn restore_cannot_enable_reminders_or_repopulate_cached_events() {
    for target in stores() {
        let source = rig(Arc::new(MemoryStore::default()));
        call(
            &source.service,
            "backup_library",
            json!({"path":"fixture","includeAudio":false}),
        );
        let mut manifest = source.archive.backup.lock().unwrap().clone().unwrap();
        // A hand-edited/older producer cannot restore opt-in notification consent.
        manifest["settings"]["meetingReminders"] = json!(true);
        let r = rig(target);
        *r.archive.backup.lock().unwrap() = Some(manifest);
        call(
            &r.service,
            "restore_library",
            json!({"path":"fixture","expectedFingerprint":"a".repeat(64)}),
        );
        assert!(!r.service.meeting_reminders_enabled().unwrap());
        assert_eq!(
            r.service.get_meeting_day_at(NOW).unwrap()["events"],
            json!([])
        );
    }
}
#[test]
fn snapshots_filter_before_retaining_earliest_bounded_events() {
    for store in stores() {
        let r = rig(store);
        let values: Vec<_> = (0..300)
            .rev()
            .map(|n| {
                let mut value = event();
                value["id"] = json!(format!("device:{n}"));
                let start = tuck_core::meeting_day::instant(NOW).unwrap()
                    + chrono::Duration::minutes(n + 1);
                value["start"] = json!(start.to_rfc3339());
                value["end"] = json!((start + chrono::Duration::minutes(30)).to_rfc3339());
                value
            })
            .collect();
        let day = r
            .service
            .replace_calendar_snapshot("import", &values, NOW)
            .unwrap();
        assert_eq!(day["events"].as_array().unwrap().len(), 256);
        assert_eq!(day["events"][0]["event"]["id"], "device:0");
        assert_eq!(day["events"][255]["event"]["id"], "device:255");
    }
}
#[test]
fn reminder_claim_skips_busy_lifecycle_and_read_never_queues_behind_it() {
    use std::sync::mpsc::channel;
    for store in stores() {
        let r = rig(store);
        r.service.set_meeting_reminders_enabled(true).unwrap();
        r.service
            .replace_calendar_snapshot("import", &[event()], NOW)
            .unwrap();
        let (entered, observed) = channel();
        let (release, wait) = channel();
        *r.archive.backup_gate.lock().unwrap() = Some(Arc::new(ArchiveGate {
            entered,
            release: Mutex::new(wait),
        }));
        let s = r.service.clone();
        let backup = std::thread::spawn(move || {
            s.handle("backup_library", json!({"path":"fixture"}))
                .unwrap()
        });
        observed
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let s = r.service.clone();
        let (finished, receive) = channel();
        let probe = std::thread::spawn(move || {
            let claim = s.claim_meeting_reminders_at(NOW).unwrap();
            let day = s.get_meeting_day_at(NOW).unwrap();
            finished.send((claim, day)).unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(1));
        release.send(()).unwrap();
        backup.join().unwrap();
        probe.join().unwrap();
        let (claim, day) = result.expect("reminder read/claim must not wait for capture lifecycle");
        assert_eq!(claim, json!([]));
        assert_eq!(day["reminders"].as_array().unwrap().len(), 1);
        assert_eq!(
            r.service
                .claim_meeting_reminders_at(NOW)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
