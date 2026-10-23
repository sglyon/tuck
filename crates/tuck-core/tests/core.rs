use serde_json::{json, Value};
use std::sync::{Arc, Mutex, Weak};
use tuck_core::{
    domain::{self, Meeting, TranscriptSegment},
    ports::*,
    store::{memory::MemoryStore, sqlite::SqliteStore},
    Adapters, AppError, MeetingService,
};
fn tx(store: &dyn Store, mut f: impl FnMut(&mut dyn Repository) -> Result<Value>) -> Result<Value> {
    store.transaction(&mut f)
}
fn stores() -> Vec<Arc<dyn Store>> {
    vec![
        Arc::new(MemoryStore::default()),
        Arc::new(SqliteStore::in_memory().unwrap()),
    ]
}
fn segment(id: &str, n: &str) -> Value {
    json!({"id":n,"meetingId":id,"start":3.,"end":5.,"text":"We agreed to ship on Friday.","speakerId":"you","source":"microphone"})
}
#[test]
fn repository_contract() {
    for store in stores() {
        let mut m = Meeting::create("Contract").unwrap();
        tx(&*store, |r| {
            r.save(&mut m)?;
            Ok(Value::Null)
        })
        .unwrap();
        assert_eq!(m.version, 1);
        let mut stale = m.clone();
        m.notes = "new notes".into();
        tx(&*store, |r| {
            r.save(&mut m)?;
            Ok(Value::Null)
        })
        .unwrap();
        assert_eq!(
            tx(&*store, |r| {
                r.save(&mut stale)?;
                Ok(Value::Null)
            })
            .unwrap_err()
            .code,
            "conflict"
        );
        let s = TranscriptSegment::from_dto(segment(&m.id, "one")).unwrap();
        tx(&*store, |r| {
            assert!(r.append(&s)?);
            assert!(!r.append(&s)?);
            r.rename_speaker(&m.id, "you", "SGL")?;
            r.add_message(
                &m.id,
                &json!({"id":"msg","role":"user","content":"question","createdAt":"2026-01-01"}),
            )?;
            r.add_bookmark(&m.id, &json!({"id":"b","time":4.,"label":"Decision"}))?;
            assert_eq!(r.segments(&m.id)?.len(), 1);
            assert_eq!(r.speakers(&m.id)?[0]["name"], "SGL");
            assert_eq!(r.messages(&m.id)?.len(), 1);
            assert_eq!(r.bookmarks(&m.id)?.len(), 1);
            Ok(Value::Null)
        })
        .unwrap();
        tx(&*store, |r| {
            let mut next = r.get(&m.id)?;
            next.title = "will rollback".into();
            r.save(&mut next)?;
            Err(AppError::invalid("rollback"))
        })
        .unwrap_err();
        tx(&*store, |r| {
            assert_eq!(r.get(&m.id)?.title, "Contract");
            assert_eq!(r.get(&m.id)?.notes, "new notes");
            r.delete(&m)?;
            assert!(r.get(&m.id).is_err());
            assert!(r.segments(&m.id)?.is_empty());
            assert!(r.speakers(&m.id)?.is_empty());
            assert!(r.messages(&m.id)?.is_empty());
            assert!(r.bookmarks(&m.id)?.is_empty());
            Ok(Value::Null)
        })
        .unwrap();
    }
}
#[derive(Default)]
struct Capture {
    callback: Mutex<Option<OnSegment>>,
    id: Mutex<String>,
    fail_start: Mutex<bool>,
    configured: Mutex<Option<Value>>,
    fail_configure: Mutex<bool>,
    partial_start: Mutex<bool>,
    fail_stop: Mutex<bool>,
    flush_on_pause: Mutex<bool>,
    shutdown_calls: Mutex<usize>,
}
impl RecordingPort for Capture {
    fn configure(&self, settings: &Value) -> Result<()> {
        *self.configured.lock().unwrap() = Some(settings.clone());
        if *self.fail_configure.lock().unwrap() {
            Err(AppError::unavailable("Device selection failed"))
        } else {
            Ok(())
        }
    }
    fn status(&self) -> Value {
        json!({"captureAvailable":true,"modelInstalled":true})
    }
    fn start(&self, id: &str, on: OnSegment, _: bool, _: bool, _: u32, _: bool) -> Result<()> {
        *self.id.lock().unwrap() = id.into();
        *self.callback.lock().unwrap() = Some(on.clone());
        if *self.partial_start.lock().unwrap() {
            on(segment(id, "partial"))?;
        }
        if *self.fail_start.lock().unwrap() {
            return Err(AppError::unavailable("Device unavailable"));
        }
        Ok(())
    }
    fn pause(&self) -> Result<()> {
        if *self.flush_on_pause.lock().unwrap() {
            if let Some(on) = self.callback.lock().unwrap().clone() {
                on(segment(&self.id.lock().unwrap(), "pause-flush"))?;
            }
        }
        Ok(())
    }
    fn resume(&self) -> Result<()> {
        Ok(())
    }
    fn stop(&self) -> Result<f64> {
        if let Some(on) = self.callback.lock().unwrap().clone() {
            on(segment(&self.id.lock().unwrap(), "drained"))?;
        }
        if *self.fail_stop.lock().unwrap() {
            Err(AppError::unavailable("Transcription stopped"))
        } else {
            Ok(6.)
        }
    }
    fn shutdown(&self) -> Result<()> {
        *self.shutdown_calls.lock().unwrap() += 1;
        Ok(())
    }
    fn install_model(&self, kind: &str) -> Result<Value> {
        Ok(json!({"kind":kind}))
    }
    fn unload(&self) -> Result<()> {
        Ok(())
    }
    fn import_file(&self, _: &str, id: &str, on: OnSegment, _: u32) -> Result<f64> {
        on(segment(id, "import"))?;
        Ok(6.)
    }
}
#[derive(Default)]
struct AI {
    hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    calls: Mutex<Vec<(String, Vec<Value>, Value)>>,
    answer: Mutex<String>,
    fail: Mutex<bool>,
}
impl IntelligencePort for AI {
    fn models(&self, _: &Value) -> Result<Vec<Value>> {
        Ok(vec![json!({"id":"local-model","name":"Local Model"})])
    }
    fn complete(
        &self,
        settings: &Value,
        system: &str,
        messages: &[Value],
        on: &mut dyn FnMut(&str),
    ) -> Result<String> {
        if *self.fail.lock().unwrap() {
            return Err(AppError::unavailable("No AI model available."));
        }
        self.calls
            .lock()
            .unwrap()
            .push((system.into(), messages.into(), settings.clone()));
        if let Some(hook) = self.hook.lock().unwrap().as_ref() {
            hook();
        }
        on("answer");
        let answer = self.answer.lock().unwrap();
        Ok(if answer.is_empty() {
            "Summary with evidence [00:03]".into()
        } else {
            answer.clone()
        })
    }
}
#[derive(Default)]
struct Publisher {
    uncertain: Mutex<bool>,
    bad_url: Mutex<bool>,
    calls: Mutex<usize>,
    settings: Mutex<Vec<Value>>,
    recoveries: Mutex<Vec<(String, Value, String)>>,
}
impl PublicationPort for Publisher {
    fn recover(&self, target: &str, meeting: &Value, address: &str) -> Result<String> {
        self.recoveries
            .lock()
            .unwrap()
            .push((target.into(), meeting.clone(), address.into()));
        if *self.bad_url.lock().unwrap() {
            return Err(AppError::invalid(
                "This page does not belong to this meeting.",
            ));
        }
        Ok(address.into())
    }
    fn publish_with_settings(
        &self,
        target: &str,
        meeting: &Value,
        settings: &Value,
    ) -> Result<String> {
        self.settings.lock().unwrap().push(settings.clone());
        self.publish(target, meeting)
    }
    fn destinations(&self, query: &str) -> Result<Value> {
        Ok(
            json!({"destinations":[{"id":"db","title":query,"url":"https://www.notion.so/db"}],"hasMore":false}),
        )
    }
    fn destination(&self, target: &str) -> Result<Value> {
        Ok(json!({"id":target,"title":"Meetings","properties":[]}))
    }

