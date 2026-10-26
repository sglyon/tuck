//! Real controller/core/temporary SQLite acceptance, with synthetic speech and fake delivery.
//! No native clipboard, accessibility inspection, microphone, model or provider is used.
use super::*;
use crate::adapters::dictation_target::DeliveryOutcome;
use std::{path::Path, sync::Weak};
use tuck_core::{
    ports::{
        AccountPort, AudioArchivePort, IntelligencePort, OnSegment, PublicationPort, SecretPort,
    },
    store::sqlite::SqliteStore,
    Adapters,
};

const START: &str = "These are the original recognized words.";
const FINAL: &str = "The final microphone words were drained before delivery.";
#[derive(Default)]
struct SyntheticCapture {
    active: Mutex<Option<(String, OnSegment)>>,
    sources: Mutex<Vec<(bool, bool)>>,
    configured: Mutex<Vec<Value>>,
    stops: std::sync::atomic::AtomicUsize,
}
impl RecordingPort for SyntheticCapture {
    fn status(&self) -> Value {
        json!({"captureAvailable":true,"modelInstalled":true})
    }
    fn configure(&self, settings: &Value) -> Result<()> {
        self.configured.lock().unwrap().push(settings.clone());
        Ok(())
    }
    fn start(
        &self,
        id: &str,
        on: OnSegment,
        system: bool,
        mic: bool,
        _: u32,
        _: bool,
    ) -> Result<()> {
        self.sources.lock().unwrap().push((system, mic));
        *self.active.lock().unwrap() = Some((id.into(), on.clone()));
        on(
            json!({"id":"recognized-start","meetingId":id,"start":0.,"end":2.,"text":START,"speakerId":"you","source":"microphone"}),
        )
    }
    fn pause(&self) -> Result<()> {
        Ok(())
    }
    fn resume(&self) -> Result<()> {
        Ok(())
    }
    fn stop(&self) -> Result<f64> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        if let Some((id, on)) = self.active.lock().unwrap().take() {
            on(
                json!({"id":"recognized-drain","meetingId":id,"start":2.,"end":5.,"text":FINAL,"speakerId":"you","source":"microphone"}),
            )?;
        }
        Ok(5.)
    }
    fn shutdown(&self) -> Result<()> {
        Ok(())
    }
    fn install_model(&self, _: &str) -> Result<Value> {
        panic!("Acceptance must not download speech models")
    }
    fn unload(&self) -> Result<()> {
        Ok(())
    }
    fn import_file(&self, _: &str, _: &str, _: OnSegment, _: u32) -> Result<f64> {
        panic!("Acceptance must not read audio files")
    }
}
struct NoIntelligence;
impl IntelligencePort for NoIntelligence {
    fn models(&self, _: &Value) -> Result<Vec<Value>> {
        panic!("Dictation must not load AI providers")
    }
    fn complete(&self, _: &Value, _: &str, _: &[Value], _: &mut dyn FnMut(&str)) -> Result<String> {
        panic!("Dictation must not summarize or process recognized speech with an LLM")
    }
    fn complete_coach(&self, _: &Value, _: &str, _: &[Value]) -> Result<String> {
        panic!("Dictation must not invoke live agents")
    }
}
struct NoPublication;
impl PublicationPort for NoPublication {
    fn publish(&self, _: &str, _: &Value) -> Result<String> {
        panic!("Dictation must not publish notes to Notion")
    }
}
struct NoSecrets;
impl SecretPort for NoSecrets {
    fn contains(&self, _: &str) -> Result<bool> {
        Ok(false)
    }
    fn get(&self, _: &str) -> Result<Option<String>> {
        panic!("Dictation must not decrypt credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<()> {
        panic!("Dictation must not write credentials")
    }
    fn delete(&self, _: &str) -> Result<()> {
        panic!("Dictation must not delete credentials")
    }
}
struct NoAccounts;
impl AccountPort for NoAccounts {
    fn accounts(&self) -> Result<Vec<Value>> {
        Ok(vec![])
    }
    fn sign_in(&self, _: Option<&str>) -> Result<Vec<Value>> {
        panic!("No account operation belongs to dictation")
    }
    fn select(&self, _: &str) -> Result<Vec<Value>> {
        panic!("No account operation belongs to dictation")
    }
    fn sign_out(&self, _: &str) -> Result<Value> {
        panic!("No account operation belongs to dictation")
    }
}
struct NoArchive;
impl AudioArchivePort for NoArchive {
    fn remove(&self, _: &str) -> Result<()> {
        panic!("This acceptance must not mutate retained audio")
    }
    fn export(&self, _: &str, _: &str) -> Result<()> {
        panic!("This acceptance must not export files")
    }
}
#[derive(Default)]
struct FakeDelivery {
    captures: std::sync::atomic::AtomicUsize,
    copies: Mutex<Vec<String>>,
}
impl TextDelivery for FakeDelivery {
    fn capture_target(&self) -> Result<DictationTarget> {
        self.captures.fetch_add(1, Ordering::SeqCst);
        Ok(DictationTarget::none())
    }
    fn deliver(
        &self,
        _: &DictationTarget,
        text: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<DeliveryOutcome> {
        assert!(allowed(), "Controller must not deliver a cancelled session");
        self.copies.lock().unwrap().push(text.into());
        Ok(DeliveryOutcome {
            copied: true,
            paste_attempted: false,
            reason: "paste_unsupported".into(),
        })
    }
    fn discard_target(&self, _: &DictationTarget) -> Result<()> {
        Ok(())
    }
    fn status(&self) -> Value {
        json!({"pasteAvailable":false,"pastePermission":"unavailable"})
    }
}
struct CoreApplication(Arc<MeetingService>);
impl Application for CoreApplication {
    fn active(&self) -> Result<Option<String>> {
        Ok(self.0.get_status()?["activeMeetingId"]
            .as_str()
            .map(str::to_owned))
    }
    fn ready(&self) -> Result<()> {
        Ok(())
    } // Synthetic capture requires no hardware readiness check.
    fn create(&self) -> Result<String> {
        Ok(self.0.handle("create_dictation", json!({}))?["id"]
            .as_str()
            .unwrap()
            .into())
    }
    fn start(&self, id: &str) -> Result<()> {
        self.0
            .handle(
                "start_recording",
                json!({"meetingId":id,"microphone":true,"systemAudio":false}),
            )
            .map(|_| ())
    }
    fn stop(&self, id: &str) -> Result<Value> {
        self.0.handle("stop_recording", json!({"meetingId":id}))
    }
}
fn service(
    data: &Path,
    observer: Arc<Mutex<Option<Weak<GlobalDictation>>>>,
) -> (Arc<MeetingService>, Arc<SyntheticCapture>) {
    let capture = Arc::new(SyntheticCapture::default());
    let service = MeetingService::new(
        Arc::new(SqliteStore::open(data.join("global-dictation-acceptance.sqlite3")).unwrap()),
        Adapters {
            capture: capture.clone(),
            intelligence: Arc::new(NoIntelligence),
            publisher: Arc::new(NoPublication),
            secrets: Arc::new(NoSecrets),
            accounts: Arc::new(NoAccounts),
            archive: Arc::new(NoArchive),
        },
        Arc::new(move |name, payload| {
            if name == "meeting_updated" {
                let target = observer.lock().unwrap().clone();
                if let Some(target) = target.and_then(|weak| weak.upgrade()) {
                    target.observe_meeting(&payload["meeting"]);
                }
            }
        }),
    )
    .unwrap();
    (service, capture)
}
fn call(service: &Arc<MeetingService>, method: &str, params: Value) -> Value {
    service.handle(method, params).unwrap()
}
fn toggle(controller: &GlobalDictation) -> Value {
    controller
        .toggle(controller.prepare_target().unwrap())
        .unwrap()
}

#[test]
fn global_dictation_real_core_saves_and_delivers_original_drained_speech_without_meeting_automation(
) {
    let data = tempfile::tempdir().unwrap();
    let observer = Arc::new(Mutex::new(None));
    let (service, capture) = service(data.path(), observer.clone());
    // Even explicitly enabled meeting automations must never run for voice notes.
    call(
        &service,
        "update_settings",
        json!({"changes":{"autoSummary":true,"notionAutoSync":true,"notionTarget":"approved-fixture-database"}}),
    );
    let delivery = Arc::new(FakeDelivery::default());
    let controller = Arc::new(GlobalDictation::new(
        Arc::new(CoreApplication(service.clone())),
        delivery.clone(),
        Arc::new(|_, _| {}),
    ));
    *observer.lock().unwrap() = Some(Arc::downgrade(&controller));
    let started = toggle(&controller);
    assert_eq!(started["state"], "listening");
    let id = started["meetingId"].as_str().unwrap().to_owned();
    let recording = service.meeting(&id).unwrap();
    assert_eq!(recording["kind"], "dictation");
    assert_eq!(recording["status"], "recording");
    assert_eq!(*capture.sources.lock().unwrap(), vec![(false, true)]);
    assert_eq!(capture.configured.lock().unwrap()[0]["diarization"], false);
    call(
        &service,
        "update_meeting",
        json!({"id":id,"changes":{"notes":"My separate edited note must not replace raw speech in delivery."}}),
    );
    let finished = toggle(&controller);
    assert_eq!(finished["state"], "copied");
    assert_eq!(capture.stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        *delivery.copies.lock().unwrap(),
        vec![format!("{START} {FINAL}")]
    );
    assert_eq!(
        delivery.captures.load(Ordering::SeqCst),
        1,
        "Stopping must retain the original target rather than recapturing focus"
    );
    let saved = service.meeting(&id).unwrap();
    assert_eq!(saved["status"], "complete");
    assert_eq!(saved["summary"], "");
    assert_eq!(saved["notionStatus"], "idle");
    assert_eq!(
        saved["notes"],
        "My separate edited note must not replace raw speech in delivery."
    );
    assert_eq!(saved["segments"].as_array().unwrap().len(), 2);
    assert_eq!(saved["segments"][1]["text"], FINAL);
    assert!(saved["segments"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["source"] == "microphone"));
    assert_eq!(
        service.get_status().unwrap()["activeMeetingId"],
        Value::Null
    );
    controller.shutdown().unwrap();
    call(&service, "shutdown", json!({}));
    drop(controller);
    drop(service);
    let (reopened, _) = self::service(data.path(), Arc::new(Mutex::new(None)));
    let persisted = reopened.meeting(&id).unwrap();
    assert_eq!(persisted["kind"], "dictation");
    assert_eq!(persisted["notes"], saved["notes"]);
    assert_eq!(persisted["segments"], saved["segments"]);
    assert_eq!(persisted["summary"], "");
    assert_eq!(persisted["notionUrl"], "");
    call(&reopened, "shutdown", json!({}));
}

#[test]
fn global_dictation_real_core_refuses_an_existing_meeting_without_stopping_or_copying_it() {
    let data = tempfile::tempdir().unwrap();
    let (service, capture) = service(data.path(), Arc::new(Mutex::new(None)));
    let meeting = call(
        &service,
        "create_meeting",
        json!({"title":"Synthetic existing meeting"}),
    );
    let id = meeting["id"].as_str().unwrap();
    call(
        &service,
        "start_recording",
        json!({"meetingId":id,"microphone":true,"systemAudio":true}),
    );
    let delivery = Arc::new(FakeDelivery::default());
    let controller = GlobalDictation::new(
        Arc::new(CoreApplication(service.clone())),
        delivery.clone(),
        Arc::new(|_, _| {}),
    );
    assert!(controller
        .toggle(controller.prepare_target().unwrap())
        .is_err());
    assert_eq!(capture.stops.load(Ordering::SeqCst), 0);
    assert_eq!(service.get_status().unwrap()["activeMeetingId"], id);
    assert_eq!(service.meeting(id).unwrap()["status"], "recording");
    assert!(delivery.copies.lock().unwrap().is_empty());
    let draft = call(&service, "create_dictation", json!({}));
    assert!(service
        .handle("start_recording", json!({"meetingId":draft["id"]}))
        .is_err());
    assert_eq!(*capture.sources.lock().unwrap(), vec![(true, true)]);
    assert_eq!(capture.stops.load(Ordering::SeqCst), 0);
    controller.shutdown().unwrap();
    assert_eq!(
        capture.stops.load(Ordering::SeqCst),
        0,
        "Controller shutdown must not stop an unowned meeting"
    );
    call(&service, "shutdown", json!({}));
    assert_eq!(service.meeting(id).unwrap()["status"], "interrupted");
}

#[test]
fn dictation_preferences_survive_sqlite_reopen() {
    let data = tempfile::tempdir().unwrap();
    let observer = Arc::new(Mutex::new(None));
    let (app, capture) = service(data.path(), observer.clone());
    let config = json!({"enabled":false,"shortcut":"Alt+Shift+D","autoPaste":false,"cleanupEnabled":true,"cleanupPrompt":"Keep words and add paragraph breaks."});
    call(
        &app,
        "update_settings",
        json!({"changes":{"globalDictation":config}}),
    );
    drop(app);
    drop(capture);
    let (app, _) = service(data.path(), observer);
    assert_eq!(app.get_settings().unwrap()["globalDictation"], config);
}
