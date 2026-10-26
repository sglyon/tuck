//! Read-only feedback that never activates Tuck or changes the dictation target.
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;

#[derive(Default)]
pub struct State {
    revision: AtomicU64,
    last_state: Mutex<String>,
}

impl State {
    fn advance(&self, state: &str) -> Option<u64> {
        let mut previous = self.last_state.lock().unwrap_or_else(|p| p.into_inner());
        if *previous == state {
            return None;
        }
        *previous = state.to_owned();
        Some(self.revision.fetch_add(1, Ordering::AcqRel) + 1)
    }
}

// Active states stay visible; terminal feedback is brief, and idle stays hidden.
fn lifetime(state: &str) -> Option<Option<Duration>> {
    match state {
        "starting" | "listening" | "processing" | "cleaning" => Some(None),
        "copied" | "paste_sent" => Some(Some(Duration::from_secs(3))),
        "error" => Some(Some(Duration::from_secs(7))),
        _ => None,
    }
}

pub fn update(app: &tauri::AppHandle, payload: &Value) {
    let state = payload["state"].as_str().unwrap_or("idle");
    let Some(revision) = app.state::<State>().advance(state) else {
        return;
    };
    let lifetime = lifetime(state);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.state::<State>().revision.load(Ordering::Acquire) != revision {
            return;
        }
        let Some(window) = handle.get_webview_window("dictation-overlay") else {
            return;
        };
        let Some(timeout) = lifetime else {
            let _ = window.hide();
            return;
        };
        // Place on the pointer's screen, above the Dock/work-area edge. Keep the
        // overlay stationary throughout a session, including its result state.
        if !window.is_visible().unwrap_or(false) {
            let monitor = handle
                .cursor_position()
                .ok()
                .and_then(|point| handle.monitor_from_point(point.x, point.y).ok().flatten())
                .or_else(|| handle.primary_monitor().ok().flatten());
            if let Some(monitor) = monitor {
                let area = monitor.work_area();
                let scale = monitor.scale_factor();
                let width = (320.0 * scale).round() as i32;
                let height = (76.0 * scale).round() as i32;
                let gap = (24.0 * scale).round() as i32;
                let x = area.position.x + ((area.size.width as i32 - width) / 2).max(0);
                let y = area.position.y + (area.size.height as i32 - height - gap).max(0);
                let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
        show_without_focus(&window);
        if let Some(timeout) = timeout {
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(timeout).await;
                let timer_app = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    // An old completion timer must not hide a new recording.
                    if timer_app.state::<State>().revision.load(Ordering::Acquire) == revision {
                        if let Some(window) = timer_app.get_webview_window("dictation-overlay") {
                            let _ = window.hide();
                        }
                    }
                });
            });
        }
    });
}

fn show_without_focus(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    if let Ok(pointer) = window.ns_window() {
        // Called only on the main thread. Tauri's ordinary show() uses AppKit's
        // makeKeyAndOrderFront; orderFrontRegardless preserves the user's key
        // window, which is essential for safe automatic paste.
        unsafe {
            let native = pointer.cast::<objc2::runtime::AnyObject>();
            let behavior: usize = objc2::msg_send![native, collectionBehavior];
            // NSWindowCollectionBehaviorFullScreenAuxiliary (1 << 8), alongside
            // Tauri's CanJoinAllSpaces, also permits feedback over full-screen apps.
            let _: () = objc2::msg_send![native, setCollectionBehavior: behavior | (1usize << 8)];
            let _: () = objc2::msg_send![native, orderFrontRegardless];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.show(); // The overlay is configured focusable(false).
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_status_does_not_extend_feedback_and_new_capture_invalidates_old_timer() {
        let state = State::default();
        state.advance("listening").unwrap();
        let completed = state.advance("copied").unwrap();
        assert!(state.advance("copied").is_none());
        assert_eq!(state.revision.load(Ordering::Acquire), completed);
        let next_capture = state.advance("starting").unwrap();
        assert_ne!(completed, next_capture);
        assert_eq!(state.revision.load(Ordering::Acquire), next_capture);
    }
    #[test]
    fn stays_for_capture_then_hides_after_results_and_errors() {
        for state in ["starting", "listening", "processing", "cleaning"] {
            assert_eq!(lifetime(state), Some(None));
        }
        for state in ["copied", "paste_sent"] {
            assert_eq!(lifetime(state), Some(Some(Duration::from_secs(3))));
        }
        assert_eq!(lifetime("error"), Some(Some(Duration::from_secs(7))));
        assert_eq!(lifetime("idle"), None);
        assert_eq!(lifetime("unknown"), None);
    }
}
