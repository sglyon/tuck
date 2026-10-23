//! Desktop composition root. No speech download, recording, or outbound request on launch.
use crate::adapters::{
    archive::LocalArchive, capture::CaptureEngine, chatgpt::ChatGptAuth,
    intelligence::Intelligence, notion::NotionPublisher, secrets::NativeSecrets,
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
}
impl DesktopRuntime {
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
                self.capture.configure(&self.service.get_settings()?)?;
                let mut ready = self.capture.readiness()?;
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
            "calendar_status" => calendar::status(),
            "connect_calendar" => calendar::connect(),
            "calendar_events" => calendar::events(text("from")?, text("to")?),
            "import_calendar" => calendar::import(Path::new(text("path")?)),
            // Window lifecycle is supplied by the desktop host; diagnostics still flush safely.
            "hide_window" => Ok(json!({"hidden":true})),
            "quit_app" => self.service.handle("shutdown", json!({})),
            _ => self.service.handle(method, params),
        }
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
    let adapters = Adapters {
        capture: capture.clone(),
        intelligence: Arc::new(Intelligence::new(auth.clone(), secrets.clone())?),
        publisher: Arc::new(NotionPublisher::new(secrets.clone())?),
        accounts: auth,
        secrets,
        archive: archive.clone(),
    };
    let service = MeetingService::new(
        Arc::new(SqliteStore::open(data.join("tuck.sqlite3"))?),
        adapters,
        emit,
    )?;
    *application
        .lock()
        .map_err(|_| AppError::unavailable("Tuck's recording recovery could not initialize."))? =
        Arc::downgrade(&service);
    Ok(Arc::new(DesktopRuntime {
        service,
        capture,
        archive,
        shortcut: Mutex::new(
            serde_json::json!({"recordingShortcut":"CommandOrControl+Shift+R","recordingShortcutAvailable":false,"message":null,"lastIntent":null}),
        ),
    }))
}