    fn publish(&self, _: &str, _: &Value) -> Result<String> {
        *self.calls.lock().unwrap() += 1;
        if *self.uncertain.lock().unwrap() {
            Err(AppError::new(
                "publish_uncertain",
                "Check Notion before retrying",
            ))
        } else if *self.bad_url.lock().unwrap() {
            Ok("https://attacker.test/page".into())
        } else {
            Ok("https://www.notion.so/page".into())
        }
    }
}
#[derive(Default)]
struct Secrets(Mutex<std::collections::HashMap<String, String>>);
impl SecretPort for Secrets {
    fn get(&self, k: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<()> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<()> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
struct Accounts;
impl AccountPort for Accounts {
    fn accounts(&self) -> Result<Vec<Value>> {
        Ok(vec![])
    }
    fn sign_in(&self, _: Option<&str>) -> Result<Vec<Value>> {
        Err(AppError::unavailable("No browser"))
    }
    fn select(&self, _: &str) -> Result<Vec<Value>> {
        Err(AppError::invalid("Unknown account"))
    }
    fn sign_out(&self, _: &str) -> Result<Value> {
        Ok(json!({"accounts":[],"remoteRevoked":false}))
    }
}
struct ArchiveGate {
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
#[derive(Default)]
struct Archive {
    backup_gate: Mutex<Option<Arc<ArchiveGate>>>,
    backup: Mutex<Option<Value>>,
    restore_maps: Mutex<Vec<Value>>,
    discarded: Mutex<usize>,
    fail_restore_audio: Mutex<bool>,
    removed: Mutex<Vec<String>>,
    exports: Mutex<Vec<(String, String)>>,
}
impl AudioArchivePort for Archive {
    fn list(&self, _: &str) -> Result<Vec<Value>> {
        Ok(vec![
            json!({"id":"microphone.wav","name":"Microphone","bytes":320044,"duration":10}),
        ])
    }
    fn resolve(&self, _: &str, audio_id: &str) -> Result<String> {
        if audio_id != "microphone.wav" {
            return Err(AppError::invalid("Missing audio"));
        }
        Ok("/fixture/source.wav".into())
    }
    fn write_backup(&self, _: &str, manifest: &Value, audio: bool) -> Result<Value> {
        let gate = self.backup_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.entered
                .send(())
                .map_err(|_| AppError::invalid("Fixture observer closed"))?;
            gate.release
                .lock()
                .unwrap()
                .recv()
                .map_err(|_| AppError::invalid("Fixture archive gate closed"))?;
        }
        *self.backup.lock().unwrap() = Some(manifest.clone());
        Ok(json!({"audioFiles":if audio {1}else{0},"warnings":[]}))
    }
    fn read_backup(&self, _: &str) -> Result<Value> {
        Ok(
            json!({"manifest":self.backup.lock().unwrap().clone().ok_or_else(||AppError::invalid("Missing backup"))?,"stagingToken":"fixture-stage","fingerprint":"a".repeat(64),"audioCount":1,"warnings":[]}),
        )
    }
    fn commit_restore_audio(&self, _: &str, map: &Value) -> Result<Value> {
        self.restore_maps.lock().unwrap().push(map.clone());
        if *self.fail_restore_audio.lock().unwrap() {
            Err(AppError::unavailable("Disk is full"))
        } else {
            Ok(json!({"audioFilesRestored":map.as_object().unwrap().len(),"warnings":[]}))
        }
    }
    fn discard_restore(&self, _: &str) -> Result<()> {
        *self.discarded.lock().unwrap() += 1;
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<()> {
        self.removed.lock().unwrap().push(id.into());
        Ok(())
    }
    fn export(&self, path: &str, content: &str) -> Result<()> {
        self.exports
            .lock()
            .unwrap()
            .push((path.into(), content.into()));
        Ok(())
    }
}
struct Rig {
    service: Arc<MeetingService>,
    capture: Arc<Capture>,
    ai: Arc<AI>,
    publisher: Arc<Publisher>,
    archive: Arc<Archive>,
    events: Arc<Mutex<Vec<(String, Value)>>>,
}
fn rig(store: Arc<dyn Store>) -> Rig {
    let capture = Arc::new(Capture::default());
    let ai = Arc::new(AI::default());
    let publisher = Arc::new(Publisher::default());
    let archive = Arc::new(Archive::default());
    let events = Arc::new(Mutex::new(Vec::new()));
    let saved = events.clone();
    let event_store = store.clone();
    let service = MeetingService::new(
        store,
        Adapters {
            capture: capture.clone(),
            intelligence: ai.clone(),
            publisher: publisher.clone(),
            secrets: Arc::new(Secrets::default()),
            accounts: Arc::new(Accounts),
            archive: archive.clone(),
        },
        Arc::new(move |name, value| {
            if name == "segment" {
                let id = value["meetingId"].as_str().unwrap();
                tx(&*event_store, |r| {
                    assert!(r
                        .segments(id)?
                        .iter()
                        .any(|s| s["id"] == value["segment"]["id"]));
                    Ok(Value::Null)
                })
                .unwrap();
            }
            if name == "meeting_updated" {
                tx(&*event_store, |r| {
                    assert_eq!(
                        r.get(value["meeting"]["id"].as_str().unwrap())?.notes,
                        value["meeting"]["notes"]
                    );
                    Ok(Value::Null)
                })
                .unwrap();
            }
            saved.lock().unwrap().push((name.into(), value));
        }),
    )
    .unwrap();
    Rig {
        service,
        capture,
        ai,
        publisher,
        archive,
        events,
    }
}
fn call(s: &Arc<MeetingService>, method: &str, p: Value) -> Value {
    s.handle(method, p).unwrap()
}
fn create(r: &Rig) -> String {
    call(&r.service, "create_meeting", json!({"title":"Meeting"}))["id"]
        .as_str()
        .unwrap()
        .into()
}
#[test]
fn recording_lifecycle_drains_evidence_before_complete() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        assert!(r
            .service
            .handle(
                "start_recording",
                json!({"meetingId":id,"systemAudio":false,"microphone":false})
            )
            .is_err());
        call(&r.service, "start_recording", json!({"meetingId":id}));
        assert!(r
            .service
            .handle("delete_meeting", json!({"id":id}))
            .is_err());
        let other = create(&r);
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":other}))
            .is_err());
        call(&r.service, "pause_recording", json!({"meetingId":id}));
        call(&r.service, "resume_recording", json!({"meetingId":id}));
        let complete = call(&r.service, "stop_recording", json!({"meetingId":id}));
        assert_eq!(complete["status"], "complete");
        assert_eq!(complete["duration"], 6.);
        assert_eq!(complete["segments"].as_array().unwrap().len(), 1);
        assert_eq!(complete["speakers"][0]["name"], "You");
        assert!(r.service.segment(&id, segment(&id, "late")).is_err());
        assert!(r.events.lock().unwrap().iter().any(|(n, _)| n == "segment"));
    }
}
#[test]
fn failed_capture_preserves_partial_evidence() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        *r.capture.fail_start.lock().unwrap() = true;
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":id}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["status"], "draft");
        *r.capture.partial_start.lock().unwrap() = true;
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":id}))
            .is_err());
        let m = r.service.meeting(&id).unwrap();
        assert_eq!(m["status"], "interrupted");
        assert_eq!(m["segments"].as_array().unwrap().len(), 1);
        assert!(r.archive.removed.lock().unwrap().is_empty());
    }
}
#[test]
fn stop_failure_retains_audio_and_marks_interrupted() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"keepAudio":false}}),
        );
        call(&r.service, "start_recording", json!({"meetingId":id}));
        *r.capture.fail_stop.lock().unwrap() = true;
        assert!(r
            .service
            .handle("stop_recording", json!({"meetingId":id}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["status"], "interrupted");
        assert!(r.archive.removed.lock().unwrap().is_empty());
        assert_eq!(
            r.service.get_status().unwrap()["activeMeetingId"],
            Value::Null
        );
    }
}
#[test]
fn audio_retention_uses_start_snapshot() {
    let r = rig(Arc::new(MemoryStore::default()));
    let id = create(&r);
    call(&r.service, "start_recording", json!({"meetingId":id}));
    call(
        &r.service,
        "update_settings",
        json!({"changes":{"keepAudio":false}}),
    );
    call(&r.service, "stop_recording", json!({"meetingId":id}));
    assert!(r.archive.removed.lock().unwrap().is_empty());
    let imported = call(&r.service, "import_audio", json!({"path":"chosen.wav"}));
    assert_eq!(imported["status"], "complete");
    assert_eq!(r.archive.removed.lock().unwrap().len(), 1);
}
#[test]
fn summaries_reload_metadata_and_block_deletion_during_ai() {
    for store in stores() {
        let r = rig(store);
        let m = call(&r.service, "import_audio", json!({"path":"chosen.wav"}));
        let id = m["id"].as_str().unwrap().to_owned();
        let weak: Weak<MeetingService> = Arc::downgrade(&r.service);
        let target = id.clone();
        *r.ai.hook.lock().unwrap() = Some(Box::new(move || {
            let service = weak.upgrade().unwrap();
            call(
                &service,
                "update_meeting",
                json!({"id":target,"changes":{"notes":"edited during inference","title":"new title"}}),
            );
            assert!(service
                .handle("delete_meeting", json!({"id":target}))
                .is_err());
            assert!(service
                .handle("summarize", json!({"meetingId":target}))
                .is_err());
        }));
        let m = call(&r.service, "summarize", json!({"meetingId":id}));
        assert_eq!(m["notes"], "edited during inference");
        assert_eq!(m["title"], "new title");
        assert_eq!(m["summary"], "Summary with evidence [00:03]");
        assert_eq!(r.ai.calls.lock().unwrap()[0].2["webSearch"], false);
    }
}
#[test]
fn chat_persists_user_and_answer_and_streams_request_id() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        let message = call(
            &r.service,
            "chat",
            json!({"meetingId":id,"prompt":"What did we decide?","requestId":"request"}),
        );
        assert_eq!(message["role"], "assistant");
        let m = r.service.meeting(&id).unwrap();
        assert_eq!(m["messages"].as_array().unwrap().len(), 2);
        assert!(r
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(n, v)| n == "chat_delta" && v["requestId"] == "request"));
        assert!(r.ai.calls.lock().unwrap()[0]
            .0
            .contains("[No transcript yet]"));
    }
}
#[test]
fn uncertain_publication_blocks_duplicate_requests() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"publish these"}}),
        );
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":"target"}}),
        );
        *r.publisher.uncertain.lock().unwrap() = true;
        assert_eq!(
            r.service
                .handle("publish_notion", json!({"meetingId":id}))
                .unwrap_err()
                .code,
            "publish_uncertain"
        );
        assert_eq!(r.service.meeting(&id).unwrap()["notionStatus"], "uncertain");
        assert!(r
            .service
            .handle("publish_notion", json!({"meetingId":id}))
            .is_err());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 1);
    }
}
#[test]
fn invalid_publication_address_is_uncertain_after_send() {
    let r = rig(Arc::new(MemoryStore::default()));
    let id = create(&r);
    call(
        &r.service,
        "update_meeting",
        json!({"id":id,"changes":{"notes":"publish these"}}),
    );
    call(
        &r.service,
        "update_settings",
        json!({"changes":{"notionTarget":"target"}}),
    );
    *r.publisher.bad_url.lock().unwrap() = true;
    assert!(r
        .service
        .handle("publish_notion", json!({"meetingId":id}))
        .is_err());
    assert_eq!(r.service.meeting(&id).unwrap()["notionStatus"], "uncertain");
}
#[test]
fn example_and_editing_and_exports_keep_wire_contract() {
    for store in stores() {
        let r = rig(store);
        let m = call(&r.service, "load_example", json!({}));
        assert_eq!(m["isExample"], true);
        assert_eq!(m["segments"].as_array().unwrap().len(), 9);
        assert_eq!(call(&r.service, "load_example", json!({}))["id"], m["id"]);
        assert!(r
            .service
            .handle("start_recording", json!({"meetingId":m["id"]}))
            .is_err());
        let id = m["id"].as_str().unwrap();
        let added = call(
            &r.service,
            "add_bookmark",
            json!({"meetingId":id,"time":20,"label":"test"}),
        );
        let bookmark = added["bookmarks"][0]["id"].clone();
        let removed = call(
            &r.service,
            "remove_bookmark",
            json!({"meetingId":id,"id":bookmark}),
        );
        assert_eq!(removed["bookmarks"].as_array().unwrap().len(), 1);
        call(
            &r.service,
            "rename_speaker",
            json!({"meetingId":id,"speakerId":"you","name":"SGL"}),
        );
        let srt = call(
            &r.service,
            "export_meeting",
            json!({"meetingId":id,"format":"srt"}),
        );
        assert!(srt["content"]
            .as_str()
            .unwrap()
            .contains("00:00:00,000 --> 00:00:18,000\nSGL:"));
        let md = call(
            &r.service,
            "export_meeting",
            json!({"meetingId":id,"path":"chosen.md"}),
        );
        assert!(md["content"]
            .as_str()
            .unwrap()
            .contains("Illustrative example"));
        assert_eq!(r.archive.exports.lock().unwrap().len(), 1);
        let export = call(
            &r.service,
            "export_meeting",
            json!({"meetingId":id,"format":"json"}),
        );
        assert!(
            serde_json::from_str::<Value>(export["content"].as_str().unwrap())
                .unwrap()
                .get("version")
                .is_none()
        );
    }
}
#[test]
fn startup_recovers_recording_and_inflight_publication() {
    for store in stores() {
        let mut m = Meeting::create("Recover").unwrap();
        m.start().unwrap();
        m.notion_status = "syncing".into();
        tx(&*store, |r| {
            r.save(&mut m)?;
            Ok(Value::Null)
        })
        .unwrap();
        let r = rig(store);
        let m = r.service.meeting(&m.id).unwrap();
        assert_eq!(m["status"], "interrupted");
        assert_eq!(m["notionStatus"], "uncertain");
    }
}
#[test]
fn settings_validate_and_secrets_never_enter_sql() {
    for store in stores() {
        let r = rig(store.clone());
        call(
            &r.service,
            "save_secret",
            json!({"kind":"openai","value":"private-key"}),
        );
        assert_eq!(r.service.get_settings().unwrap()["hasOpenAIKey"], true);
        tx(&*store, |repo| {
            assert!(repo.settings()?.get("hasOpenAIKey").is_none());
            assert!(!repo.settings()?.to_string().contains("private-key"));
            Ok(Value::Null)
        })
        .unwrap();
        for changes in [
            json!({"provider":"bad"}),
            json!({"threads":true}),
            json!({"threads":9}),
            json!({"localUrl":"https://localhost"}),
            json!({"localUrl":"http://localhost.evil.test"}),
            json!({"localUrl":"http://user@localhost"}),
            json!({"webSearch":"yes"}),
            json!({"apiKey":"secret"}),
        ] {
            assert!(r
                .service
                .handle("update_settings", json!({"changes":changes}))
                .is_err());
        }
        assert_eq!(
            call(
                &r.service,
                "update_settings",
                json!({"changes":{"localUrl":"http://[::1]:11434"}})
            )["localUrl"],
            "http://[::1]:11434"
        );
        assert!(r.service.handle("not_a_method", json!({})).is_err());
        assert!(r
            .service
            .handle(
                "update_meeting",
                json!({"id":create(&r),"changes":{"notionStatus":"synced"}})
            )
            .is_err());
    }
}
#[test]
fn evidence_chunking_preserves_all_unicode_and_citations() {
    let transcript = "résumé 世界 ".repeat(10_000);
    let m = json!({"title":"Long meeting","notes":"private notes","speakers":[{"id":"you","name":"SGL"}],"segments":[{"start":192,"speakerId":"you","text":transcript}]});
    let chunks = tuck_core::service::evidence_chunks(&m, 24000);
    assert!(chunks.len() > 1);
    assert!(chunks
        .iter()
        .all(|c| c.chars().count() <= 24000 && c.starts_with("[03:12] SGL: ")));
    let recovered = chunks
        .iter()
        .flat_map(|c| c.lines())
        .map(|line| line.strip_prefix("[03:12] SGL: ").unwrap())
        .collect::<String>();
    assert_eq!(recovered, transcript);
    assert!(tuck_core::service::context_for(&m, 14000).contains("Earlier transcript omitted"));
}
#[test]
fn domain_rejects_invalid_evidence_and_atomic_edits() {
    let mut m = Meeting::create("Title").unwrap();
    assert!(m.update(&json!({"title":"new","starred":"true"})).is_err());
    assert_eq!(m.title, "Title");
    assert!(TranscriptSegment::from_dto(json!({"id":"s","meetingId":"m","start":2,"end":1,"source":"microphone","text":"x","speakerId":"you"})).is_err());
    assert!(domain::seconds(&json!(true)).is_err());
    assert!(domain::seconds(&json!(-1)).is_err());
}

