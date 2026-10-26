//! Explicit global voice-note capture. The host keeps focus in the user's app.
use crate::adapters::{
    capture::CaptureEngine,
    dictation_target::{DictationTarget, TextDelivery},
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tuck_core::{
    ports::{Emit, RecordingPort, Result},
    AppError, MeetingService,
};

#[cfg(test)]
mod acceptance;

pub const SHORTCUT: &str = "CommandOrControl+Shift+Space";

pub trait Application: Send + Sync {
    fn active(&self) -> Result<Option<String>>;
    fn ready(&self) -> Result<()>;
    fn create(&self) -> Result<String>;
    fn start(&self, id: &str) -> Result<()>;
    fn stop(&self, id: &str) -> Result<Value>;
    fn cleanup(&self, _text: &str, _prompt: &str) -> Result<String> {
        Err(AppError::unavailable("Cleanup is unavailable."))
    }
    fn save_cleanup(&self, _id: &str, _original: &str, _cleaned: &str) -> Result<()> { Ok(()) }

}
pub struct NativeApplication {
    pub service: Arc<MeetingService>,
    pub capture: Arc<CaptureEngine>,
}
impl Application for NativeApplication {
    fn active(&self) -> Result<Option<String>> {
        Ok(self.service.get_status()?["activeMeetingId"]
            .as_str()
            .map(str::to_owned))
    }
    fn ready(&self) -> Result<()> {
        let mut settings = self.service.get_settings()?;
        settings["diarization"] = json!(false);
        self.capture.configure(&settings)?;
        let r = self.capture.readiness_for_dictation()?;
        if ["modelReady", "helperReady", "microphoneAvailable"]
            .iter()
            .all(|key| r[*key] == true)
        {
            Ok(())
        } else {
            Err(AppError::unavailable("Set up the speech model and microphone in Settings → Audio & models before dictating. Nothing has started."))
        }
    }
    fn cleanup(&self, text: &str, prompt: &str) -> Result<String> {
        // Wall-clock cap includes credential refresh as well as HTTP inference.
        // The adapter uses a non-queuing gate and a five-second network timeout.
        let service = self.service.clone();
        let text = text.to_owned();
        let prompt = prompt.to_owned();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new().name("tuck-dictation-cleanup".into()).spawn(move || {
            let _ = send.send(service.format_dictation(&text, &prompt));
        }).map_err(|_| AppError::unavailable("Cleanup could not start."))?;
        receive.recv_timeout(std::time::Duration::from_secs(6))
            .map_err(|_| AppError::unavailable("Cleanup took too long."))?
    }
    fn save_cleanup(&self, id: &str, original: &str, cleaned: &str) -> Result<()> {
        self.service.save_dictation_cleanup(id, original, cleaned)
    }
    fn create(&self) -> Result<String> {
        self.service.handle("create_dictation", json!({}))?["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| AppError::unavailable("The voice note could not be created."))
    }
    fn start(&self, id: &str) -> Result<()> {
        self.service
            .handle(
                "start_recording",
                json!({"meetingId":id,"microphone":true,"systemAudio":false}),
            )
            .map(|_| ())
    }
    fn stop(&self, id: &str) -> Result<Value> {
        self.service
            .handle("stop_recording", json!({"meetingId":id}))
    }
}
pub struct PreparedToggle {
    target: Option<DictationTarget>,
    generation: u64,
    ticket: u64,
}
struct Session {
    id: String,
    target: DictationTarget,
}
struct State {
    phase: &'static str,
    message: String,
    cleanup_message: String,
    meeting_id: Option<String>,
    session: Option<Session>,
    shortcut_available: bool,
    config: crate::dictation_config::Config,
    prepared: Option<u64>,
    prepared_target: Option<DictationTarget>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            phase: "idle",
            message: String::new(),
            cleanup_message: String::new(),
            meeting_id: None,
            session: None,
            shortcut_available: false,
            config: crate::dictation_config::Config::default(),
            prepared: None,
            prepared_target: None,
        }
    }
}
pub struct GlobalDictation {
    app: Arc<dyn Application>,
    delivery: Arc<dyn TextDelivery>,
    emit: Emit,
    state: Mutex<State>,
    operation: Mutex<()>,
    generation: AtomicU64,
    tickets: AtomicU64,
    closing: AtomicBool,
}
impl GlobalDictation {
    pub fn new(app: Arc<dyn Application>, delivery: Arc<dyn TextDelivery>, emit: Emit) -> Self {
        Self {
            app,
            delivery,
            emit,
            state: Mutex::new(State::default()),
            operation: Mutex::new(()),
            generation: AtomicU64::new(0),
            tickets: AtomicU64::new(0),
            closing: AtomicBool::new(false),
        }
    }
    pub fn status(&self) -> Value {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut result = self.delivery.status();
        result["shortcut"] = json!(s.config.shortcut);
        result["enabled"] = json!(s.config.enabled);
        result["autoPaste"] = json!(s.config.auto_paste);
        result["cleanupEnabled"] = json!(s.config.cleanup_enabled);
        result["cleanupPrompt"] = json!(s.config.cleanup_prompt);
        result["cleanupDefaultPrompt"] = json!(tuck_core::dictation_cleanup::DEFAULT_PROMPT);
        result["cleanupMessage"] = json!(s.cleanup_message);
        result["pasteAvailable"] = json!(s.config.auto_paste && result["pasteAvailable"] == true);
        result["shortcutAvailable"] = json!(s.shortcut_available);
        result["state"] = json!(s.phase);
        result["message"] = json!(s.message);
        result["meetingId"] = json!(s.meeting_id);
        result
    }
    pub fn configuration(&self) -> crate::dictation_config::Config {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .config
            .clone()
    }
    /// Keep settings stable from target capture through completion and delivery.
    pub fn configure(
        &self,
        config: crate::dictation_config::Config,
        apply: impl FnOnce(
            &crate::dictation_config::Config,
            &crate::dictation_config::Config,
        ) -> Result<bool>,
    ) -> Result<Value> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| AppError::unavailable("Finish dictation before changing its settings."))?;
        let old = {
            let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if self.closing.load(Ordering::Acquire) || s.session.is_some() || s.prepared.is_some() {
                return Err(AppError::unavailable(
                    "Finish dictation before changing its settings.",
                ));
            }
            s.config.clone()
        };
        let available = apply(&old, &config)?;
        {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.config = config;
            s.shortcut_available = available;
        }
        Ok(self.changed())
    }
    fn changed(&self) -> Value {
        let status = self.status();
        (self.emit)("global_dictation_updated", status.clone());
        status
    }
    fn phase(&self, phase: &'static str, message: impl Into<String>) {
        {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.phase = phase;
            s.message = message.into();
        }
        self.changed();
    }
    pub fn registered(&self, available: bool) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shortcut_available = available;
        self.changed();
    }
    fn allowed(&self, generation: u64) -> bool {
        !self.closing.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == generation
    }
    /// Capture identity before yielding the native shortcut callback. No window is activated.
    pub fn prepare_target(&self) -> Result<PreparedToggle> {
        let _operation = self.operation.try_lock().map_err(|_| {
            AppError::unavailable(
                "Dictation is finishing the previous action. Please wait a moment.",
            )
        })?;
        if self.closing.load(Ordering::Acquire) {
            return Err(AppError::unavailable("Tuck is closing."));
        }
        let config = self.configuration();
        if !config.enabled {
            return Err(AppError::unavailable(
                "Dictation anywhere is off. Turn it on in Settings → Audio & models.",
            ));
        }
        let generation = self.generation.load(Ordering::Acquire);
        let starting = {
            let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if s.prepared.is_some() {
                return Err(AppError::unavailable(
                    "Dictation is already preparing that shortcut.",
                ));
            }
            s.session.is_none()
        };
        let target = if starting {
            Some(if config.auto_paste {
                self.delivery.capture_target()?
            } else {
                DictationTarget::none()
            })
        } else {
            None
        };
        let ticket = self.tickets.fetch_add(1, Ordering::AcqRel);
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.prepared = Some(ticket);
        s.prepared_target = target.clone();
        Ok(PreparedToggle {
            target,
            generation,
            ticket,
        })
    }
    pub fn abandon_prepared(&self) {
        let target = {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.prepared = None;
            s.prepared_target.take()
        };
        if let Some(target) = target {
            let _ = self.delivery.discard_target(&target);
        }
    }
    pub fn toggle(&self, prepared: PreparedToggle) -> Result<Value> {
        let Ok(_operation) = self.operation.try_lock() else {
            return Err(AppError::unavailable(
                "Dictation is finishing the previous action. Please wait a moment.",
            ));
        };
        let generation = prepared.generation;
        let owned = {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if !self.allowed(generation) || s.prepared != Some(prepared.ticket) {
                drop(s);
                if let Some(target) = prepared.target {
                    let _ = self.delivery.discard_target(&target);
                }
                return Err(AppError::unavailable(
                    "That dictation action was cancelled. Press the shortcut again when ready.",
                ));
            }
            s.prepared = None;
            s.prepared_target = None;
            s.session.take()
        };
        let target = prepared.target;
        let result = if let Some(session) = owned {
            self.finish(session, generation, true)
        } else {
            self.begin(target, generation)
        };
        if let Err(error) = &result {
            self.phase("error", &error.message);
        }
        result.map(|()| self.status())
    }
    fn begin(&self, target: Option<DictationTarget>, generation: u64) -> Result<()> {
        let target = target.ok_or_else(|| {
            AppError::unavailable("Press the dictation shortcut again to start a new voice note.")
        })?;
        let result = (|| {
            if self.app.active()?.is_some() {
                return Err(AppError::invalid("Finish the current recording before dictating. Its recording has not been changed."));
            }
            self.state.lock().unwrap_or_else(|p| p.into_inner()).cleanup_message.clear();
            self.phase("starting", "Preparing your microphone…");
            self.app.ready()?;
            if !self.allowed(generation) {
                return Ok(());
            }
            let id = self.app.create()?;
            self.state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .meeting_id = Some(id.clone());
            if !self.allowed(generation) {
                return Ok(());
            }
            // Establish ownership before start can emit an interruption callback.
            self.state.lock().unwrap_or_else(|p| p.into_inner()).session = Some(Session {
                id: id.clone(),
                target: target.clone(),
            });
            if let Err(error) = self.app.start(&id) {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if state
                    .session
                    .as_ref()
                    .is_some_and(|session| session.id == id)
                {
                    state.session = None;
                }
                return Err(error);
            }
            if !self.allowed(generation) {
                if self.app.active()?.as_deref() == Some(&id) {
                    self.app.stop(&id)?;
                }
                self.state.lock().unwrap_or_else(|p| p.into_inner()).session = None;
                return Ok(());
            }
            if self.app.active()?.as_deref() != Some(&id) {
                self.observe_meeting(&json!({"id":id,"status":"interrupted"}));
                return Ok(());
            }
            {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                if state
                    .session
                    .as_ref()
                    .is_some_and(|session| session.id == id)
                {
                    state.phase = "listening";
                    state.message = "Listening. Press the shortcut again to finish.".into();
                }
            }
            self.changed();
            Ok(())
        })();
        if result.is_err() || !self.allowed(generation) {
            let _ = self.delivery.discard_target(&target);
        }
        result
    }
    fn finish(&self, session: Session, generation: u64, insert: bool) -> Result<()> {
        let result = (|| {
            if self.app.active()?.as_deref() != Some(&session.id) {
                self.phase(
                    "idle",
                    "This voice note was stopped in Tuck. Nothing was copied or inserted.",
                );
                return Ok(());
            }
            self.phase(
                "processing",
                "Finishing your words. Your voice note stays saved in Tuck.",
            );
            let completed = self.app.stop(&session.id)?;
            if !insert || !self.allowed(generation) {
                self.phase("idle", "Voice note saved without copying or inserting.");
                return Ok(());
            }
            let mut text = completed_speech(&completed)?;
            if text.is_empty() {
                self.phase(
                    "idle",
                    "No speech was recognized. Your clipboard has not been changed.",
                );
                return Ok(());
            }
            let config = self.configuration();
            if config.cleanup_enabled {
                self.phase("cleaning", "Adding punctuation and formatting while preserving your words…");
                let cleaned = self.app.cleanup(&text, &config.cleanup_prompt)
                    .and_then(|candidate| tuck_core::dictation_cleanup::validate(&text, &candidate));
                if !self.allowed(generation) { return Ok(()); }
                match cleaned {
                    Ok(candidate) => {
                        if self.app.save_cleanup(&session.id, &text, &candidate).is_ok() {
                            text = candidate;
                        } else {
                            self.state.lock().unwrap_or_else(|p| p.into_inner()).cleanup_message = "Original transcript kept because the saved note changed.".into();
                        }
                    }
                    Err(error) => {
                        let detail: String = error.message.chars().take(240).collect();
                        self.state.lock().unwrap_or_else(|p| p.into_inner()).cleanup_message = format!("Original transcript kept. {detail}");
                    }
                }
            }
            let outcome = self
                .delivery
                .deliver(&session.target, &text, &|| self.allowed(generation))?;
            if outcome.paste_attempted {
                self.phase("paste_sent", "Copied. Paste sent to your text field.");
            } else if outcome.copied {
                let config = self.configuration();
                let message = match outcome.reason.as_str() {
                    _ if !config.auto_paste => "Copied. Paste your words wherever you need them.",
                    "permission_denied" | "accessibility_denied" | "accessibility_permission" => "Copied. Allow Tuck in Accessibility settings for automatic insertion.",
                    "unsupported" | "unsupported_platform" | "paste_unsupported" => "Copied. Automatic insertion is not available on this platform yet.",
                    _ => "Copied. Automatic insertion was skipped because the original text field could not be confirmed. Paste when you are ready.",
                };
                self.phase("copied", message);
            } else {
                self.phase("idle", "Voice note saved without copying or inserting.");
            }
            Ok(())
        })();
        let _ = self.delivery.discard_target(&session.target);
        result
    }
    /// Cancel delivery first, then drain/save owned capture. Never affect another recording.
    pub fn cancel(&self) -> Result<Value> {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
        self.abandon_prepared();
        let owned = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .session
            .take();
        if let Some(session) = owned {
            if let Err(error) = self.finish(session, 0, false) {
                self.phase("error", &error.message);
                return Err(error);
            }
        }
        self.phase("idle", "Voice note saved without copying or inserting.");
        Ok(self.status())
    }
    pub fn shutdown(&self) -> Result<()> {
        self.closing.store(true, Ordering::Release);
        self.cancel().map(|_| ())
    }
    /// Core events retire an externally stopped/interrupted voice note without inserting it.
    pub fn observe_meeting(&self, meeting: &Value) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.session
            .as_ref()
            .is_some_and(|owned| meeting["id"] == owned.id)
            && !matches!(
                meeting["status"].as_str(),
                Some("recording" | "paused" | "processing")
            )
        {
            let target = s.session.take().expect("owned session").target;
            s.phase = "idle";
            s.message = "Voice note saved in Tuck. Nothing was copied or inserted.".into();
            drop(s);
            let _ = self.delivery.discard_target(&target);
            self.changed();
        }
    }
}
fn completed_speech(meeting: &Value) -> Result<String> {
    if meeting["kind"] != "dictation" || meeting["status"] != "complete" {
        return Err(AppError::unavailable("Dictation did not finish completely. Your saved voice note is in Tuck; nothing was copied or inserted."));
    }
    let mut segments = meeting["segments"]
        .as_array()
        .ok_or_else(|| {
            AppError::unavailable(
                "Recognized speech is unavailable. Open the saved voice note in Tuck.",
            )
        })?
        .iter()
        .collect::<Vec<_>>();
    segments.sort_by(|a, b| {
        a["start"]
            .as_f64()
            .unwrap_or(0.)
            .total_cmp(&b["start"].as_f64().unwrap_or(0.))
    });
    let mut text = String::new();
    let mut chars = 0;
    for segment in segments {
        let speech = segment["originalText"]
            .as_str()
            .or_else(|| segment["text"].as_str())
            .unwrap_or("")
            .trim();
        if speech.is_empty() {
            continue;
        }
        chars += speech.chars().count() + usize::from(!text.is_empty());
        if chars > 200_000 {
            return Err(AppError::unavailable("Your voice note is saved, but too long for automatic insertion. Open it in Tuck to use its text."));
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(speech);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::dictation_target::DeliveryOutcome;
    use std::{
        sync::{atomic::AtomicUsize, mpsc, Condvar},
        thread,
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct Drain {
        held: Mutex<bool>,
        wake: Condvar,
        entered: Mutex<Option<mpsc::Sender<()>>>,
    }
    struct FakeApp {
        active: Mutex<Option<String>>,
        log: Arc<Mutex<Vec<&'static str>>>,
        creates: AtomicUsize,
        stops: AtomicUsize,
        ready: AtomicBool,
        start_fails: AtomicBool,
        on_start: Mutex<Option<Box<dyn Fn() + Send>>>,
        stop_fails: AtomicBool,
        complete: Mutex<Value>,
        cleanup_text: Mutex<Option<String>>,
        cleanup_drain: Drain,
        cleanup_saves: Mutex<Vec<String>>,
        drain: Drain,
    }
    impl Application for FakeApp {
        fn cleanup(&self, _: &str, _: &str) -> Result<String> {
            self.log.lock().unwrap().push("cleanup");
            let mut held = self.cleanup_drain.held.lock().unwrap();
            if *held {
                if let Some(entered) = self.cleanup_drain.entered.lock().unwrap().take() { let _ = entered.send(()); }
                while *held { held = self.cleanup_drain.wake.wait(held).unwrap(); }
            }
            self.cleanup_text.lock().unwrap().clone().ok_or_else(|| AppError::unavailable("AI unavailable"))
        }
        fn save_cleanup(&self, _: &str, _: &str, cleaned: &str) -> Result<()> {
            self.cleanup_saves.lock().unwrap().push(cleaned.to_owned());
            Ok(())
        }
        fn active(&self) -> Result<Option<String>> {
            Ok(self.active.lock().unwrap().clone())
        }
        fn ready(&self) -> Result<()> {
            self.log.lock().unwrap().push("ready");
            if self.ready.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(AppError::unavailable("Set up your microphone."))
            }
        }
        fn create(&self) -> Result<String> {
            self.creates.fetch_add(1, Ordering::Relaxed);
            Ok("voice-note".into())
        }
        fn start(&self, id: &str) -> Result<()> {
            self.log.lock().unwrap().push("start");
            if self.start_fails.load(Ordering::Acquire) {
                return Err(AppError::unavailable(
                    "Capture failed; partial speech is saved.",
                ));
            }
            *self.active.lock().unwrap() = Some(id.into());
            let on_start = self.on_start.lock().unwrap().take();
            if let Some(on_start) = on_start {
                on_start();
            }
            Ok(())
        }
        fn stop(&self, id: &str) -> Result<Value> {
            assert_eq!(id, "voice-note");
            self.stops.fetch_add(1, Ordering::Relaxed);
            let mut held = self.drain.held.lock().unwrap();
            if let Some(sender) = self.drain.entered.lock().unwrap().take() {
                sender.send(()).unwrap();
            }
            while *held {
                held = self.drain.wake.wait(held).unwrap();
            }
            *self.active.lock().unwrap() = None;
            if self.stop_fails.load(Ordering::Acquire) {
                Err(AppError::unavailable("Capture did not drain completely."))
            } else {
                Ok(self.complete.lock().unwrap().clone())
            }
        }
    }
    struct FakeDelivery {
        log: Arc<Mutex<Vec<&'static str>>>,
        copies: Mutex<Vec<String>>,
    }
    impl TextDelivery for FakeDelivery {
        fn capture_target(&self) -> Result<DictationTarget> {
            self.log.lock().unwrap().push("target");
            Ok(DictationTarget::none())
        }
        fn deliver(
            &self,
            _: &DictationTarget,
            text: &str,
            allowed: &dyn Fn() -> bool,
        ) -> Result<DeliveryOutcome> {
            assert!(allowed());
            self.copies.lock().unwrap().push(text.into());
            Ok(DeliveryOutcome {
                copied: true,
                paste_attempted: false,
                reason: "no_editable_target".into(),
            })
        }
        fn discard_target(&self, _: &DictationTarget) -> Result<()> {
            Ok(())
        }
        fn status(&self) -> Value {
            json!({"pasteAvailable":false,"pastePermission":"unavailable"})
        }
    }
    fn setup() -> (Arc<GlobalDictation>, Arc<FakeApp>, Arc<FakeDelivery>) {
        let log = Arc::new(Mutex::new(vec![]));
        let app = Arc::new(FakeApp {
            active: Mutex::new(None),
            log: log.clone(),
            creates: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            ready: AtomicBool::new(true),
            start_fails: AtomicBool::new(false),
            on_start: Mutex::new(None),
            stop_fails: AtomicBool::new(false),
            complete: Mutex::new(
                json!({"id":"voice-note","kind":"dictation","status":"complete","notes":"Typed notes must not be inserted","segments":[{"start":3.,"text":"corrected text","originalText":"final speech"},{"start":0.,"text":"Hello"}]}),
            ),
            cleanup_text: Mutex::new(None),
            cleanup_drain: Drain::default(),
            cleanup_saves: Mutex::new(Vec::new()),
            drain: Drain::default(),
        });
        let delivery = Arc::new(FakeDelivery {
            log,
            copies: Mutex::new(vec![]),
        });
        let controller = Arc::new(GlobalDictation::new(
            app.clone(),
            delivery.clone(),
            Arc::new(|_, _| {}),
        ));
        (controller, app, delivery)
    }
    fn start(controller: &GlobalDictation) {
        let target = controller.prepare_target().unwrap();
        assert_eq!(controller.toggle(target).unwrap()["state"], "listening");
    }
    #[test]
    fn cleanup_is_opt_in_and_word_changes_fall_back_to_original_delivery() {
        for candidate in [Some("Hello, final speech!"), Some("Hello invented words"), None] {
            let (c, app, delivery) = setup();
            c.configure(crate::dictation_config::Config { cleanup_enabled: true, ..Default::default() }, |_, _| Ok(true)).unwrap();
            *app.cleanup_text.lock().unwrap() = candidate.map(str::to_owned);
            start(&c);
            let status = c.toggle(c.prepare_target().unwrap()).unwrap();
            let expected = if candidate == Some("Hello, final speech!") { "Hello, final speech!" } else { "Hello final speech" };
            assert_eq!(*delivery.copies.lock().unwrap(), vec![expected]);
            assert_eq!(app.cleanup_saves.lock().unwrap().len(), usize::from(candidate == Some("Hello, final speech!")));
            assert_eq!(status["cleanupMessage"].as_str().unwrap().is_empty(), candidate == Some("Hello, final speech!"));
        }
        let (c, app, _) = setup();
        start(&c);
        c.toggle(c.prepare_target().unwrap()).unwrap();
        assert!(!app.log.lock().unwrap().contains(&"cleanup"));
    }
    #[test]
    fn cancellation_during_cleanup_prevents_late_save_copy_and_paste() {
        let (c, app, delivery) = setup();
        c.configure(crate::dictation_config::Config { cleanup_enabled: true, ..Default::default() }, |_, _| Ok(true)).unwrap();
        *app.cleanup_text.lock().unwrap() = Some("Hello, final speech!".into());
        start(&c);
        *app.cleanup_drain.held.lock().unwrap() = true;
        let (entered, wait) = mpsc::channel();
        *app.cleanup_drain.entered.lock().unwrap() = Some(entered);
        let stopping = c.clone();
        let finish = thread::spawn(move || stopping.toggle(stopping.prepare_target().unwrap()));
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        let cancelling = c.clone();
        let cancel = thread::spawn(move || cancelling.cancel());
        let until = Instant::now() + Duration::from_secs(2);
        while c.generation.load(Ordering::Acquire) == 0 && Instant::now() < until { thread::yield_now(); }
        assert_ne!(c.generation.load(Ordering::Acquire), 0);
        *app.cleanup_drain.held.lock().unwrap() = false;
        app.cleanup_drain.wake.notify_all();
        finish.join().unwrap().unwrap();
        cancel.join().unwrap().unwrap();
        assert!(delivery.copies.lock().unwrap().is_empty());
        assert!(app.cleanup_saves.lock().unwrap().is_empty());
    }
    #[test]
    fn disabled_dictation_never_captures_a_target_or_starts_audio() {
        let (c, app, delivery) = setup();
        let config = crate::dictation_config::Config {
            enabled: false,
            ..Default::default()
        };
        c.configure(config, |_, _| Ok(false)).unwrap();
        assert!(c.prepare_target().is_err());
        assert_eq!(app.creates.load(Ordering::Relaxed), 0);
        assert!(delivery.log.lock().unwrap().is_empty());
        assert_eq!(c.status()["enabled"], false);
    }
    #[test]
    fn clipboard_only_does_not_inspect_a_target_and_active_config_is_stable() {
        let (c, _, delivery) = setup();
        let config = crate::dictation_config::Config {
            auto_paste: false,
            ..Default::default()
        };
        c.configure(config.clone(), |_, _| Ok(true)).unwrap();
        let prepared = c.prepare_target().unwrap();
        assert!(c
            .configure(Default::default(), |_, _| panic!(
                "must not save while prepared"
            ))
            .is_err());
        c.toggle(prepared).unwrap();
        assert!(c
            .configure(Default::default(), |_, _| panic!(
                "must not save during capture"
            ))
            .is_err());
        assert_eq!(c.configuration(), config);
        c.toggle(c.prepare_target().unwrap()).unwrap();
        assert!(!delivery.log.lock().unwrap().contains(&"target"));
        assert_eq!(
            delivery.copies.lock().unwrap().as_slice(),
            ["Hello final speech"]
        );
        assert_eq!(
            c.status()["message"],
            "Copied. Paste your words wherever you need them."
        );
    }
    #[test]
    fn a_failed_configuration_apply_does_not_change_current_status() {
        let (c, _, _) = setup();
        let before = c.configuration();
        assert!(c
            .configure(
                crate::dictation_config::Config {
                    enabled: false,
                    ..Default::default()
                },
                |_, _| Err(AppError::unavailable("conflict"))
            )
            .is_err());
        assert_eq!(c.configuration(), before);
    }
    #[test]
    fn prepared_actions_are_single_use_and_cannot_start_after_cancel() {
        let (c, app, delivery) = setup();
        let prepared = c.prepare_target().unwrap();
        assert!(c.prepare_target().is_err());
        c.cancel().unwrap();
        assert!(c.toggle(prepared).is_err());
        assert_eq!(app.creates.load(Ordering::Relaxed), 0);
        assert!(delivery.copies.lock().unwrap().is_empty());
        start(&c);
    }
    #[test]
    fn interruption_emitted_during_start_retires_ownership_before_listening() {
        let (c, app, delivery) = setup();
        let controller = Arc::downgrade(&c);
        let capture = Arc::downgrade(&app);
        *app.on_start.lock().unwrap() = Some(Box::new(move || {
            let controller = controller.upgrade().unwrap();
            assert!(controller.state.lock().unwrap().session.is_some());
            // Core interruption publishes the durable terminal meeting before clearing
            // active capture. Reproduce that ordering inside the synchronous start call.
            controller.observe_meeting(&json!({"id":"voice-note","status":"interrupted"}));
            *capture.upgrade().unwrap().active.lock().unwrap() = None;
        }));

        let result = c.toggle(c.prepare_target().unwrap()).unwrap();

        assert_eq!(result["state"], "idle");
        assert!(c.state.lock().unwrap().session.is_none());
        assert!(app.active().unwrap().is_none());
        assert_eq!(app.stops.load(Ordering::Relaxed), 0);
        assert!(delivery.copies.lock().unwrap().is_empty());
    }
    #[test]
    fn focus_identity_precedes_readiness_and_only_complete_original_speech_is_copied() {
        let (c, app, delivery) = setup();
        assert!(delivery.copies.lock().unwrap().is_empty());
        start(&c);
        assert_eq!(*app.log.lock().unwrap(), vec!["target", "ready", "start"]);
        assert_eq!(
            c.toggle(c.prepare_target().unwrap()).unwrap()["state"],
            "copied"
        );
        assert_eq!(*delivery.copies.lock().unwrap(), vec!["Hello final speech"]);
        assert_eq!(app.stops.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn readiness_unrelated_capture_and_failed_start_never_touch_clipboard_or_other_capture() {
        for mode in ["unready", "other", "start-failure"] {
            let (c, app, delivery) = setup();
            match mode {
                "unready" => app.ready.store(false, Ordering::Release),
                "other" => *app.active.lock().unwrap() = Some("meeting-other".into()),
                _ => app.start_fails.store(true, Ordering::Release),
            }
            assert!(c.toggle(c.prepare_target().unwrap()).is_err());
            assert_eq!(app.stops.load(Ordering::Relaxed), 0);
            assert!(delivery.copies.lock().unwrap().is_empty());
            if mode == "other" {
                assert_eq!(app.active().unwrap().as_deref(), Some("meeting-other"));
                assert_eq!(app.creates.load(Ordering::Relaxed), 0);
            }
        }
    }
    #[test]
    fn incomplete_failed_or_empty_stop_never_delivers_partial_text() {
        for mode in ["interrupted", "wrong-kind", "stop-failure", "empty"] {
            let (c, app, delivery) = setup();
            start(&c);
            match mode {
                "interrupted" => app.complete.lock().unwrap()["status"] = json!("interrupted"),
                "wrong-kind" => app.complete.lock().unwrap()["kind"] = json!("meeting"),
                "stop-failure" => app.stop_fails.store(true, Ordering::Release),
                _ => app.complete.lock().unwrap()["segments"] = json!([]),
            }
            let result = c.toggle(c.prepare_target().unwrap());
            if mode == "empty" {
                assert_eq!(result.unwrap()["state"], "idle");
            } else {
                assert!(result.is_err());
            }
            assert!(delivery.copies.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn cancel_and_shutdown_drain_save_without_delivery_and_shutdown_prevents_future_capture() {
        for shutdown in [false, true] {
            let (c, app, delivery) = setup();
            start(&c);
            if shutdown {
                c.shutdown().unwrap();
            } else {
                c.cancel().unwrap();
            }
            assert_eq!(app.stops.load(Ordering::Relaxed), 1);
            assert!(app.active().unwrap().is_none());
            assert!(delivery.copies.lock().unwrap().is_empty());
            if shutdown {
                assert!(c.prepare_target().is_err());
                assert!(c
                    .toggle(PreparedToggle {
                        target: Some(DictationTarget::none()),
                        generation: 0,
                        ticket: 0
                    })
                    .is_err());
            }
        }
    }
    #[test]
    fn drain_finishes_before_delivery_and_rapid_toggle_cannot_replace_original_target() {
        let (c, app, delivery) = setup();
        start(&c);
        let (tx, rx) = mpsc::channel();
        *app.drain.held.lock().unwrap() = true;
        *app.drain.entered.lock().unwrap() = Some(tx);
        let pending = c.clone();
        let stopping = thread::spawn(move || {
            pending
                .prepare_target()
                .and_then(|request| pending.toggle(request))
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(delivery.copies.lock().unwrap().is_empty());
        assert!(c.prepare_target().is_err());
        *app.drain.held.lock().unwrap() = false;
        app.drain.wake.notify_all();
        stopping.join().unwrap().unwrap();
        assert_eq!(delivery.copies.lock().unwrap().len(), 1);
    }
    #[test]
    fn cancel_during_final_drain_invalidates_delivery_before_waiting_for_operation() {
        let (c, app, delivery) = setup();
        start(&c);
        let (tx, rx) = mpsc::channel();
        *app.drain.held.lock().unwrap() = true;
        *app.drain.entered.lock().unwrap() = Some(tx);
        let pending = c.clone();
        let stopping = thread::spawn(move || {
            pending
                .prepare_target()
                .and_then(|request| pending.toggle(request))
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let cancelling = c.clone();
        let cancelled = thread::spawn(move || cancelling.cancel());
        let deadline = Instant::now() + Duration::from_secs(2);
        while c.generation.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(c.generation.load(Ordering::Acquire), 1);
        *app.drain.held.lock().unwrap() = false;
        app.drain.wake.notify_all();
        stopping.join().unwrap().unwrap();
        cancelled.join().unwrap().unwrap();
        assert!(delivery.copies.lock().unwrap().is_empty());
    }
    #[test]
    fn external_stop_or_interruption_retires_ownership_without_copy_or_stopping_other_meeting() {
        let (c, app, delivery) = setup();
        start(&c);
        c.observe_meeting(&json!({"id":"voice-note","status":"interrupted"}));
        assert_eq!(c.status()["state"], "idle");
        *app.active.lock().unwrap() = Some("meeting-other".into());
        assert!(c.toggle(c.prepare_target().unwrap()).is_err());
        assert_eq!(app.stops.load(Ordering::Relaxed), 0);
        assert!(delivery.copies.lock().unwrap().is_empty());
    }
}
