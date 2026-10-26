//! Desktop composition root. No speech download, recording, or outbound request on launch.
use crate::adapters::{
    archive::LocalArchive, capture::CaptureEngine, chatgpt::ChatGptAuth,
    intelligence::Intelligence, notion::NotionPublisher, notion_auth::NotionAuth,
    secrets::NativeSecrets,
};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, Weak},
};
use tuck_core::{
    ports::{Emit, RecordingPort, Result, SecretPort},
    store::sqlite::SqliteStore,
    Adapters, AppError, MeetingService,
};

pub struct DesktopRuntime {
    pub service: Arc<MeetingService>,
    capture: Arc<CaptureEngine>,
    pub archive: Arc<LocalArchive>,
    shortcut: Mutex<serde_json::Value>,
    pub dictation: Arc<crate::global_dictation::GlobalDictation>,
    text_delivery: Arc<crate::adapters::dictation_target::NativeTextDelivery>,
    reminders: crate::reminders::ReminderWorker,
    coaching: crate::coaching::CoachWorker,
    notion_auth: Arc<NotionAuth>,
}
impl DesktopRuntime {
    /// Called on the native shortcut callback before spawning capture work.
    pub fn dispatch_dictation(self: &Arc<Self>) {
        let target = match self.dictation.prepare_target() {
            Ok(target) => target,
            Err(_) => return,
        };
        let controller = self.dictation.clone();
        if std::thread::Builder::new()
            .name("tuck-global-dictation".into())
            .spawn(move || {
                let _ = controller.toggle(target);
            })
            .is_err()
        {
            self.dictation.abandon_prepared();
        }
    }