#[test]
fn opens_python_schema_v1_without_rewriting_documents() {
    let root = std::env::temp_dir().join(format!("tuck-core-{}", domain::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("tuck.sqlite3");
    let document = json!({"id":"python-id","title":"From Python","createdAt":"2026-10-01T12:00:00+00:00","duration":17.5,"status":"complete","notes":"Keep my notes","summary":"A decision [00:03]","template":"general","starred":true,"isExample":false,"notionUrl":"","notionStatus":"idle"});
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE meetings(id TEXT PRIMARY KEY,created_at TEXT NOT NULL,document TEXT NOT NULL,version INTEGER NOT NULL CHECK(version > 0)); PRAGMA user_version=1;").unwrap();
        db.execute(
            "INSERT INTO meetings VALUES(?,?,?,?)",
            rusqlite::params![
                "python-id",
                "2026-10-01T12:00:00+00:00",
                document.to_string(),
                9
            ],
        )
        .unwrap();
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        tx(&store, |r| {
            let mut m = r.get("python-id")?;
            assert_eq!(m.version, 9);
            for (key, value) in document.as_object().unwrap() {
                assert_eq!(&m.dto()[key], value);
            }
            assert_eq!(m.folder, "");
            assert!(m.tasks.is_empty());
            assert!(m.calendar_event.is_none());
            let s = TranscriptSegment::from_dto(segment("python-id", "native-segment"))?;
            r.append(&s)?;
            m.update(&json!({"notes":"Native edit"}))?;
            r.save(&mut m)?;
            Ok(Value::Null)
        })
        .unwrap();
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let (saved, version): (String, i64) = db
            .query_row(
                "SELECT document,version FROM meetings WHERE id='python-id'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved["notes"], "Native edit");
        assert_eq!(saved["summary"], document["summary"]);
        assert!(saved.get("version").is_none());
        assert_eq!(version, 10);
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        db.pragma_update(None, "user_version", 4).unwrap();
    }
    assert!(SqliteStore::open(&path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bootstrap_is_local_and_failed_chat_retains_only_user_message() {
    for store in stores() {
        let r = rig(store);
        let bootstrap = call(&r.service, "bootstrap", json!({}));
        assert!(bootstrap["meetings"].as_array().unwrap().is_empty());
        assert!(r.ai.calls.lock().unwrap().is_empty());
        let id = create(&r);
        *r.ai.fail.lock().unwrap() = true;
        assert!(r
            .service
            .handle(
                "chat",
                json!({"meetingId":id,"prompt":"Question","requestId":"request"})
            )
            .is_err());
        let m = r.service.meeting(&id).unwrap();
        assert_eq!(m["messages"].as_array().unwrap().len(), 1);
        assert_eq!(m["messages"][0]["role"], "user");
        *r.ai.fail.lock().unwrap() = false;
        call(
            &r.service,
            "chat",
            json!({"meetingId":id,"prompt":"Retry question","requestId":"retry"}),
        );
    }
}
#[test]
fn pause_allows_capture_flush_without_holding_database_transaction() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        *r.capture.flush_on_pause.lock().unwrap() = true;
        let paused = call(&r.service, "pause_recording", json!({"meetingId":id}));
        assert_eq!(paused["status"], "paused");
        assert_eq!(paused["segments"].as_array().unwrap().len(), 1);
    }
}
#[test]
fn long_summary_includes_early_and_late_decisions() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        let mut early = segment(&id, "early");
        early["text"] = json!("EARLY DECISION ".to_owned() + &"evidence ".repeat(2900));
        let mut late = segment(&id, "late");
        late["start"] = json!(400.);
        late["end"] = json!(410.);
        late["text"] = json!("LATE DECISION ".to_owned() + &"evidence ".repeat(2900));
        r.service.segment(&id, early).unwrap();
        r.service.segment(&id, late).unwrap();
        call(&r.service, "stop_recording", json!({"meetingId":id}));
        call(&r.service, "summarize", json!({"meetingId":id}));
        let calls = r.ai.calls.lock().unwrap();
        let sections: Vec<_> = calls
            .iter()
            .filter(|(system, _, _)| system.contains("TRANSCRIPT SECTION"))
            .collect();
        assert!(sections.len() >= 2);
        assert!(sections
            .iter()
            .any(|(system, _, _)| system.contains("EARLY DECISION")));
        assert!(sections
            .iter()
            .any(|(system, _, _)| system.contains("LATE DECISION")));
        assert!(calls
            .last()
            .unwrap()
            .0
            .contains("COVERING ALL TRANSCRIPT SECTIONS"));
    }
}
struct FailCommitStore {
    inner: Arc<dyn Store>,
    field: &'static str,
    value: &'static str,
    enabled: std::sync::atomic::AtomicBool,
}
impl Store for FailCommitStore {
    fn transaction(
        &self,
        action: &mut dyn FnMut(&mut dyn Repository) -> Result<Value>,
    ) -> Result<Value> {
        self.inner.transaction(&mut |r| {
            let value = action(r)?;
            if self.enabled.load(std::sync::atomic::Ordering::SeqCst)
                && r.list()?.iter().any(|m| m.dto()[self.field] == self.value)
            {
                return Err(AppError::new("storage_error", "Simulated commit failure"));
            }
            Ok(value)
        })
    }
}
#[test]
fn final_commit_failure_preserves_audio_and_partial_evidence() {
    for inner in stores() {
        let store = Arc::new(FailCommitStore {
            inner,
            field: "status",
            value: "complete",
            enabled: std::sync::atomic::AtomicBool::new(false),
        });
        let r = rig(store.clone());
        let id = create(&r);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"keepAudio":false}}),
        );
        call(&r.service, "start_recording", json!({"meetingId":id}));
        store
            .enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(r
            .service
            .handle("stop_recording", json!({"meetingId":id}))
            .is_err());
        let m = r.service.meeting(&id).unwrap();
        assert_eq!(m["status"], "interrupted");
        assert!(!m["segments"].as_array().unwrap().is_empty());
        assert!(r.archive.removed.lock().unwrap().is_empty());
    }
}
#[test]
fn commit_failure_after_publication_is_uncertain() {
    for inner in stores() {
        let store = Arc::new(FailCommitStore {
            inner,
            field: "notionStatus",
            value: "synced",
            enabled: std::sync::atomic::AtomicBool::new(false),
        });
        let r = rig(store.clone());
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Publication"}}),
        );
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":"target"}}),
        );
        store
            .enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(r
            .service
            .handle("publish_notion", json!({"meetingId":id}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["notionStatus"], "uncertain");
        assert_eq!(*r.publisher.calls.lock().unwrap(), 1);
        assert!(r
            .service
            .handle("publish_notion", json!({"meetingId":id}))
            .is_err());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 1);
    }
}

