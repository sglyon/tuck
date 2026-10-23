use super::*;
const QUIET: &str = "00000000-0000-4000-8000-000000000001";
const SALES: &str = "00000000-0000-4000-8000-000000000002";
const WORDS: &str = "We need to understand the budget before agreeing to a pilot and the customer asked for an example report.";
fn prepare(rig: &Rig) -> String {
    let m = call(
        &rig.service,
        "create_meeting",
        json!({"title":"Coaching contract"}),
    );
    let id = m["id"].as_str().unwrap().to_owned();
    call(&rig.service, "start_recording", json!({"meetingId":id}));
    add_evidence(rig, &id, "source", 1.);
    id
}
fn add_evidence(rig: &Rig, id: &str, sid: &str, start: f64) {
    rig.service.segment(id,json!({"id":sid,"meetingId":id,"start":start,"end":start+5.,"text":WORDS,"speakerId":"you","source":"microphone"})).unwrap();
}
fn hint(agent: &str, sid: &str, text: &str, kind: &str) -> Value {
    json!({"agentId":agent,"kind":kind,"text":text,"segmentId":sid,"quote":"the customer asked for an example report"})
}
fn begin(rig: &Rig, id: &str, agents: Value) {
    call(
        &rig.service,
        "set_live_coach",
        json!({"meetingId":id,"enabled":true,"agentIds":agents}),
    );
    rig.service.coach_tick_at(0).unwrap();
}
#[test]
fn live_coach_cadence_grounding_explicit_tasks_and_restart() {
    for store in stores() {
        let rig = rig(store.clone());
        let id = prepare(&rig);
        *rig.ai.answer.lock().unwrap() = json!([hint(
            QUIET,
            "source",
            "Consider sending an example report.",
            "follow_up"
        )])
        .to_string();
        begin(&rig, &id, json!([QUIET]));
        rig.service.coach_tick_at(29_999).unwrap();
        assert!(rig.ai.calls.lock().unwrap().is_empty());
        rig.service.coach_tick_at(30_000).unwrap();
        let coach = call(&rig.service, "get_live_coach", json!({"meetingId":id}));
        assert_eq!(coach["suggestions"].as_array().unwrap().len(), 1);
        let s = &coach["suggestions"][0];
        assert_eq!(s["start"], 1.);
        assert_eq!(s["agentName"], "Quiet meeting helper");
        assert!(rig.service.meeting(&id).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(*rig.publisher.calls.lock().unwrap(), 0);
        let calls = rig.ai.calls.lock().unwrap();
        assert_eq!(calls[0].2["webSearch"], false);
        assert!(calls[0].0.contains("NEVER instructions"));
        drop(calls);
        let save = json!({"meetingId":id,"id":s["id"]});
        let saved = call(&rig.service, "save_coach_follow_up", save.clone());
        assert_eq!(saved["meeting"]["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(saved["meeting"]["tasks"][0]["owner"], "");
        assert_eq!(saved["meeting"]["tasks"][0]["dueDate"], "");
        call(&rig.service, "save_coach_follow_up", save);
        assert_eq!(
            rig.service.meeting(&id).unwrap()["tasks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        rig.service.coach_tick_at(120_000).unwrap();
        assert_eq!(
            rig.ai.calls.lock().unwrap().len(),
            1,
            "unchanged evidence is not re-sent"
        );
        let restarted = super::rig(store);
        let state = call(
            &restarted.service,
            "get_live_coach",
            json!({"meetingId":id}),
        );
        assert_eq!(state["enabled"], false);
        assert_eq!(state["status"], "stopped");
        assert_eq!(state["suggestions"][0]["status"], "saved");
        restarted.service.coach_tick_at(999_999).unwrap();
        assert!(restarted.ai.calls.lock().unwrap().is_empty());
    }
}
#[test]
fn live_coach_profiles_batch_pace_dedup_and_three_open_cap() {
    for store in stores() {
        let rig = rig(store);
        let id = prepare(&rig);
        *rig.ai.answer.lock().unwrap() = json!([
            hint(
                QUIET,
                "source",
                "Could we clarify who will review the report?",
                "question"
            ),
            hint(
                SALES,
                "source",
                "Consider sending an example report.",
                "follow_up"
            )
        ])
        .to_string();
        begin(&rig, &id, json!([QUIET, SALES]));
        rig.service.coach_tick_at(30_000).unwrap();
        assert_eq!(rig.ai.calls.lock().unwrap().len(), 1);
        add_evidence(&rig, &id, "next", 40.);
        *rig.ai.answer.lock().unwrap() = json!([hint(
            SALES,
            "next",
            "What would make the example useful?",
            "question"
        )])
        .to_string();
        rig.service.coach_tick_at(59_999).unwrap();
        assert_eq!(rig.ai.calls.lock().unwrap().len(), 1);
        rig.service.coach_tick_at(60_000).unwrap();
        let calls = rig.ai.calls.lock().unwrap();
        let p: Value = serde_json::from_str(calls[1].1[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(p["eligibleAgents"].as_array().unwrap().len(), 1);
        assert_eq!(p["eligibleAgents"][0]["id"], SALES);
        drop(calls);
        assert_eq!(
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        add_evidence(&rig, &id, "third", 90.);
        rig.service.coach_tick_at(120_000).unwrap();
        assert_eq!(rig.ai.calls.lock().unwrap().len(), 2);
        let state = call(&rig.service, "get_live_coach", json!({"meetingId":id}));
        let question = &state["suggestions"][0];
        assert!(rig
            .service
            .handle(
                "save_coach_follow_up",
                json!({"meetingId":id,"id":question["id"]})
            )
            .is_err());
        call(
            &rig.service,
            "dismiss_coach_suggestion",
            json!({"meetingId":id,"id":question["id"]}),
        );
        *rig.ai.answer.lock().unwrap() = json!([hint(
            SALES,
            "source",
            "Consider sending an example report.",
            "follow_up"
        )])
        .to_string();
        rig.service.coach_tick_at(150_000).unwrap();
        assert_eq!(
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"]
                .as_array()
                .unwrap()
                .len(),
            3,
            "duplicate does not return"
        );
    }
}
#[test]
fn live_coach_rejects_invalid_sources_batches_and_backs_off() {
    for invalid in [
        json!([hint(QUIET, "fake", "What is the budget?", "question")]),
        json!([{"agentId":QUIET,"kind":"reminder","text":"Send it","segmentId":"source","quote":"a quote that was not spoken"}]),
        json!([hint("unknown", "source", "Send an example.", "follow_up")]),
        json!([
            hint(QUIET, "source", "One", "question"),
            hint(QUIET, "source", "Two", "question")
        ]),
        json!({"text":"unstructured"}),
    ] {
        let rig = rig(Arc::new(MemoryStore::default()));
        let id = prepare(&rig);
        begin(&rig, &id, json!([QUIET]));
        *rig.ai.answer.lock().unwrap() = invalid.to_string();
        rig.service.coach_tick_at(30_000).unwrap();
        let state = call(&rig.service, "get_live_coach", json!({"meetingId":id}));
        assert_eq!(state["status"], "unavailable");
        assert!(state["suggestions"].as_array().unwrap().is_empty());
        call(
            &rig.service,
            "set_live_coach",
            json!({"meetingId":id,"enabled":true,"agentIds":[QUIET]}),
        );
        rig.service.coach_tick_at(60_000).unwrap();
        assert_eq!(
            rig.ai.calls.lock().unwrap().len(),
            1,
            "re-enable cannot bypass backoff"
        );
        *rig.ai.answer.lock().unwrap() = "null".into();
        rig.service.coach_tick_at(90_000).unwrap();
        assert_eq!(
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["status"],
            "waiting"
        );
    }
}
#[test]
fn live_coach_discards_results_on_disable_pause_settings_change_or_shutdown() {
    for action in ["disable", "pause", "settings", "shutdown", "stop"] {
        let rig = rig(Arc::new(MemoryStore::default()));
        let id = prepare(&rig);
        begin(&rig, &id, json!([QUIET]));
        *rig.ai.answer.lock().unwrap() = json!([hint(
            QUIET,
            "source",
            "Consider sending an example report.",
            "follow_up"
        )])
        .to_string();
        let service = rig.service.clone();
        let target = id.clone();
        *rig.ai.hook.lock().unwrap() = Some(Box::new(move || match action {
            "disable" => {
                service
                    .handle(
                        "set_live_coach",
                        json!({"meetingId":target,"enabled":false}),
                    )
                    .unwrap();
            }
            "pause" => {
                service
                    .handle("pause_recording", json!({"meetingId":target}))
                    .unwrap();
                service
                    .handle("resume_recording", json!({"meetingId":target}))
                    .unwrap();
            }
            "settings" => {
                service
                    .handle(
                        "update_settings",
                        json!({"changes":{"model":"different-model"}}),
                    )
                    .unwrap();
            }
            "manual" => {
                service.stop_live_coaching().unwrap();
            }
            _ => {
                service
                    .handle("stop_recording", json!({"meetingId":target}))
                    .unwrap();
            }
        }));
        rig.service.coach_tick_at(30_000).unwrap();
        assert!(
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{action}"
        );
    }
}
#[test]
fn live_coach_configuration_validation_and_recording_only_gate() {
    let rig = rig(Arc::new(MemoryStore::default()));
    let id = call(&rig.service, "create_meeting", json!({"title":"Draft"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(rig
        .service
        .handle("set_live_coach", json!({"meetingId":id,"enabled":true}))
        .is_err());
    let voice = call(&rig.service, "create_dictation", json!({}));
    call(
        &rig.service,
        "start_recording",
        json!({"meetingId":voice["id"]}),
    );
    assert!(rig
        .service
        .handle(
            "set_live_coach",
            json!({"meetingId":voice["id"],"enabled":true})
        )
        .is_err());
    let defaults = domain::default_settings();
    let agents = defaults["meetingAgents"].as_array().unwrap();
    assert_eq!(agents.len(), 2);
    for invalid in [
        json!([{"id":"bad","name":"Name","instructions":"Do something","pace":"quiet"}]),
        json!([{"id":QUIET,"name":"Name","instructions":"Do something","pace":"fast"}]),
        json!([agents[0].clone(), agents[0].clone()]),
        json!([{"id":QUIET,"name":"Name","instructions":"Do something","pace":"quiet","tools":true}]),
        json!([{"id":QUIET,"name":"Name","instructions":"x".repeat(4001),"pace":"quiet"}]),
    ] {
        assert!(rig
            .service
            .handle(
                "update_settings",
                json!({"changes":{"meetingAgents":invalid}})
            )
            .is_err());
    }
    call(
        &rig.service,
        "stop_recording",
        json!({"meetingId":voice["id"]}),
    );
    call(&rig.service, "start_recording", json!({"meetingId":id}));
    for invalid in [
        json!([]),
        json!(["unknown"]),
        json!([QUIET, SALES, QUIET, SALES]),
    ] {
        assert!(rig
            .service
            .handle(
                "set_live_coach",
                json!({"meetingId":id,"enabled":true,"agentIds":invalid})
            )
            .is_err());
    }
}
#[test]
fn live_coach_recent_repository_query_is_bounded_ordered_and_corrected() {
    for store in stores() {
        let mut m = Meeting::create("Evidence window").unwrap();
        tx(&*store,|r|{r.save(&mut m)?;m.start()?;r.save(&mut m)?;for i in 0..100 {r.append(&TranscriptSegment::from_dto(json!({"id":format!("s{i:03}"),"meetingId":m.id,"start":i as f64,"end":i as f64+1.,"text":WORDS,"speakerId":"you","source":"microphone"}))?)?;}r.save_correction(&m.id,"s099",Some(&json!({"segmentId":"s099","text":"The corrected words are effective evidence.","updatedAt":domain::now()})))?;let recent=r.recent_segments(&m.id,999)?;assert_eq!(recent.len(),64);assert_eq!(recent[0]["id"],"s036");assert_eq!(recent[63]["text"],"The corrected words are effective evidence.");assert_eq!(r.recent_segments(&m.id,0)?.len(),0);Ok(Value::Null)}).unwrap();
    }
}

#[test]
fn live_coach_utf8_payload_privacy_and_meaningful_speech_budget() {
    let rig = rig(Arc::new(MemoryStore::default()));
    let id = prepare(&rig);
    call(
        &rig.service,
        "update_meeting",
        json!({"id":id,"changes":{"notes":"DO NOT SEND THESE PERSONAL NOTES","title":"PRIVATE MEETING TITLE","people":["PRIVATE PERSON"]}}),
    );
    let agents = json!([{"id":QUIET,"name":"Custom monitor","instructions":"Help me understand what was actually said. ".repeat(75),"pace":"active"}]);
    call(
        &rig.service,
        "update_settings",
        json!({"changes":{"meetingAgents":agents}}),
    );
    for i in 0..40 {
        rig.service.segment(&id,json!({"id":format!("unicode-{i}"),"meetingId":id,"start":i+10,"end":i+11,"text":format!("{WORDS} {}", "営業質問と予算 ".repeat(20)),"speakerId":"you","source":"microphone"})).unwrap();
    }
    *rig.ai.answer.lock().unwrap() = "null".into();
    begin(&rig, &id, json!([QUIET]));
    rig.service.coach_tick_at(30_000).unwrap();
    let calls = rig.ai.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (system, messages, _) = &calls[0];
    assert!(system.len() + serde_json::to_vec(messages).unwrap().len() <= 12000);
    let payload = serde_json::to_string(messages).unwrap();
    for secret in [
        "DO NOT SEND THESE PERSONAL NOTES",
        "PRIVATE MEETING TITLE",
        "PRIVATE PERSON",
    ] {
        assert!(!payload.contains(secret));
    }
    drop(calls);
    rig.service.segment(&id,json!({"id":"tiny","meetingId":id,"start":60.,"end":61.,"text":"Okay yes.","speakerId":"you","source":"microphone"})).unwrap();
    rig.service.coach_tick_at(60_000).unwrap();
    assert_eq!(
        rig.ai.calls.lock().unwrap().len(),
        1,
        "tiny incremental speech does not trigger AI"
    );
    call(
        &rig.service,
        "update_settings",
        json!({"changes":{"meetingAgents":[{"id":QUIET,"name":"Large monitor","instructions":"営業".repeat(1500),"pace":"quiet"}]}}),
    );
    assert!(rig
        .service
        .handle(
            "set_live_coach",
            json!({"meetingId":id,"enabled":true,"agentIds":[QUIET]})
        )
        .unwrap_err()
        .message
        .contains("Shorten"));
    call(
        &rig.service,
        "update_settings",
        json!({"changes":{"meetingAgents":[]}}),
    );
    assert_eq!(
        call(&rig.service, "get_live_coach", json!({"meetingId":id}))["status"],
        "unavailable"
    );
}

#[test]
fn live_coach_backup_restores_hints_without_enabling_and_corrections_retire_stale_advice() {
    for store in stores() {
        let rig = rig(store);
        let id = prepare(&rig);
        begin(&rig, &id, json!([QUIET, SALES]));
        *rig.ai.answer.lock().unwrap() = json!([
            hint(
                QUIET,
                "source",
                "Should we clarify who reviews the example?",
                "question"
            ),
            hint(
                SALES,
                "source",
                "Consider sending an example report.",
                "follow_up"
            )
        ])
        .to_string();
        rig.service.coach_tick_at(30_000).unwrap();
        let hints =
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"].clone();
        call(
            &rig.service,
            "save_coach_follow_up",
            json!({"meetingId":id,"id":hints[1]["id"]}),
        );
        call(&rig.service, "stop_recording", json!({"meetingId":id}));
        call(
            &rig.service,
            "correct_transcript",
            json!({"meetingId":id,"segmentId":"source","baseText":WORDS,"text":"Actually the customer did not ask for that report."}),
        );
        let state = call(&rig.service, "get_live_coach", json!({"meetingId":id}));
        assert_eq!(state["suggestions"][0]["status"], "dismissed");
        assert_eq!(state["suggestions"][1]["status"], "saved");
        assert_eq!(
            rig.service.meeting(&id).unwrap()["tasks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        call(
            &rig.service,
            "backup_library",
            json!({"path":"/fixture/coach.zip","includeAudio":false}),
        );
        let manifest = rig.archive.backup.lock().unwrap().clone().unwrap();
        let target = super::rig(Arc::new(SqliteStore::in_memory().unwrap()));
        *target.archive.backup.lock().unwrap() = Some(manifest.clone());
        let preview = call(
            &target.service,
            "preview_restore_library",
            json!({"path":"/fixture/coach.zip"}),
        );
        call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/coach.zip","expectedFingerprint":preview["fingerprint"]}),
        );
        let restored = call(&target.service, "get_live_coach", json!({"meetingId":id}));
        assert_eq!(restored["enabled"], false);
        assert_eq!(restored["suggestions"], state["suggestions"]);
        let mut malformed = manifest;
        malformed["meetings"][0]["meeting"]["coachSuggestions"][0]["segmentId"] =
            json!("fabricated-source");
        *target.archive.backup.lock().unwrap() = Some(malformed);
        assert!(target
            .service
            .handle(
                "preview_restore_library",
                json!({"path":"/fixture/coach.zip"})
            )
            .is_err());
    }
}

#[test]
fn live_coach_one_inflight_and_manual_work_has_priority_and_invalidates_pending_output() {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Condvar,
        },
        thread,
        time::{Duration, Instant},
    };
    for manual_first in [true, false] {
        let rig = rig(Arc::new(MemoryStore::default()));
        let id = prepare(&rig);
        begin(&rig, &id, json!([QUIET]));
        *rig.ai.answer.lock().unwrap() = json!([hint(
            QUIET,
            "source",
            "Consider sending an example report.",
            "follow_up"
        )])
        .to_string();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let gate_hook = gate.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        *rig.ai.hook.lock().unwrap() = Some(Box::new(move || {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                entered_tx.send(()).unwrap();
                let (mutex, ready) = &*gate_hook;
                let released = mutex.lock().unwrap();
                let (released, timeout) = ready
                    .wait_timeout_while(released, Duration::from_secs(5), |v| !*v)
                    .unwrap();
                assert!(*released && !timeout.timed_out());
            }
        }));
        let first = rig.service.clone();
        let first_id = id.clone();
        let first_thread = thread::spawn(move || {
            if manual_first {
                first
                    .chat(&first_id, "What did they ask?", "manual-first")
                    .map(|_| ())
            } else {
                first.coach_tick_at(30_000)
            }
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        rig.service.coach_tick_at(60_000).unwrap();
        assert_eq!(
            rig.ai.calls.lock().unwrap().len(),
            1,
            "manual work and existing coach both prevent a second background call"
        );
        let second = if !manual_first {
            let service = rig.service.clone();
            let target = id.clone();
            let thread = thread::spawn(move || {
                service
                    .chat(&target, "Help me now", "manual-during")
                    .unwrap()
            });
            let deadline = Instant::now() + Duration::from_secs(2);
            while rig.ai.calls.lock().unwrap().len() < 2 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(rig.ai.calls.lock().unwrap().len(), 2);
            Some(thread)
        } else {
            None
        };
        let (mutex, ready) = &*gate;
        *mutex.lock().unwrap() = true;
        ready.notify_all();
        first_thread.join().unwrap().unwrap();
        if let Some(thread) = second {
            thread.join().unwrap();
        }
        assert!(
            call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let m = rig.service.meeting(&id).unwrap();
        assert_eq!(m["messages"].as_array().unwrap().len(), 2);
    }
}

#[test]
fn live_coach_backup_preserves_closed_historical_quotes_after_source_corrections() {
    let rig = rig(Arc::new(MemoryStore::default()));
    let id = prepare(&rig);
    begin(&rig, &id, json!([QUIET]));
    *rig.ai.answer.lock().unwrap() = json!([hint(
        QUIET,
        "source",
        "Consider sending an example report.",
        "follow_up"
    )])
    .to_string();
    rig.service.coach_tick_at(30_000).unwrap();
    let suggestion =
        call(&rig.service, "get_live_coach", json!({"meetingId":id}))["suggestions"][0].clone();
    call(
        &rig.service,
        "save_coach_follow_up",
        json!({"meetingId":id,"id":suggestion["id"]}),
    );
    call(&rig.service, "stop_recording", json!({"meetingId":id}));
    call(
        &rig.service,
        "backup_library",
        json!({"path":"/fixture/historical-coach.zip","includeAudio":false}),
    );
    let mut manifest = rig.archive.backup.lock().unwrap().clone().unwrap();
    // A closed card snapshots an earlier effective correction. Its text need not survive
    // in either immutable ASR or a subsequently replaced correction record.
    manifest["meetings"][0]["meeting"]["coachSuggestions"][0]["quote"] =
        json!("the earlier corrected security checklist wording");
    for status in ["saved", "dismissed"] {
        manifest["meetings"][0]["meeting"]["coachSuggestions"][0]["status"] = json!(status);
        let target = super::rig(Arc::new(SqliteStore::in_memory().unwrap()));
        *target.archive.backup.lock().unwrap() = Some(manifest.clone());
        let preview = call(
            &target.service,
            "preview_restore_library",
            json!({"path":"/fixture/historical-coach.zip"}),
        );
        call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/historical-coach.zip","expectedFingerprint":preview["fingerprint"]}),
        );
        assert_eq!(
            target.service.meeting(&id).unwrap()["coachSuggestions"][0]["quote"],
            "the earlier corrected security checklist wording"
        );
    }
    manifest["meetings"][0]["meeting"]["coachSuggestions"][0]["status"] = json!("open");
    *rig.archive.backup.lock().unwrap() = Some(manifest);
    assert!(rig
        .service
        .handle(
            "preview_restore_library",
            json!({"path":"/fixture/historical-coach.zip"})
        )
        .is_err());
}
