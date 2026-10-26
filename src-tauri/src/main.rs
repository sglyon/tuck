#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dictation_overlay;

use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tuck::runtime::DesktopRuntime;
struct RuntimeState {
    service: Result<Arc<DesktopRuntime>, String>,
    slots: Arc<tokio::sync::Semaphore>,
    quitting: AtomicBool,
    dictation_key_down: AtomicBool,
    shortcut_recorder: AtomicBool,
    dictation_menu: std::sync::Mutex<Vec<MenuItem<tauri::Wry>>>,
}
/// Failed composition has no live capture or loaded local meeting to flush. Keep
/// ordinary commands unavailable, but retain a working native quit path.
fn command_runtime<T: Clone>(
    runtime: &Result<T, String>,
    method: &str,
) -> Result<Option<T>, String> {
    match runtime {
        Ok(runtime) => Ok(Some(runtime.clone())),
        Err(_) if method == "quit_app" => Ok(None),
        Err(error) => Err(error.clone()),
    }
}
fn must_flush_before_exit(runtime_initialized: bool, quitting: bool) -> bool {
    runtime_initialized && !quitting
}
fn shortcut_pressed(latch: &AtomicBool, pressed: bool) -> bool {
    if pressed {
        !latch.swap(true, Ordering::AcqRel)
    } else {
        latch.store(false, Ordering::Release);
        false
    }
}
fn state_key_reset(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<RuntimeState>() {
        state.dictation_key_down.store(false, Ordering::Release);
    }
}
fn dictate(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<RuntimeState>() {
        if let Ok(runtime) = &state.service {
            runtime.dispatch_dictation();
        }
    }
}
fn cancel_dictation(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<RuntimeState>() {
        if let Ok(runtime) = &state.service {
            let controller = runtime.dictation.clone();
            let _ = std::thread::Builder::new()
                .name("tuck-cancel-dictation".into())
                .spawn(move || {
                    let _ = controller.cancel();
                });
        }
    }
}
fn shortcut_recorder_active(app: &tauri::AppHandle) -> bool {
    app.try_state::<RuntimeState>().is_some_and(|state| {
        state.shortcut_recorder.load(Ordering::Acquire)
            && app.get_webview_window("main").is_some_and(|window| window.is_focused().unwrap_or(false))
    })
}
fn intent(app: &tauri::AppHandle, action: &str) {
    // The File menu's accelerator can arrive separately from the global plugin.
    if action == "record" && shortcut_recorder_active(app) {
        let _ = app.emit("engine-event", json!({"event":"shortcut_recorded","payload":{"shortcut":"CommandOrControl+Shift+R"}}));
        return;
    }
    if let Some(state) = app.try_state::<RuntimeState>() {
        if let Ok(runtime) = &state.service {
            runtime.desktop_intent(action);
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        if action != "hide" {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
    let _ = app.emit(
        "engine-event",
        json!({"event":"desktop_intent","payload":{"intent":action}}),
    );
}
#[tauri::command]
async fn engine_call(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, RuntimeState>,
    method: String,
    params: Option<Value>,
) -> Result<Value, String> {
    if window.label() == "dictation-overlay" && method != "global_dictation_status" {
        return Err("The dictation indicator is read-only.".into());
    }
    let params = params.unwrap_or_else(|| json!({}));
    if method.len() > 80
        || !params.is_object()
        || serde_json::to_vec(&params).map_or(true, |v| v.len() > 1024 * 1024)
    {
        return Err("Invalid Tuck command.".into());
    }
    if method == "hide_window" {
        if let Some(window) = app.get_webview_window("main") {
            window
                .hide()
                .map_err(|_| "Tuck's window could not be hidden.")?;
        }
        return Ok(json!({"hidden":true}));
    }
    let Some(service) = command_runtime(&state.service, &method)? else {
        state.quitting.store(true, Ordering::Release);
        app.exit(0);
        return Ok(json!({"quitting":true}));
    };
    let _permit = state
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Tuck is finishing other work. Try again in a moment.")?;
    if method == "shortcut_recorder" {
        let active = params["active"].as_bool().ok_or("Invalid shortcut recorder state.")?;
        state.shortcut_recorder.store(active, Ordering::Release);
        state_key_reset(&app);
        return Ok(json!({"active":active}));
    }
    if method == "global_dictation_configure" {
        return tauri::async_runtime::spawn_blocking(move || {
            struct HostRegistry(tauri::AppHandle);
            impl tuck::dictation_config::Registry for HostRegistry {
                fn contains(&self, key: tauri_plugin_global_shortcut::Shortcut) -> bool { self.0.global_shortcut().is_registered(key) }
                fn register(&self, key: tauri_plugin_global_shortcut::Shortcut) -> tuck_core::ports::Result<()> {
                    self.0.global_shortcut().register(key).map_err(|_| tuck_core::AppError::unavailable("That shortcut is unavailable or used by another app. Your previous shortcut is unchanged."))
                }
                fn unregister(&self, key: tauri_plugin_global_shortcut::Shortcut) -> tuck_core::ports::Result<()> {
                    self.0.global_shortcut().unregister(key).map_err(|_| tuck_core::AppError::unavailable("The shortcut could not be released. Try again."))
                }
            }
            let config = tuck::dictation_config::Config::from_value(&params)?;
            let result = service.dictation.configure(config, |old, new| {
                tuck::dictation_config::apply(old,new,&HostRegistry(app.clone()),|| {
                    service.service.handle("update_settings",json!({"changes":{"globalDictation":new}})).map(|_| ())
                })
            });
            if result.is_ok() {
                state_key_reset(&app);
                service.sync_dictation_warmth();
            } else {
                let config = service.dictation.configuration();
                service.dictation.registered(config.enabled && config.key().is_ok_and(|key| app.global_shortcut().is_registered(key)));
            }
            result
        }).await.map_err(|_| "Dictation settings could not be saved.".to_string())?.map_err(|e| e.message);
    }
    let quitting = method == "quit_app";
    let result = tauri::async_runtime::spawn_blocking(move || {
        service.handle(&method, params).map_err(|e| e.message)
    })
    .await
    .map_err(|_| {
        "This operation could not finish. Your saved meeting is preserved.".to_string()
    })??;
    if quitting {
        state.quitting.store(true, Ordering::Release);
        app.exit(0);
    }
    Ok(result)
}
fn main() {
    let builder = tauri::Builder::default();
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_notification::init());
    let app = builder
        .register_asynchronous_uri_scheme_protocol("tuck-audio", |context, request, responder| {
            if context.webview_label() != "main" {
                responder.respond(
                    tauri::http::Response::builder()
                        .status(403)
                        .body(Vec::<u8>::new())
                        .unwrap(),
                );
                return;
            }
            let state = context.app_handle().state::<RuntimeState>();
            let service = state.service.clone();
            let permit = state.slots.clone().try_acquire_owned();
            match (service, permit) {
                (Ok(runtime), Ok(permit)) => {
                    tauri::async_runtime::spawn_blocking(move || {
                        let _permit = permit;
                        responder.respond(tuck::adapters::playback::response(
                            &runtime.archive,
                            &request,
                        ));
                    });
                }
                _ => responder.respond(
                    tauri::http::Response::builder()
                        .status(503)
                        .body(Vec::<u8>::new())
                        .unwrap(),
                ),
            }
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    use std::str::FromStr;
                    use tauri_plugin_global_shortcut::Shortcut;
                    if shortcut_recorder_active(app) {
                        if event.state() == ShortcutState::Pressed {
                            let _ = app.emit("engine-event", json!({"event":"shortcut_recorded","payload":{"shortcut":shortcut.to_string()}}));
                        }
                        state_key_reset(app);
                        return;
                    }
                    let dictation = app.try_state::<RuntimeState>().is_some_and(|state| {
                        state.service.as_ref().is_ok_and(|runtime| {
                            let config = runtime.dictation.configuration();
                            config.enabled
                                && config.key().is_ok_and(|value| value.id() == shortcut.id())
                        })
                    });
                    if dictation {
                        if let Some(state) = app.try_state::<RuntimeState>() {
                            if shortcut_pressed(
                                &state.dictation_key_down,
                                event.state() == ShortcutState::Pressed,
                            ) {
                                dictate(app);
                            }
                        }
                    } else if event.state() == ShortcutState::Pressed
                        && Shortcut::from_str("CommandOrControl+Shift+R")
                            .is_ok_and(|value| value.id() == shortcut.id())
                    {
                        intent(app, "record");
                    }
                })
                .build(),
        )
        .setup(|app| {
            app.manage(dictation_overlay::State::default());
            if let Some(overlay) = app.get_webview_window("dictation-overlay") {
                overlay.set_ignore_cursor_events(true)?;
            }
            let handle = app.handle().clone();
            let emit = Arc::new(move |event: &str, payload: Value| {
                if event == "global_dictation_updated" {
                    dictation_overlay::update(&handle, &payload);
                    let title = match payload["state"].as_str() {
                        Some("starting") => Some("… Preparing"),
                        Some("listening") => Some("● Dictating"),
                        Some("processing") => Some("… Finishing"),
                        Some("copied") => Some("✓ Copied"),
                        Some("paste_sent") => Some("✓ Dictated"),
                        Some("error") => Some("! Dictation"),
                        _ => None,
                    };
                    let tooltip = payload["message"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Tuck — private meeting notes")
                        .to_owned();
                    let enabled = payload["enabled"] != false;
                    let tray_app = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Some(state) = tray_app.try_state::<RuntimeState>() {
                            for item in state
                                .dictation_menu
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .iter()
                            {
                                let _ = item.set_enabled(enabled);
                            }
                        }
                        if let Some(tray) = tray_app.tray_by_id("tuck") {
                            let _ = tray.set_title(title);
                            let _ = tray.set_tooltip(Some(tooltip));
                        }
                    });
                }
                let _ = handle.emit("engine-event", json!({"event":event,"payload":payload}));
            });
            let service = (|| {
                let data = app
                    .path()
                    .app_data_dir()
                    .map_err(|_| "Cannot locate the local Tuck library.".to_string())?;
                let resources = app
                    .path()
                    .resource_dir()
                    .map_err(|_| "Cannot locate Tuck resources.".to_string())?;
                tuck::runtime::compose(&data, &resources, emit).map_err(|e| e.message)
            })();
            app.manage(RuntimeState {
                service,
                slots: Arc::new(tokio::sync::Semaphore::new(4)),
                quitting: AtomicBool::new(false),
                dictation_key_down: AtomicBool::new(false),
                shortcut_recorder: AtomicBool::new(false),
                dictation_menu: std::sync::Mutex::new(Vec::new()),
            });
            let show = MenuItem::with_id(app, "show", "Open Tuck", true, None::<&str>)?;
            let record = MenuItem::with_id(app, "record", "Record a meeting…", true, None::<&str>)?;
            let file_record = MenuItem::with_id(
                app,
                "file-record",
                "Record a meeting…",
                true,
                Some("CmdOrCtrl+Shift+R"),
            )?;
            let file_dictate =
                MenuItem::with_id(app, "file-dictate", "Dictate anywhere", true, None::<&str>)?;
            let app_menu = Menu::default(app.handle())?;
            let mut file_menu = None;
            for item in app_menu.items()? {
                if let Some(menu) = item.as_submenu() {
                    if menu.text()? == "File" {
                        file_menu = Some(menu.clone());
                        break;
                    }
                }
            }
            if let Some(file_menu) = file_menu {
                file_menu.prepend(&file_record)?;
                file_menu.prepend(&file_dictate)?;
            } else {
                app_menu.prepend(&Submenu::with_items(
                    app,
                    "File",
                    true,
                    &[&file_dictate, &file_record],
                )?)?;
            }
            app.set_menu(app_menu)?;
            app.on_menu_event(|app, event| match event.id.as_ref() {
                "file-record" => intent(app, "record"),
                "file-dictate" => dictate(app),
                _ => {}
            });
            let quit = MenuItem::with_id(app, "quit", "Quit Tuck", true, None::<&str>)?;
            let dictation = MenuItem::with_id(
                app,
                "dictate",
                "Start / finish dictation",
                true,
                None::<&str>,
            )?;
            *app.state::<RuntimeState>()
                .dictation_menu
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = vec![file_dictate.clone(), dictation.clone()];
            let cancel = MenuItem::with_id(
                app,
                "dictation-cancel",
                "Save dictation without inserting",
                true,
                None::<&str>,
            )?;
            let menu = Menu::with_items(app, &[&show, &record, &dictation, &cancel, &quit])?;
            let mut tray = TrayIconBuilder::with_id("tuck")
                .tooltip("Tuck — private meeting notes")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => intent(app, "show"),
                    "record" => intent(app, "record"),
                    "dictate" => dictate(app),
                    "dictation-cancel" => cancel_dictation(app),
                    "quit" => intent(app, "quit"),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            let registered = match app.global_shortcut().register("CommandOrControl+Shift+R") {
                Ok(()) => app
                    .global_shortcut()
                    .is_registered("CommandOrControl+Shift+R"),
                Err(error) => {
                    eprintln!("Tuck recording shortcut unavailable: {error}");
                    false
                }
            };
            if let Ok(runtime) = &app.state::<RuntimeState>().service {
                let config = runtime.dictation.configuration();
                let available = config.enabled
                    && config
                        .key()
                        .is_ok_and(|key| app.global_shortcut().register(key).is_ok());
                runtime.recording_shortcut_registered(registered);
                runtime.dictation.registered(available);
                #[cfg(target_os = "macos")]
                let notifications = Arc::new(tuck::adapters::notifications::MacNotifications);
                #[cfg(not(target_os = "macos"))]
                let notifications = Arc::new(tuck::adapters::notifications::DesktopNotifications(
                    app.handle().clone(),
                ));
                if let Err(error) = runtime.start_reminders(notifications) {
                    eprintln!("Tuck meeting reminders unavailable: {}", error.message);
                }
                runtime.sync_dictation_warmth();
                if let Err(error) = runtime.start_coaching() {
                    eprintln!("Tuck live coach unavailable: {}", error.message);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" { return; }
            if matches!(event, tauri::WindowEvent::Focused(false) | tauri::WindowEvent::CloseRequested { .. }) {
                if let Some(state) = window.app_handle().try_state::<RuntimeState>() {
                    state.shortcut_recorder.store(false, Ordering::Release);
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                intent(window.app_handle(), "hide");
            }
        })
        .invoke_handler(tauri::generate_handler![engine_call])
        .build(tauri::generate_context!())
        .expect("Tuck could not initialize its native window");
    app.run(|handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. }
            if must_flush_before_exit(
                handle.state::<RuntimeState>().service.is_ok(),
                handle
                    .state::<RuntimeState>()
                    .quitting
                    .load(Ordering::Acquire),
            ) =>
        {
            api.prevent_exit();
            intent(handle, "quit");
        }
        tauri::RunEvent::Exit => {
            if let Ok(service) = &handle.state::<RuntimeState>().service {
                let _ = service.handle("shutdown", json!({}));
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => intent(handle, "show"),
        _ => {}
    });
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn held_dictation_shortcut_fires_once_until_released() {
        let latch = AtomicBool::new(false);
        assert!(shortcut_pressed(&latch, true));
        assert!(!shortcut_pressed(&latch, true));
        assert!(!shortcut_pressed(&latch, false));
        assert!(shortcut_pressed(&latch, true));
    }
    #[test]
    fn failed_runtime_allows_quit_but_preserves_other_command_errors() {
        let failed: Result<(), String> = Err("Cannot open local library".into());
        assert_eq!(command_runtime(&failed, "quit_app").unwrap(), None);
        assert_eq!(
            command_runtime(&failed, "bootstrap").unwrap_err(),
            "Cannot open local library"
        );
        assert!(!must_flush_before_exit(failed.is_ok(), false));
        assert!(!must_flush_before_exit(failed.is_ok(), true));
    }
    #[test]
    fn initialized_runtime_requires_flush_and_shutdown_before_exit() {
        let initialized = Ok::<_, String>(42);
        assert_eq!(command_runtime(&initialized, "quit_app").unwrap(), Some(42));
        assert!(must_flush_before_exit(initialized.is_ok(), false));
        assert!(!must_flush_before_exit(initialized.is_ok(), true));
    }
}