#[test]
fn asynchronous_capture_failure_drains_and_persists_before_clearing_active() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"keepAudio":false}}),
        );
        call(&r.service, "start_recording", json!({"meetingId":id}));
        *r.capture.fail_stop.lock().unwrap() = true;
        assert!(r.service.interrupt_recording(&id).unwrap());
        let interrupted = r.service.meeting(&id).unwrap();
        assert_eq!(interrupted["status"], "interrupted");
        assert_eq!(interrupted["segments"].as_array().unwrap().len(), 1);
        assert_eq!(
            r.service.get_status().unwrap()["activeMeetingId"],
            Value::Null
        );
        assert!(r.archive.removed.lock().unwrap().is_empty());
        assert!(!r.service.interrupt_recording(&id).unwrap());
        let other = create(&r);
        *r.capture.fail_stop.lock().unwrap() = false;
        call(&r.service, "start_recording", json!({"meetingId":other}));
        assert!(!r.service.interrupt_recording(&id).unwrap());
        assert_eq!(r.service.meeting(&other).unwrap()["status"], "recording");
        assert!(r.service.meeting(&other).unwrap()["segments"]
            .as_array()
            .unwrap()
            .is_empty());
        call(&r.service, "stop_recording", json!({"meetingId":other}));
    }
}

#[test]
fn workspace_metadata_and_tasks_are_validated_atomically_and_survive_reload() {
    for store in stores() {
        let r = rig(store.clone());
        let id = create(&r);
        let event = json!({"id":"ics:demo","source":"ics","title":"Planning","start":"2026-10-23T10:00:00-04:00","end":"2026-10-23T11:00:00-04:00","joinUrl":"https://example.com/join","attendees":["SGL"]});
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"summary":"Edited recap","folder":"Clients","tags":["pilot","pilot"],"people":["SGL"],"calendarEvent":event}}),
        );
        let mut bad = event.clone();
        bad["end"] = json!("2026-10-22T11:00:00-04:00");
        assert!(r
            .service
            .handle(
                "update_meeting",
                json!({"id":id,"changes":{"notes":"must roll back","calendarEvent":bad}})
            )
            .is_err());
        let m = r.service.meeting(&id).unwrap();
        assert_eq!(m["notes"], "");
        assert_eq!(m["summary"], "Edited recap");
        assert_eq!(m["tags"], json!(["pilot"]));
        assert_eq!(m["calendarEvent"], event);
        let m = call(
            &r.service,
            "add_task",
            json!({"meetingId":id,"title":"Send plan","owner":"SGL","dueDate":"2026-10-30"}),
        );
        let task = m["tasks"][0]["id"].as_str().unwrap().to_owned();
        assert!(r.service.handle("update_task",json!({"meetingId":id,"id":task,"changes":{"title":"Bad change","dueDate":"2026-02-30"}})).is_err());
        assert_eq!(
            r.service.meeting(&id).unwrap()["tasks"][0]["title"],
            "Send plan"
        );
        call(
            &r.service,
            "update_task",
            json!({"meetingId":id,"id":task,"changes":{"done":true}}),
        );
        assert!(call(&r.service, "list_tasks", json!({}))
            .as_array()
            .unwrap()
            .is_empty());
        let all = call(&r.service, "list_tasks", json!({"includeDone":true}));
        assert_eq!(all[0]["meetingId"], id);
        assert_eq!(all[0]["owner"], "SGL");
        assert_eq!(all[0]["done"], true);
        let reloaded = rig(store.clone());
        assert_eq!(
            reloaded.service.meeting(&id).unwrap()["tasks"][0]["id"],
            task
        );
        call(
            &reloaded.service,
            "delete_task",
            json!({"meetingId":id,"id":task}),
        );
        assert!(
            call(&reloaded.service, "list_tasks", json!({"includeDone":true}))
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(reloaded
            .service
            .handle(
                "update_task",
                json!({"meetingId":id,"id":task,"changes":{"done":false}})
            )
            .is_err());
    }
}

#[test]
fn proposals_require_review_guard_intervening_edits_and_keep_lazy_prior_text() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Original personal notes"}}),
        );
        let service = Arc::downgrade(&r.service);
        let meeting_id = id.clone();
        *r.ai.hook.lock().unwrap() = Some(Box::new(move || {
            service
                .upgrade()
                .unwrap()
                .handle(
                    "update_meeting",
                    json!({"id":meeting_id,"changes":{"notes":"Typed while thinking"}}),
                )
                .unwrap();
        }));
        let p = call(
            &r.service,
            "propose_notes",
            json!({"meetingId":id,"target":"notes","instruction":"Clarify the phrasing","requestId":"proposal-1"}),
        );
        assert_eq!(p["baseText"], "Original personal notes");
        assert_eq!(
            r.service.meeting(&id).unwrap()["notes"],
            "Typed while thinking"
        );
        assert_eq!(r.service.handle("apply_proposal",json!({"meetingId":id,"target":"notes","baseText":p["baseText"],"text":p["text"]})).unwrap_err().code,"conflict");
        *r.ai.hook.lock().unwrap() = None;
        let p = call(
            &r.service,
            "propose_notes",
            json!({"meetingId":id,"target":"notes","instruction":"Clarify the phrasing","requestId":"proposal-2"}),
        );
        assert_eq!(
            r.service.meeting(&id).unwrap()["notes"],
            "Typed while thinking"
        );
        call(
            &r.service,
            "apply_proposal",
            json!({"meetingId":id,"target":"notes","baseText":p["baseText"],"text":p["text"]}),
        );
        let history = call(&r.service, "note_history", json!({"meetingId":id}));
        assert_eq!(history[0]["text"], "Typed while thinking");
        assert_eq!(history[1]["kind"], "manual");
        assert_eq!(history.as_array().unwrap().len(), 2);
        assert!(r.service.meeting(&id).unwrap().get("revisions").is_none());
        assert!(r
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, v)| name == "proposal_delta" && v["requestId"] == "proposal-2"));
        call(&r.service, "delete_meeting", json!({"id":id}));
        assert!(r
            .service
            .handle("note_history", json!({"meetingId":id}))
            .is_err());
    }
}

