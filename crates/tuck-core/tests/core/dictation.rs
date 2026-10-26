use super::*;

fn note(r: &Rig) -> String {
    call(&r.service, "create_dictation", json!({}))["id"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn dictation_reuses_recording_with_mic_only_no_automation_and_editable_plain_text() {
    for store in stores() {
        let r = rig(store);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"autoSummary":true,"notionAutoSync":true,"notionTarget":"target"}}),
        );
        let prefs = call(&r.service, "get_settings", json!({}));
        let id = note(&r);
        // Even a caller requesting system audio only cannot enable it for a voice note.
        call(
            &r.service,
            "start_recording",
            json!({"meetingId":id,"systemAudio":true,"microphone":false}),
        );
        assert_eq!(*r.capture.sources.lock().unwrap(), Some((false, true)));
        assert_eq!(
            r.capture.configured.lock().unwrap().as_ref().unwrap()["diarization"],
            false
        );
        let other = create(&r);
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":other}))
            .is_err());
        call(&r.service, "pause_recording", json!({"meetingId":id}));
        call(&r.service, "resume_recording", json!({"meetingId":id}));
        let finished = call(&r.service, "stop_recording", json!({"meetingId":id}));
        assert_eq!(finished["kind"], "dictation");
        assert_eq!(finished["notes"], "We agreed to ship on Friday.");
        assert_eq!(finished["summary"], "");
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Edited voice note."}}),
        );
        assert_eq!(
            r.service.meeting(&id).unwrap()["segments"][0]["text"],
            "We agreed to ship on Friday."
        );
        assert!(r.ai.calls.lock().unwrap().is_empty());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 0);
        assert_eq!(call(&r.service, "get_settings", json!({})), prefs);
        let page = call(&r.service, "list_meetings", json!({}));
        assert!(page["meetings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == id && m["kind"] == "dictation"));
        // Configuring a subsequent meeting restores default speaker behavior.
        call(&r.service, "start_recording", json!({"meetingId":other}));
        assert_eq!(
            r.capture.configured.lock().unwrap().as_ref().unwrap()["diarization"],
            Value::Null
        );
        // Keep this test free of intentionally enabled meeting post-processing.
        call(&r.service, "shutdown", json!({}));
    }
}