    /// Explicit host activation; diagnostic composition remains inert.
    pub fn start_coaching(&self) -> Result<()> {
        self.coaching.start(self.service.clone())
    }
    fn reset_notion_destination(&self) -> Result<()> {
        self.service.reset_notion_destination()
    }
    /// Only the Tauri host calls this; the diagnostic host remains inert.
    pub fn start_reminders(
        &self,
        notifications: Arc<dyn crate::adapters::notifications::Notifications>,
    ) -> Result<()> {
        self.reminders.start(self.service.clone(), notifications)
    }
    pub fn recording_shortcut_registered(&self, available: bool) {
        let mut status = self.shortcut.lock().unwrap_or_else(|p| p.into_inner());
        status["recordingShortcutAvailable"] = serde_json::json!(available);
        status["message"] = if available {
            serde_json::Value::Null
        } else {
            serde_json::json!("The recording shortcut could not be registered. Another app may be using it. Choose File → Record a meeting or use the Tuck tray menu.")
        };
    }
    pub fn desktop_intent(&self, intent: &str) {
        self.shortcut.lock().unwrap_or_else(|p| p.into_inner())["lastIntent"] =
            serde_json::json!({"intent":intent,"at":chrono::Utc::now().to_rfc3339()});
    }
    pub fn handle(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        use crate::adapters::{calendar, capture::capture_devices};
        use serde_json::json;
        let text = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| AppError::new("invalid_arguments", format!("Missing {key}.")))
        };
        match method {
            "update_settings" if params["changes"].get("globalDictation").is_some() => Err(
                AppError::invalid("Use the Dictation anywhere controls to change its shortcut."),
            ),
            "global_dictation_status" => Ok(self.dictation.status()),
            "global_dictation_toggle" => {
                let target = self.dictation.prepare_target()?;
                self.dictation.toggle(target)
            }
            "global_dictation_cancel" => self.dictation.cancel(),
            "dictation_accessibility_settings" => {
                self.text_delivery.open_accessibility_settings()?;
                Ok(self.dictation.status())
            }

            "notion_account_status" => self.notion_auth.status(),
            "notion_connect" => {
                let before = self.notion_auth.status()?;
                let connected = self.notion_auth.connect()?;
                if before["workspaceId"].as_str().filter(|id| !id.is_empty())
                    != connected["workspaceId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                    || before["mode"] != connected["mode"]
                {
                    self.reset_notion_destination()?;
                }
                Ok(connected)
            }
            "notion_cancel_connect" => self.notion_auth.cancel(),
            "notion_disconnect" => {
                let disconnected = self.notion_auth.disconnect()?;
                self.reset_notion_destination()?;
                Ok(disconnected)
            }
            "save_secret" if params["kind"] == "notion" => {
                let token = tuck_core::domain::text(&params["value"], "Credential", 8192, true)?;
                if token.trim().is_empty() {
                    self.notion_auth.disconnect()?;
                } else {
                    self.notion_auth.set_manual_token(token)?;
                }
                self.reset_notion_destination()?;
                self.service.get_settings()
            }
            "update_settings" if params["changes"].get("meetingReminders").is_some() => {
                Err(AppError::invalid(
                    "Use the meeting reminder control to change notification access.",
                ))
            }
            "meeting_reminder_status" => self
                .reminders
                .status(self.service.meeting_reminders_enabled()?),
            "set_meeting_reminders" => {
                let enabled = params["enabled"].as_bool().ok_or_else(|| {
                    AppError::invalid("Choose whether to enable meeting reminders.")
                })?;
                if enabled {
                    self.reminders.request_permission()?;
                }
                self.service.set_meeting_reminders_enabled(enabled)?;
                self.reminders.status(enabled)
            }
            "shutdown" | "quit_app" => {
                let _ = self.notion_auth.cancel_pending();
                let dictation = self.dictation.shutdown();
                // Stop/drain hardware before waiting on an already-sent AI request.
                // Core shutdown first invalidates monitoring so its late result is discarded.
                let stopped = self.service.handle("shutdown", json!({}));
                let coaching = self.coaching.shutdown();
                self.reminders.shutdown();
                stopped.and_then(|value| dictation.and(coaching).map(|()| value))
            }
            "audio_source" => {
                self.service.handle(method, params.clone())?;
                Ok(
                    json!({"url":crate::adapters::playback::source_url(text("meetingId")?,text("audioId")?)}),
                )
            }
            "desktop_status" => Ok(self
                .shortcut
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()),
            "capture_devices" => capture_devices(),
            "capture_readiness" => {
                let mut settings = self.service.get_settings()?;
                if params["kind"] == "dictation" {
                    settings["diarization"] = json!(false);
                }
                self.capture.configure(&settings)?;
                let mut ready = if params["kind"] == "dictation" {
                    self.capture.readiness_for_dictation()?
                } else {
                    self.capture.readiness()?
                };
                let shortcut = self
                    .shortcut
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                ready["recordingShortcutAvailable"] =
                    shortcut["recordingShortcutAvailable"].clone();
                ready["recordingShortcut"] = shortcut["recordingShortcut"].clone();
                if let Some(message) = shortcut["message"].as_str() {
                    if let Some(issues) = ready["issues"].as_array_mut() {
                        issues.push(json!({"code":"shortcut_unavailable","message":message}));
                    }
                }
                Ok(ready)
            }
            "test_audio" => {
                self.capture.configure(&self.service.get_settings()?)?;
                self.capture.test_audio(
                    params["microphone"].as_bool().unwrap_or(true),
                    params["systemAudio"].as_bool().unwrap_or(true),
                )
            }
            "calendar_status" | "connect_calendar" => {
                let status = if method == "connect_calendar" {
                    calendar::connect()?
                } else {
                    calendar::status()?
                };
                if status["connected"] != true {
                    self.service.invalidate_calendar_snapshot("device")?;
                }
                Ok(status)
            }
            "calendar_events" => {
                let events = calendar::events(text("from")?, text("to")?).inspect_err(|_| {
                    let _ = self.service.invalidate_calendar_snapshot("device");
                })?;
                let snapshot = events
                    .as_array()
                    .ok_or_else(|| AppError::invalid("Calendar returned an invalid snapshot."))?;
                self.service
                    .replace_calendar_snapshot("device", snapshot, &chrono::Utc::now().to_rfc3339())
                    .inspect_err(|_| {
                        let _ = self.service.invalidate_calendar_snapshot("device");
                    })?;
                Ok(events)
            }
            "import_calendar" => {
                let events = calendar::import(Path::new(text("path")?))?;
                let snapshot = events
                    .as_array()
                    .ok_or_else(|| AppError::invalid("Calendar returned an invalid snapshot."))?;
                self.service.replace_calendar_snapshot(
                    "import",
                    snapshot,
                    &chrono::Utc::now().to_rfc3339(),
                )?;
                Ok(events)
            }
            // Window lifecycle is supplied by the desktop host; diagnostics still flush safely.
            "hide_window" => Ok(json!({"hidden":true})),
            _ => self.service.handle(method, params),
        }
    }
}