#[test]
fn generated_recap_cannot_overwrite_a_user_edit_and_chat_uses_visible_recap() {
    for store in stores() {
        let r = rig(store);
        let id = call(&r.service, "load_example", json!({}))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let service = Arc::downgrade(&r.service);
        let meeting_id = id.clone();
        *r.ai.hook.lock().unwrap() = Some(Box::new(move || {
            service
                .upgrade()
                .unwrap()
                .handle(
                    "update_meeting",
                    json!({"id":meeting_id,"changes":{"summary":"Recap edited during generation"}}),
                )
                .unwrap();
        }));
        assert_eq!(r.service.summarize(&id).unwrap_err().code, "conflict");
        assert_eq!(
            r.service.meeting(&id).unwrap()["summary"],
            "Recap edited during generation"
        );
        *r.ai.hook.lock().unwrap() = None;
        r.service
            .chat(&id, "Explain my recap", "recap-chat")
            .unwrap();
        assert!(r
            .ai
            .calls
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .0
            .contains("Recap edited during generation"));
    }
}

#[test]
fn saved_templates_and_recipes_are_durable_and_templates_change_summary_instructions() {
    for store in stores() {
        let r = rig(store.clone());
        let builtins = r.service.get_settings().unwrap()["recipes"]
            .as_array()
            .unwrap()
            .clone();
        assert!(builtins.len() >= 3);
        let templates = call(
            &r.service,
            "upsert_template",
            json!({"template":{"name":"Board review","instructions":"Group the recap by investment decisions and approved budget."}}),
        );
        let tid = templates[0]["id"].as_str().unwrap().to_owned();
        let recipes = call(
            &r.service,
            "upsert_recipe",
            json!({"recipe":{"name":"Budget follow-up","prompt":"What budget approvals are still pending? Cite evidence."}}),
        );
        let rid = recipes.as_array().unwrap().last().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let id = call(&r.service, "load_example", json!({}))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"template":tid}}),
        );
        r.service.summarize(&id).unwrap();
        assert!(r
            .ai
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(_, messages, _)| messages.iter().any(|m| m["content"]
                .as_str()
                .unwrap_or("")
                .contains("investment decisions and approved budget"))));
        let reloaded = rig(store);
        assert_eq!(
            reloaded.service.get_settings().unwrap()["customTemplates"][0]["id"],
            tid
        );
        assert!(reloaded
            .service
            .handle(
                "upsert_recipe",
                json!({"recipe":{"id":rid,"name":"","prompt":"keep"}})
            )
            .is_err());
        assert_eq!(
            reloaded.service.get_settings().unwrap()["recipes"]
                .as_array()
                .unwrap()
                .len(),
            builtins.len() + 1
        );
        call(&reloaded.service, "delete_recipe", json!({"id":rid}));
        call(&reloaded.service, "delete_template", json!({"id":tid}));
        assert!(reloaded.service.get_settings().unwrap()["customTemplates"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_ne!(
            domain::template_instructions("sales", &json!({})),
            domain::template_instructions("interview", &json!({}))
        );
    }
}

fn seed_library(store: &dyn Store) -> (String, String) {
    let mut scoped = Meeting::create("Pilot review").unwrap();
    scoped.update(&json!({"folder":"Clients","tags":["pilot"],"people":["SGL"],"notes":"Budget discussion only in personal notes."})).unwrap();
    let mut excluded = Meeting::create("Private unrelated meeting").unwrap();
    excluded.update(&json!({"folder":"Private","tags":["private"],"people":["Other"],"summary":"budget secret excluded sentinel"})).unwrap();
    tx(store,|r| {
        r.save(&mut scoped)?; r.save(&mut excluded)?;
        for i in 0..45 {
            r.append(&TranscriptSegment::from_dto(json!({"id":format!("pilot-{i}"),"meetingId":scoped.id,"start":i*5,"end":i*5+3,"text":format!("{}Budget approved for the pilot.{}", "前文 ".repeat(500)," closing context".repeat(200)),"speakerId":"you","source":"import"}))?)?;
        }
        r.append(&TranscriptSegment::from_dto(json!({"id":"excluded","meetingId":excluded.id,"start":0,"end":5,"text":"budget secret excluded sentinel","speakerId":"you","source":"import"}))?)?;
        Ok(Value::Null)
    }).unwrap();
    (scoped.id, excluded.id)
}

#[test]
fn retrieval_contract_is_scoped_bounded_literal_and_returns_real_citation_evidence() {
    for store in stores() {
        let (id, excluded) = seed_library(&*store);
        let query = domain::SearchQuery::from_value(&json!({"query":"budget pilot","folder":"Clients","tags":["pilot"],"people":["SGL"],"meetingIds":[id],"limit":7})).unwrap();
        let result = tx(&*store, |r| Ok(json!(r.search(&query)?))).unwrap();
        let hits = result.as_array().unwrap();
        assert_eq!(hits.len(), 7);
        for hit in hits {
            assert_eq!(hit["meetingId"], id);
            assert!(hit["text"].as_str().unwrap().chars().count() <= 1200);
            assert!(hit["text"].as_str().unwrap().contains("Budget approved"));
            assert!(hit["url"]
                .as_str()
                .unwrap()
                .starts_with(&format!("tuck://meeting/{id}?")));
            if hit["source"] == "transcript" {
                let actual = tx(&*store, |r| Ok(json!(r.segments(&id)?))).unwrap();
                assert!(actual
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["id"] == hit["segmentId"] && s["start"] == hit["start"]));
            }
        }
        let empty = domain::SearchQuery::from_value(
            &json!({"query":"budget","folder":"Clients","meetingIds":[excluded]}),
        )
        .unwrap();
        assert!(tx(&*store, |r| Ok(json!(r.search(&empty)?)))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        let literal =
            domain::SearchQuery::from_value(&json!({"query":"xyzzy' qqxx 991199 --","limit":2}))
                .unwrap();
        assert!(tx(&*store, |r| Ok(json!(r.search(&literal)?)))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        tx(&*store, |r| {
            let m = r.get(&id)?;
            r.delete(&m)?;
            assert!(r.search(&query)?.is_empty());
            Ok(Value::Null)
        })
        .unwrap();
    }
}

#[test]
fn library_answers_use_only_retrieved_scope_and_leave_meeting_chat_separate() {
    for store in stores() {
        let (id, _) = seed_library(&*store);
        let r = rig(store);
        *r.ai.answer.lock().unwrap() =
            "The pilot budget was approved [E1](tuck://meeting/fake).".into();
        let message = call(
            &r.service,
            "chat_library",
            json!({"prompt":"budget pilot","requestId":"library-1","folder":"Clients","tags":["pilot"],"people":["SGL"]}),
        );
        assert!(!message["content"]
            .as_str()
            .unwrap()
            .contains("tuck://meeting/fake"));
        assert_eq!(message["retrieval"], "lexical");
        assert!(!message["citations"].as_array().unwrap().is_empty());
        assert!(message["content"]
            .as_str()
            .unwrap()
            .contains(&format!("[E1](tuck://meeting/{id}?")));
        let calls = r.ai.calls.lock().unwrap();
        let system = &calls.last().unwrap().0;
        assert!(system.contains("Budget approved"));
        assert!(!system.contains("excluded sentinel"));
        assert!(system.chars().count() < 20_000);
        assert_eq!(calls.last().unwrap().2["webSearch"], false);
        drop(calls);
        assert!(r.service.meeting(&id).unwrap()["messages"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(r
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, v)| name == "library_chat_delta" && v["requestId"] == "library-1"));
        let before = r.ai.calls.lock().unwrap().len();
        let empty = call(
            &r.service,
            "chat_library",
            json!({"prompt":"xyzzy","requestId":"library-2"}),
        );
        assert_eq!(r.ai.calls.lock().unwrap().len(), before);
        assert!(empty["citations"].as_array().unwrap().is_empty());
    }
}

#[test]
fn capture_configuration_precedes_lifecycle_and_propagates_device_ids() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"microphoneDeviceId":"mic-unique","systemDeviceId":"output-unique"}}),
        );
        *r.capture.fail_configure.lock().unwrap() = true;
        assert!(r.service.start_recording(&id, true, true).is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["status"], "draft");
        assert!(r.service.get_status().unwrap()["activeMeetingId"].is_null());
        assert!(r.capture.callback.lock().unwrap().is_none());
        *r.capture.fail_configure.lock().unwrap() = false;
        r.service.start_recording(&id, true, true).unwrap();
        assert_eq!(
            r.capture.configured.lock().unwrap().as_ref().unwrap()["microphoneDeviceId"],
            "mic-unique"
        );
        assert_eq!(
            r.capture.configured.lock().unwrap().as_ref().unwrap()["systemDeviceId"],
            "output-unique"
        );
        r.service.stop_recording(&id).unwrap();
    }
}