#[test]
fn dictation_preserves_typed_notes_and_failed_starts_partial_evidence() {
    for store in stores() {
        let r = rig(store);
        let id = note(&r);
        *r.capture.fail_start.lock().unwrap() = true;
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":id}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["status"], "draft");
        assert_eq!(r.service.meeting(&id).unwrap()["notes"], "");
        *r.capture.partial_start.lock().unwrap() = true;
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":id}))
            .is_err());
        let partial = r.service.meeting(&id).unwrap();
        assert_eq!(partial["status"], "interrupted");
        assert_eq!(partial["notes"], "We agreed to ship on Friday.");
        assert_eq!(partial["segments"].as_array().unwrap().len(), 1);
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":id}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["notes"], partial["notes"]);
        let typed = note(&r);
        *r.capture.fail_start.lock().unwrap() = false;
        *r.capture.partial_start.lock().unwrap() = false;
        call(&r.service, "start_recording", json!({"meetingId":typed}));
        call(
            &r.service,
            "update_meeting",
            json!({"id":typed,"changes":{"notes":"My typed text stays."}}),
        );
        assert_eq!(
            call(&r.service, "stop_recording", json!({"meetingId":typed}))["notes"],
            "My typed text stays."
        );
        assert!(r.ai.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn dictation_shutdown_startup_and_audio_recovery_preserve_kind_and_text() {
    for store in stores() {
        let r = rig(store.clone());
        let id = note(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        call(&r.service, "shutdown", json!({}));
        let stopped = r.service.meeting(&id).unwrap();
        assert_eq!(stopped["status"], "interrupted");
        assert_eq!(stopped["notes"], "We agreed to ship on Friday.");
        let recovered = call(
            &r.service,
            "recover_audio",
            json!({"meetingId":id,"audioId":"microphone.wav"}),
        );
        assert_eq!(recovered["kind"], "dictation");
        assert_eq!(recovered["recoveredFrom"], id);
        assert_eq!(recovered["notes"], stopped["notes"]);
        assert_eq!(
            r.capture.configured.lock().unwrap().as_ref().unwrap()["diarization"],
            false
        );
        assert_eq!(r.service.meeting(&id).unwrap(), stopped);
        // Simulate a process exit with durable active state and evidence but no stop.
        let crashed = note(&r);
        call(&r.service, "start_recording", json!({"meetingId":crashed}));
        r.service
            .segment(&crashed, segment(&crashed, "crash-evidence"))
            .unwrap();
        let reopened = rig(store);
        assert_eq!(
            reopened.service.meeting(&crashed).unwrap()["status"],
            "interrupted"
        );
        assert_eq!(
            reopened.service.meeting(&crashed).unwrap()["notes"],
            "We agreed to ship on Friday."
        );
        assert!(r.ai.calls.lock().unwrap().is_empty());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 0);
    }
}

#[test]
fn dictation_backup_roundtrip_and_legacy_default_are_storage_independent() {
    for store in stores() {
        let r = rig(store);
        let id = note(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        call(&r.service, "stop_recording", json!({"meetingId":id}));
        let legacy = create(&r);
        call(
            &r.service,
            "backup_library",
            json!({"path":"/fixture/notes.zip"}),
        );
        let mut manifest = r.archive.backup.lock().unwrap().clone().unwrap();
        for entry in manifest["meetings"].as_array_mut().unwrap() {
            if entry["meeting"]["id"] == legacy {
                entry["meeting"].as_object_mut().unwrap().remove("kind");
            }
        }
        let target = rig(Arc::new(SqliteStore::in_memory().unwrap()));
        *target.archive.backup.lock().unwrap() = Some(manifest);
        call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/notes.zip","expectedFingerprint":"a".repeat(64)}),
        );
        assert_eq!(target.service.meeting(&id).unwrap()["kind"], "dictation");
        assert_eq!(
            target.service.meeting(&id).unwrap()["notes"],
            "We agreed to ship on Friday."
        );
        assert_eq!(target.service.meeting(&legacy).unwrap()["kind"], "meeting");
    }
}

#[test]
fn explicit_dictation_cleanup_preserves_evidence_and_rejects_rewrites_or_note_edits() {
    for store in stores() {
        let r = rig(store);
        let text = "We agreed to ship on Friday.";
        assert!(r.service.format_dictation(text, "Format paragraphs.").is_err());
        let mut settings = call(&r.service, "get_settings", json!({}))["globalDictation"].clone();
        settings["cleanupEnabled"] = json!(true);
        call(&r.service, "update_settings", json!({"changes":{"globalDictation":settings}}));
        let id = note(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        call(&r.service, "stop_recording", json!({"meetingId":id}));
        *r.ai.answer.lock().unwrap() = "We agreed to ship\non Friday!".into();
        let cleaned = r.service.format_dictation(text, "Use short paragraphs.").unwrap();
        r.service.save_dictation_cleanup(&id, text, &cleaned).unwrap();
        let saved = r.service.meeting(&id).unwrap();
        assert_eq!(saved["notes"], cleaned);
        assert_eq!(saved["segments"][0]["text"], text);
        let request = r.ai.calls.lock().unwrap().last().unwrap().clone();
        assert!(request.0.contains("Use short paragraphs."));
        assert_eq!(request.1, vec![json!({"role":"user","content":text})]);
        *r.ai.answer.lock().unwrap() = "We agreed to ship on Saturday.".into();
        assert!(r.service.format_dictation(text, "Format paragraphs.").is_err());
        assert!(r.service.save_dictation_cleanup(&id, text, &cleaned).is_err(), "must not overwrite edited notes");
        let before = r.ai.calls.lock().unwrap().len();
        call(&r.service, "update_settings", json!({"changes":{"provider":"openai","hostedConsent":false}}));
        assert!(r.service.format_dictation(text, "Format paragraphs.").is_err());
        assert_eq!(r.ai.calls.lock().unwrap().len(), before);
    }
}