impl Drop for DesktopRuntime {
    fn drop(&mut self) {
        let _ = self.notion_auth.cancel_pending();
        let _ = self.dictation.shutdown();
        // Close capture promptly, then retain the application while joining AI work.
        let _ = self.service.handle("shutdown", serde_json::json!({}));
        let _ = self.coaching.shutdown();
        self.reminders.shutdown();
    }
}

pub fn compose(data: &Path, resources: &Path, emit: Emit) -> Result<Arc<DesktopRuntime>> {
    fs::create_dir_all(data)
        .map_err(|_| AppError::unavailable("Tuck could not create its local library."))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(data, fs::Permissions::from_mode(0o700))
            .map_err(|_| AppError::unavailable("Tuck could not protect its local library."))?;
    }
    let secrets: Arc<dyn SecretPort> = Arc::new(NativeSecrets);
    let auth = Arc::new(ChatGptAuth::new(data, secrets.clone())?);
    let application = Arc::new(Mutex::new(Weak::<MeetingService>::new()));
    let recovery_application = application.clone();
    let capture_events = emit.clone();
    // Capture reports faults from its own workers. Recovery must run elsewhere:
    // stopping capture waits for those workers and their final durable callbacks.
    let capture_emit: Emit = Arc::new(move |event, payload| {
        if event == "engine_error" && payload["code"] == "capture_interrupted" {
            if let Some(id) = payload["meetingId"].as_str() {
                let service = recovery_application.lock().ok().and_then(|v| v.upgrade());
                if let Some(service) = service {
                    let id = id.to_owned();
                    let recovery_events = capture_events.clone();
                    std::thread::spawn(move || {
                        if let Err(error) = service.interrupt_recording(&id) {
                            recovery_events(
                                "engine_error",
                                serde_json::json!({"code":"recovery_failed","meetingId":id,"message":error.message}),
                            );
                        }
                    });
                }
            }
        }
        capture_events(event, payload);
    });
    let capture = Arc::new(CaptureEngine::new(
        data.to_path_buf(),
        resources.to_path_buf(),
        capture_emit,
    ));
    let archive = Arc::new(LocalArchive::new(data));
    let notion_auth = Arc::new(NotionAuth::new(secrets.clone())?);
    let adapters = Adapters {
        capture: capture.clone(),
        intelligence: Arc::new(Intelligence::new(auth.clone(), secrets.clone())?),
        publisher: Arc::new(NotionPublisher::new(notion_auth.clone())?),
        accounts: auth,
        secrets,
        archive: archive.clone(),
    };
    let dictation_observer = Arc::new(Mutex::new(
        Weak::<crate::global_dictation::GlobalDictation>::new(),
    ));
    let observer = dictation_observer.clone();
    let core_emit = emit.clone();
    let service = MeetingService::new(
        Arc::new(SqliteStore::open(data.join("tuck.sqlite3"))?),
        adapters,
        Arc::new(move |event, payload| {
            if event == "meeting_updated" {
                let dictation = observer.lock().ok().and_then(|weak| weak.upgrade());
                if let Some(dictation) = dictation {
                    dictation.observe_meeting(&payload["meeting"]);
                }
            }
            core_emit(event, payload);
        }),
    )?;
    let text_delivery = Arc::new(crate::adapters::dictation_target::NativeTextDelivery::default());
    let dictation = Arc::new(crate::global_dictation::GlobalDictation::new(
        Arc::new(crate::global_dictation::NativeApplication {
            service: service.clone(),
            capture: capture.clone(),
        }),
        text_delivery.clone(),
        emit,
    ));
    dictation.configure(
        crate::dictation_config::Config::from_value(&service.get_settings()?["globalDictation"])?,
        |_, _| Ok(false),
    )?;
    *dictation_observer.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&dictation);
    *application
        .lock()
        .map_err(|_| AppError::unavailable("Tuck's recording recovery could not initialize."))? =
        Arc::downgrade(&service);
    Ok(Arc::new(DesktopRuntime {
        service,
        capture,
        archive,
        dictation,
        text_delivery,
        shortcut: Mutex::new(
            serde_json::json!({"recordingShortcut":"CommandOrControl+Shift+R","recordingShortcutAvailable":false,"message":null,"lastIntent":null}),
        ),
        reminders: crate::reminders::ReminderWorker::default(),
        coaching: crate::coaching::CoachWorker::default(),
        notion_auth,
    }))
}