#[test]
fn revision_and_text_changes_roll_back_together_without_an_update_event() {
    for inner in stores() {
        let store = Arc::new(FailCommitStore {
            inner: inner.clone(),
            field: "notes",
            value: "Rejected proposal",
            enabled: std::sync::atomic::AtomicBool::new(false),
        });
        let r = rig(store.clone());
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Keep my draft"}}),
        );
        let prior_events = r.events.lock().unwrap().len();
        let prior_history = call(&r.service, "note_history", json!({"meetingId":id}));
        store
            .enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(r.service.handle("apply_proposal",json!({"meetingId":id,"target":"notes","baseText":"Keep my draft","text":"Rejected proposal"})).unwrap_err().code,"storage_error");
        assert_eq!(r.service.meeting(&id).unwrap()["notes"], "Keep my draft");
        assert_eq!(
            call(&r.service, "note_history", json!({"meetingId":id})),
            prior_history
        );
        assert_eq!(r.events.lock().unwrap().len(), prior_events);
        store
            .enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        call(&r.service, "delete_meeting", json!({"id":id}));
        assert!(tx(&*inner, |repo| Ok(json!(repo.revisions(&id)?)))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn known_notion_page_can_sync_but_uncertain_initial_creation_cannot_retry() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":"database-id","notionPropertyMapping":{"folder":"Folder","meetingId":"Tuck ID"}}}),
        );
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"My publishable notes"}}),
        );
        assert!(r.service.sync_notion(&id).is_err());
        assert_eq!(
            call(&r.service, "notion_destination", json!({}))["id"],
            "database-id"
        );
        assert_eq!(
            call(
                &r.service,
                "notion_destinations",
                json!({"query":"Clients"})
            )["destinations"][0]["title"],
            "Clients"
        );
        *r.publisher.uncertain.lock().unwrap() = true;
        assert!(r.service.publish_notion(&id).is_err());
        assert!(r.service.publish_notion(&id).is_err());
        assert!(r.service.sync_notion(&id).is_err());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 1);
        let second = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":second,"changes":{"notes":"Another meeting"}}),
        );
        *r.publisher.uncertain.lock().unwrap() = false;
        r.service.publish_notion(&second).unwrap();
        assert_eq!(
            r.publisher.settings.lock().unwrap().last().unwrap()["notionPropertyMapping"]["folder"],
            "Folder"
        );
        assert!(r.service.publish_notion(&second).is_err());
        *r.publisher.uncertain.lock().unwrap() = true;
        assert!(r.service.sync_notion(&second).is_err());
        assert_eq!(
            r.service.meeting(&second).unwrap()["notionStatus"],
            "uncertain"
        );
        *r.publisher.uncertain.lock().unwrap() = false;
        assert_eq!(
            r.service.sync_notion(&second).unwrap()["notionStatus"],
            "synced"
        );
    }
}

#[test]
fn publication_urls_accept_modern_notion_without_weakening_the_host_boundary() {
    for address in [
        "https://app.notion.com/p/aa11",
        "https://www.notion.so/page",
        "https://workspace.notion.site/page",
        "https://app.notion.com:443/p/aa11",
    ] {
        let mut m = Meeting::create("URL validation").unwrap();
        m.publication_finished(address).unwrap();
        assert_eq!(m.notion_url, address);
    }
    for address in [
        "https://fake.app.notion.com/p/id",
        "https://app.notion.com.attacker.test/p/id",
        "https://user@app.notion.com/p/id",
        "https://app.notion.com:8443/p/id",
        "http://app.notion.com/p/id",
        "https://user:password@www.notion.so/page",
        "https://www.notion.so:8443/page",
    ] {
        let mut m = Meeting::create("URL validation").unwrap();
        assert!(m.publication_finished(address).is_err(), "{address}");
        assert!(m.notion_url.is_empty());
    }
}

#[test]
fn uncertain_first_upload_can_link_only_an_explicitly_verified_existing_page() {
    for store in stores() {
        let r = rig(store.clone());
        let id = create(&r);
        let address = "https://app.notion.com/p/12345678-1234-1234-1234-123456789012";
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":"database-id"}}),
        );
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Recoverable notes"}}),
        );
        assert!(r
            .service
            .handle("recover_notion", json!({"meetingId":id,"url":address}))
            .is_err());
        assert!(r.publisher.recoveries.lock().unwrap().is_empty());
        *r.publisher.uncertain.lock().unwrap() = true;
        assert!(r.service.publish_notion(&id).is_err());
        *r.publisher.bad_url.lock().unwrap() = true;
        let before_events = r.events.lock().unwrap().len();
        assert!(r
            .service
            .handle("recover_notion", json!({"meetingId":id,"url":address}))
            .is_err());
        assert_eq!(r.service.meeting(&id).unwrap()["notionStatus"], "uncertain");
        assert_eq!(r.events.lock().unwrap().len(), before_events);
        *r.publisher.bad_url.lock().unwrap() = false;
        let result = call(
            &r.service,
            "recover_notion",
            json!({"meetingId":id,"url":address}),
        );
        assert_eq!(result["notionUrl"], address);
        assert_eq!(result["notionStatus"], "synced");
        assert_eq!(*r.publisher.calls.lock().unwrap(), 1); // No second page creation.
        assert_eq!(
            r.publisher.recoveries.lock().unwrap().last().unwrap().0,
            "database-id"
        );
        assert_eq!(
            r.publisher.recoveries.lock().unwrap().last().unwrap().1["id"],
            id
        );
        assert_eq!(
            r.publisher.recoveries.lock().unwrap().last().unwrap().2,
            address
        );
        let reloaded = rig(store);
        assert_eq!(reloaded.service.meeting(&id).unwrap()["notionUrl"], address);
        assert!(reloaded
            .service
            .handle("recover_notion", json!({"meetingId":id,"url":address}))
            .is_err());
    }
}

#[test]
fn corrections_preserve_raw_speech_guard_conflicts_and_drive_effective_evidence() {
    for store in stores() {
        let r = rig(store.clone());
        let id = create(&r);
        call(&r.service, "start_recording", json!({"meetingId":id}));
        assert!(r
            .service
            .handle(
                "correct_transcript",
                json!({"meetingId":id,"segmentId":"drained","baseText":"anything","text":"Denied"})
            )
            .is_err());
        call(&r.service, "stop_recording", json!({"meetingId":id}));
        let original = r.service.meeting(&id).unwrap()["segments"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        let corrected = "The launch date is Thursday, according to my correction.";
        let meeting = call(
            &r.service,
            "correct_transcript",
            json!({"meetingId":id,"segmentId":"drained","baseText":original,"text":corrected}),
        );
        assert_eq!(meeting["segments"][0]["text"], corrected);
        assert_eq!(meeting["segments"][0]["originalText"], original);
        assert_eq!(meeting["segments"][0]["edited"], true);
        assert_eq!(r.service.handle("correct_transcript",json!({"meetingId":id,"segmentId":"drained","baseText":original,"text":"Stale replacement"})).unwrap_err().code,"conflict");
        let raw = tx(&*store, |repo| Ok(json!(repo.raw_segments(&id)?))).unwrap();
        assert_eq!(raw[0]["text"], original);
        let hits = call(
            &r.service,
            "search_library",
            json!({"query":"Thursday","meetingIds":[id]}),
        );
        assert_eq!(hits[0]["edited"], true);
        assert!(hits[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("[User-corrected transcript]"));
        assert!(call(
            &r.service,
            "search_library",
            json!({"query":"Friday","meetingIds":[id]})
        )
        .as_array()
        .unwrap()
        .is_empty());
        r.service
            .chat(&id, "What changed?", "correction-chat")
            .unwrap();
        let calls = r.ai.calls.lock().unwrap();
        let system = &calls.last().unwrap().0;
        assert!(system.contains(corrected));
        assert!(system.contains("[user-corrected]"));
        assert!(!system.contains(&original));
        drop(calls);
        for format in ["markdown", "srt"] {
            assert!(
                r.service.export_meeting(&id, format, None).unwrap()["content"]
                    .as_str()
                    .unwrap()
                    .contains("[user-corrected]")
            );
        }
        let exported = r.service.export_meeting(&id, "json", None).unwrap();
        assert!(exported["content"]
            .as_str()
            .unwrap()
            .contains("originalText"));
        assert!(r
            .service
            .handle(
                "reset_transcript",
                json!({"meetingId":id,"segmentId":"drained","baseText":original})
            )
            .is_err());
        let reset = call(
            &r.service,
            "reset_transcript",
            json!({"meetingId":id,"segmentId":"drained","baseText":corrected}),
        );
        assert_eq!(reset["segments"][0]["text"], original);
        assert_eq!(reset["segments"][0]["edited"], false);
        tx(&*store,|repo| { repo.save_correction(&id,"drained",Some(&json!({"segmentId":"drained","text":"Must roll back","updatedAt":domain::now()})))?; Err(AppError::invalid("rollback correction")) }).unwrap_err();
        assert_eq!(
            r.service.meeting(&id).unwrap()["segments"][0]["text"],
            original
        );
        call(
            &r.service,
            "correct_transcript",
            json!({"meetingId":id,"segmentId":"drained","baseText":original,"text":corrected}),
        );
        call(&r.service, "delete_meeting", json!({"id":id}));
        assert!(tx(&*store, |repo| Ok(json!(repo.corrections(&id)?)))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
}

fn safety_backup_fixture(store: Arc<dyn Store>) -> (Rig, String, Value) {
    let r = rig(store.clone());
    let id = create(&r);
    let template = domain::id();
    call(
        &r.service,
        "update_settings",
        json!({"changes":{"provider":"openai","hostedConsent":true,"autoSummary":true,"notionAutoSync":true,"notionTarget":"private-destination","microphoneDeviceId":"private-device","customTemplates":[{"id":template,"name":"Saved template","instructions":"Preserve decisions."}]}}),
    );
    call(
        &r.service,
        "update_meeting",
        json!({"id":id,"changes":{"notes":"Private notes preserved","summary":"A recap","folder":"Clients","tags":["pilot"],"people":["SGL"]}}),
    );
    call(
        &r.service,
        "add_task",
        json!({"meetingId":id,"title":"Follow up","owner":"SGL","dueDate":"2026-12-01"}),
    );
    tx(&*store,|repo| {
        repo.append(&TranscriptSegment::from_dto(segment(&id,"raw-speech"))?)?;
        repo.rename_speaker(&id,"you","Spencer")?;
        repo.add_bookmark(&id,&json!({"id":"marker","time":4,"label":"Decision"}))?;
        repo.add_message(&id,&json!({"id":"conversation-message","role":"assistant","content":"An answer","createdAt":domain::now()}))?;
        for n in 0..125 { repo.add_revision(&id,&json!({"id":format!("revision-{n}"),"target":"notes","text":format!("Draft {n}"),"createdAt":domain::now()}))?; }
        repo.save_correction(&id,"raw-speech",Some(&json!({"segmentId":"raw-speech","text":"Corrected Thursday evidence","updatedAt":domain::now()})))?;
        let mut m=repo.get(&id)?; m.notion_url="https://www.notion.so/old-page".into(); m.notion_status="synced".into(); repo.save(&mut m)?;
        Ok(Value::Null)
    }).unwrap();
    let report = call(
        &r.service,
        "backup_library",
        json!({"path":"/fixture/backup.tuck.zip","includeAudio":true}),
    );
    assert_eq!(report["meetingCount"], 1);
    assert_eq!(report["audioIncluded"], true);
    let manifest = r.archive.backup.lock().unwrap().clone().unwrap();
    (r, id, manifest)
}

#[test]
fn portable_backup_restores_full_evidence_and_history_with_safe_preferences() {
    for store in stores() {
        let (source, id, mut manifest) = safety_backup_fixture(store);
        assert_eq!(
            manifest["meetings"][0]["segments"][0]["text"],
            "We agreed to ship on Friday."
        );
        assert_eq!(
            manifest["meetings"][0]["corrections"][0]["text"],
            "Corrected Thursday evidence"
        );
        assert!(
            manifest["meetings"][0]["revisions"]
                .as_array()
                .unwrap()
                .len()
                > 100
        );
        for key in ["hostedConsent", "autoSummary", "notionAutoSync"] {
            assert_eq!(manifest["settings"][key], false);
        }
        assert_eq!(manifest["settings"]["notionTarget"], "");
        assert_eq!(manifest["settings"]["microphoneDeviceId"], "");
        assert!(manifest["settings"].get("hasOpenAIKey").is_none());
        manifest["meetings"][0]["meeting"]["status"] = json!("recording");
        let target = rig(Arc::new(SqliteStore::in_memory().unwrap()));
        *target.archive.backup.lock().unwrap() = Some(manifest);
        let preview = call(
            &target.service,
            "preview_restore_library",
            json!({"path":"/fixture/backup.tuck.zip"}),
        );
        assert_eq!(preview["restorable"], 1);
        assert_eq!(preview["settingsMode"], "safe-preferences");
        assert!(call(&target.service, "bootstrap", json!({}))["meetings"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(*target.archive.discarded.lock().unwrap(), 1);
        let report = call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/backup.tuck.zip","expectedFingerprint":preview["fingerprint"]}),
        );
        assert_eq!(report["restored"], 1);
        assert_eq!(report["audioFilesRestored"], 1);
        let m = target.service.meeting(&id).unwrap();
        assert_eq!(m["notes"], "Private notes preserved");
        assert_eq!(m["status"], "interrupted");
        assert_eq!(m["notionStatus"], "idle");
        assert_eq!(m["notionUrl"], "");
        assert_eq!(m["segments"][0]["text"], "Corrected Thursday evidence");
        assert_eq!(
            m["segments"][0]["originalText"],
            "We agreed to ship on Friday."
        );
        assert_eq!(m["speakers"][0]["name"], "Spencer");
        assert_eq!(m["messages"][0]["content"], "An answer");
        assert_eq!(m["bookmarks"][0]["label"], "Decision");
        assert_eq!(m["tasks"][0]["owner"], "SGL");
        let settings = target.service.get_settings().unwrap();
        assert_eq!(settings["provider"], "local");
        assert_eq!(settings["hostedConsent"], false);
        assert_eq!(settings["autoSummary"], false);
        assert_eq!(settings["customTemplates"][0]["name"], "Saved template");
        assert_eq!(
            call(&target.service, "note_history", json!({"meetingId":id}))
                .as_array()
                .unwrap()
                .len(),
            100
        );
        call(
            &target.service,
            "backup_library",
            json!({"path":"/fixture/rebackup.zip","includeAudio":false}),
        );
        assert!(
            target.archive.backup.lock().unwrap().as_ref().unwrap()["meetings"][0]["revisions"]
                .as_array()
                .unwrap()
                .len()
                > 100
        );
        assert!(source.ai.calls.lock().unwrap().is_empty());
        assert!(target.ai.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn restore_preview_binding_skip_merge_and_invalid_input_never_replace_current_work() {
    for store in stores() {
        let (_, id, manifest) = safety_backup_fixture(Arc::new(MemoryStore::default()));
        let target = rig(store.clone());
        *target.archive.backup.lock().unwrap() = Some(manifest.clone());
        assert!(target
            .service
            .handle(
                "restore_library",
                json!({"path":"/fixture/backup.zip","expectedFingerprint":"b".repeat(64)})
            )
            .is_err());
        assert!(target
            .service
            .handle("restore_library", json!({"path":"/fixture/backup.zip"}))
            .is_err());
        assert!(call(&target.service, "bootstrap", json!({}))["meetings"]
            .as_array()
            .unwrap()
            .is_empty());
        let mut malformed = manifest.clone();
        malformed["meetings"][0]["corrections"][0]["segmentId"] = json!("not-in-evidence");
        *target.archive.backup.lock().unwrap() = Some(malformed);
        assert!(target
            .service
            .handle(
                "preview_restore_library",
                json!({"path":"/fixture/backup.zip"})
            )
            .is_err());
        assert!(call(&target.service, "bootstrap", json!({}))["meetings"]
            .as_array()
            .unwrap()
            .is_empty());
        *target.archive.backup.lock().unwrap() = Some(manifest.clone());
        call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/backup.zip","expectedFingerprint":"a".repeat(64)}),
        );
        call(
            &target.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Newer local work"}}),
        );
        call(
            &target.service,
            "update_settings",
            json!({"changes":{"model":"my-current-model","provider":"openai","hostedConsent":true}}),
        );
        let report = call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/backup.zip","expectedFingerprint":"a".repeat(64)}),
        );
        assert_eq!(report["restored"], 0);
        assert_eq!(report["skipped"], 1);
        assert_eq!(report["audioFilesRestored"], 0);
        assert_eq!(report["settingsMode"], "saved-writing-merged");
        assert_eq!(
            target.service.meeting(&id).unwrap()["notes"],
            "Newer local work"
        );
        assert_eq!(
            target.service.get_settings().unwrap()["model"],
            "my-current-model"
        );
        assert_eq!(
            target.service.get_settings().unwrap()["hostedConsent"],
            true
        );
        assert!(target
            .archive
            .restore_maps
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty());
        assert!(*target.archive.discarded.lock().unwrap() >= 5);
    }
}

#[test]
fn audio_recovery_creates_a_separate_linked_workspace_and_partial_restore_is_explicit() {
    for store in stores() {
        let (source, id, manifest) = safety_backup_fixture(store.clone());
        let original = source.service.meeting(&id).unwrap();
        assert_eq!(
            call(&source.service, "meeting_audio", json!({"meetingId":id}))[0]["id"],
            "microphone.wav"
        );
        assert!(source
            .service
            .handle(
                "audio_source",
                json!({"meetingId":"missing","audioId":"microphone.wav"})
            )
            .is_err());
        let recovered = call(
            &source.service,
            "recover_audio",
            json!({"meetingId":id,"audioId":"microphone.wav"}),
        );
        assert_ne!(recovered["id"], id);
        assert_eq!(recovered["recoveredFrom"], id);
        assert_eq!(recovered["notes"], original["notes"]);
        assert_eq!(recovered["summary"], "");
        assert_eq!(recovered["notionUrl"], "");
        assert_eq!(recovered["status"], "complete");
        assert_eq!(source.service.meeting(&id).unwrap(), original);
        assert!(source.ai.calls.lock().unwrap().is_empty());
        assert_eq!(*source.publisher.calls.lock().unwrap(), 0);
        let target = rig(Arc::new(MemoryStore::default()));
        *target.archive.backup.lock().unwrap() = Some(manifest);
        *target.archive.fail_restore_audio.lock().unwrap() = true;
        let report = call(
            &target.service,
            "restore_library",
            json!({"path":"/fixture/backup.zip","expectedFingerprint":"a".repeat(64)}),
        );
        assert_eq!(report["restored"], 1);
        assert!(report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("retained audio")));
        assert!(target.service.meeting(&id).is_ok());
        assert_eq!(*target.archive.discarded.lock().unwrap(), 1);
        let active = create(&source);
        call(
            &source.service,
            "start_recording",
            json!({"meetingId":active}),
        );
        for method in [
            "backup_library",
            "preview_restore_library",
            "restore_library",
        ] {
            assert!(source
                .service
                .handle(
                    method,
                    json!({"path":"/fixture/archive.zip","expectedFingerprint":"a".repeat(64)})
                )
                .is_err());
        }
        call(
            &source.service,
            "stop_recording",
            json!({"meetingId":active}),
        );
    }
}

#[test]
fn rejected_restore_transaction_leaves_no_partial_library_and_discards_audio_stage() {
    for inner in stores() {
        let (_, _, manifest) = safety_backup_fixture(Arc::new(MemoryStore::default()));
        let failing = Arc::new(FailCommitStore {
            inner: inner.clone(),
            field: "notes",
            value: "Private notes preserved",
            enabled: std::sync::atomic::AtomicBool::new(false),
        });
        let target = rig(failing.clone());
        *target.archive.backup.lock().unwrap() = Some(manifest);
        failing
            .enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            target
                .service
                .handle(
                    "restore_library",
                    json!({"path":"/fixture/backup.zip","expectedFingerprint":"a".repeat(64)})
                )
                .unwrap_err()
                .code,
            "storage_error"
        );
        assert!(tx(&*inner, |repo| Ok(json!(repo.list()?.len()))).unwrap() == 0);
        assert!(target.archive.restore_maps.lock().unwrap().is_empty());
        assert_eq!(*target.archive.discarded.lock().unwrap(), 1);
        assert!(target.events.lock().unwrap().is_empty());
    }
}

#[test]
fn schema_v2_corrections_migrate_additively_and_raw_documents_stay_unchanged() {
    let dir = std::env::temp_dir().join(format!("tuck-correction-migration-{}", domain::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("library.sqlite3");
    let m = Meeting::create("Older v2 workspace").unwrap();
    let raw = TranscriptSegment::from_dto(segment(&m.id, "raw-segment")).unwrap();
    {
        let store = SqliteStore::open(&path).unwrap();
        let mut saved = m.clone();
        tx(&store, |repo| {
            repo.save(&mut saved)?;
            repo.append(&raw)?;
            Ok(Value::Null)
        })
        .unwrap();
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE transcript_corrections; PRAGMA user_version=2;")
            .unwrap();
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        tx(&store,|repo|{ let mut before=repo.get(&m.id)?; assert!(before.recovered_from.is_none()); before.status="complete".into();repo.save(&mut before)?; repo.save_correction(&m.id,&raw.id,Some(&json!({"segmentId":raw.id,"text":"Corrected after migration","updatedAt":domain::now()})))?;Ok(Value::Null)}).unwrap();
        assert_eq!(
            tx(&store, |repo| Ok(json!(repo.segments(&m.id)?))).unwrap()[0]["text"],
            "Corrected after migration"
        );
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    let stored: String = db
        .query_row(
            "SELECT document FROM segments WHERE meeting_id=? AND id=?",
            rusqlite::params![m.id, raw.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), raw.dto());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shutdown_waits_for_archive_file_work_to_finish_before_closing_native_capture() {
    use std::{sync::mpsc, thread, time::Duration};
    for store in stores() {
        let r = rig(store);
        create(&r);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        *r.archive.backup_gate.lock().unwrap() = Some(Arc::new(ArchiveGate {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }));
        let backup_service = r.service.clone();
        let backup = thread::spawn(move || {
            backup_service.handle(
                "backup_library",
                json!({"path":"/fixture/blocked-backup.zip"}),
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("backup reached native file work");
        let shutdown_service = r.service.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = shutdown_service.handle("shutdown", json!({}));
            finished_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let early = finished_rx.recv_timeout(Duration::from_millis(100));
        let native_shutdown_calls_before_release = *r.capture.shutdown_calls.lock().unwrap();
        // Always release the archive before asserting, so a regression cannot leave
        // either worker blocked behind a failed assertion.
        release_tx.send(()).unwrap();
        assert!(
            matches!(early, Err(mpsc::RecvTimeoutError::Timeout)),
            "shutdown returned before archive work was released"
        );
        assert_eq!(native_shutdown_calls_before_release, 0);
        assert_eq!(backup.join().unwrap().unwrap()["meetingCount"], 1);
        assert!(finished_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok());
        shutdown.join().unwrap();
        assert_eq!(*r.capture.shutdown_calls.lock().unwrap(), 1);
        assert!(r.archive.backup.lock().unwrap().is_some());
    }
}

#[test]
fn manual_autosaves_coalesce_but_semantic_replacements_keep_every_precise_prior_text() {
    for store in stores() {
        let r = rig(store.clone());
        let id = create(&r);
        let body = "Long private notes. ".repeat(4000);
        for n in 0..105 {
            let text = format!("{body}\nDraft {n}");
            let current = call(
                &r.service,
                "update_meeting",
                json!({"id":id,"changes":{"notes":text}}),
            );
            assert_eq!(current["notes"], text);
        }
        let history = call(&r.service, "note_history", json!({"meetingId":id}));
        assert_eq!(history.as_array().unwrap().len(), 1);
        assert_eq!(history[0]["kind"], "manual");
        assert_eq!(history[0]["text"], "");
        tx(&*store, |repo| {
            let info = repo.latest_revision_info(&id, "notes")?.unwrap();
            assert_eq!(info.as_object().unwrap().len(), 2);
            assert!(info.get("text").is_none());
            assert!(repo.latest_revision_info(&id, "summary")?.is_none());
            Ok(Value::Null)
        })
        .unwrap();
        let before = r.service.meeting(&id).unwrap()["notes"]
            .as_str()
            .unwrap()
            .to_owned();
        call(
            &r.service,
            "apply_proposal",
            json!({"meetingId":id,"target":"notes","baseText":before,"text":"First accepted replacement"}),
        );
        call(
            &r.service,
            "apply_proposal",
            json!({"meetingId":id,"target":"notes","baseText":"First accepted replacement","text":"Second accepted replacement"}),
        );
        let history = call(&r.service, "note_history", json!({"meetingId":id}));
        assert_eq!(history.as_array().unwrap().len(), 3);
        assert_eq!(history[0]["text"], "First accepted replacement");
        assert_eq!(history[0]["kind"], "replacement");
        assert_eq!(history[1]["text"], before);
        assert_eq!(history[1]["kind"], "replacement");
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Manual after replacement","summary":"First summary draft"}}),
        );
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"notes":"Newest durable manual notes","summary":"Newest durable recap"}}),
        );
        let all = tx(&*store, |repo| Ok(json!(repo.all_revisions(&id)?))).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 5);
        assert!(all
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["text"] == "Second accepted replacement" && v["kind"] == "manual"));
        assert_eq!(
            r.service.meeting(&id).unwrap()["notes"],
            "Newest durable manual notes"
        );
        assert_eq!(
            r.service.meeting(&id).unwrap()["summary"],
            "Newest durable recap"
        );
    }
}

#[test]
fn checkpoint_interval_and_legacy_or_changed_clocks_preserve_a_fresh_prior_state() {
    for store in stores() {
        let r = rig(store.clone());
        let cases = [
            (
                Some("manual"),
                (chrono::Utc::now() - chrono::Duration::minutes(6)).to_rfc3339(),
            ),
            (Some("manual"), "2999-01-01T00:00:00Z".into()),
            (Some("manual"), "invalid-legacy-clock".into()),
            (None, domain::now()),
        ];
        for (kind, created_at) in cases {
            let mut meeting = Meeting::create("Clock-sensitive history").unwrap();
            meeting.notes = "Prior current text".into();
            tx(&*store,|repo| {
                repo.save(&mut meeting)?;
                let mut revision=json!({"id":domain::id(),"target":"notes","text":"Older version","createdAt":created_at});
                if let Some(kind)=kind { revision["kind"]=json!(kind); }
                repo.add_revision(&meeting.id,&revision)?;
                Ok(Value::Null)
            }).unwrap();
            call(
                &r.service,
                "update_meeting",
                json!({"id":meeting.id,"changes":{"notes":"A newly saved draft"}}),
            );
            call(
                &r.service,
                "update_meeting",
                json!({"id":meeting.id,"changes":{"notes":"Another durable autosave"}}),
            );
            tx(&*store, |repo| {
                let revisions = repo.all_revisions(&meeting.id)?;
                assert_eq!(revisions.len(), 2, "case {kind:?} / {created_at}");
                assert!(revisions
                    .iter()
                    .any(|v| v["text"] == "Prior current text" && v["kind"] == "manual"));
                let info = repo.latest_revision_info(&meeting.id, "notes")?.unwrap();
                assert_eq!(info["kind"], "manual");
                assert!(
                    chrono::DateTime::parse_from_rfc3339(info["createdAt"].as_str().unwrap())
                        .unwrap()
                        <= chrono::Utc::now()
                );
                Ok(Value::Null)
            })
            .unwrap();
        }
    }
}
