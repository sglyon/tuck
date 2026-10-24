//! Write-only clipboard delivery, with conservative same-field paste on macOS.
//! Target identities stay in process; no field text or previous clipboard content is read.
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tuck_core::{ports::Result, AppError};

#[cfg(target_os = "macos")]
#[path = "dictation_target/macos.rs"]
mod platform;
#[cfg(not(target_os = "macos"))]
#[path = "dictation_target/unsupported.rs"]
mod platform;

#[derive(Clone)]
pub struct DictationTarget {
    id: uuid::Uuid,
    candidate: bool,
}
impl DictationTarget {
    pub fn none() -> Self {
        Self {
            id: uuid::Uuid::nil(),
            candidate: false,
        }
    }
    pub fn paste_candidate(&self) -> bool {
        self.candidate
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryOutcome {
    pub copied: bool,
    pub paste_attempted: bool,
    /// A stable outcome code; paste_sent means a shortcut was sent, not verified insertion.
    pub reason: String,
}
pub trait TextDelivery: Send + Sync {
    fn capture_target(&self) -> Result<DictationTarget>;
    fn deliver(
        &self,
        target: &DictationTarget,
        text: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<DeliveryOutcome>;
    fn discard_target(&self, target: &DictationTarget) -> Result<()>;
    fn status(&self) -> Value;
}

pub struct NativeTextDelivery {
    inner: Delivery<platform::Platform, NativeClipboard>,
}
impl Default for NativeTextDelivery {
    fn default() -> Self {
        Self {
            inner: Delivery::new(platform::Platform, NativeClipboard::default()),
        }
    }
}
impl TextDelivery for NativeTextDelivery {
    fn capture_target(&self) -> Result<DictationTarget> {
        self.inner.capture_target()
    }
    fn deliver(
        &self,
        target: &DictationTarget,
        text: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<DeliveryOutcome> {
        self.inner.deliver(target, text, allowed)
    }
    fn discard_target(&self, target: &DictationTarget) -> Result<()> {
        self.inner.discard_target(target)
    }
    fn status(&self) -> Value {
        self.inner.backend.status()
    }
}
impl NativeTextDelivery {
    pub fn capture_target(&self) -> Result<DictationTarget> {
        self.inner.capture_target()
    }
    pub fn deliver(
        &self,
        target: &DictationTarget,
        text: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<DeliveryOutcome> {
        self.inner.deliver(target, text, allowed)
    }
    pub fn discard_target(&self, target: &DictationTarget) -> Result<()> {
        self.inner.discard_target(target)
    }
    pub fn status(&self) -> Value {
        self.inner.backend.status()
    }
    /// Call only after an explicit user action. Status/capture never request permission.
    pub fn open_accessibility_settings(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            webbrowser::open(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            )
            .map(|_| ())
            .map_err(|_| {
                AppError::unavailable(
                    "Open System Settings → Privacy & Security → Accessibility to allow Tuck.",
                )
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(AppError::unavailable("Automatic paste is not available on this platform yet. Dictation can still copy text."))
        }
    }
}

struct Capture<T> {
    target: Option<T>,
    reason: &'static str,
}
trait Backend: Send + Sync {
    type Target: Send;
    fn capture(&self) -> Capture<Self::Target>;
    fn validate(&self, target: &Self::Target) -> std::result::Result<(), &'static str>;
    /// Must revalidate focus immediately before sending any paste shortcut.
    fn paste_if_current(
        &self,
        target: &Self::Target,
        allowed: &dyn Fn() -> bool,
    ) -> std::result::Result<(), &'static str>;
    fn status(&self) -> Value;
}
trait Clipboard: Send {
    fn copy(&mut self, text: &str) -> Result<()>;
}
#[derive(Default)]
struct NativeClipboard(Option<arboard::Clipboard>);
impl Clipboard for NativeClipboard {
    fn copy(&mut self, text: &str) -> Result<()> {
        if self.0.is_none() {
            self.0 = Some(arboard::Clipboard::new().map_err(|_| clipboard_error())?);
        }
        self.0
            .as_mut()
            .expect("clipboard initialized")
            .set_text(text)
            .map_err(|_| clipboard_error())
    }
}
fn clipboard_error() -> AppError {
    AppError::new("clipboard_unavailable","Your dictation is saved in Tuck, but it could not be copied. Open the voice note and copy its text.")
}
struct Pending<T> {
    id: uuid::Uuid,
    target: Option<T>,
    reason: &'static str,
    created: Instant,
}
struct DeliveryState<T, C> {
    pending: Option<Pending<T>>,
    clipboard: C,
}
struct Delivery<B: Backend, C: Clipboard> {
    backend: B,
    state: Mutex<DeliveryState<B::Target, C>>,
}
impl<B: Backend, C: Clipboard> Delivery<B, C> {
    fn new(backend: B, clipboard: C) -> Self {
        Self {
            backend,
            state: Mutex::new(DeliveryState {
                pending: None,
                clipboard,
            }),
        }
    }
    fn capture_target(&self) -> Result<DictationTarget> {
        let mut state = self.state.lock().map_err(|_| {
            AppError::unavailable(
                "Dictation delivery is unavailable. Your voice note stays in Tuck.",
            )
        })?;
        let capture = self.backend.capture();
        let id = uuid::Uuid::new_v4();
        let candidate = capture.target.is_some();
        state.pending = Some(Pending {
            id,
            target: capture.target,
            reason: capture.reason,
            created: Instant::now(),
        });
        Ok(DictationTarget { id, candidate })
    }
    fn discard_target(&self, target: &DictationTarget) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| AppError::unavailable("Dictation delivery is unavailable."))?;
        if state.pending.as_ref().is_some_and(|p| p.id == target.id) {
            state.pending = None;
        }
        Ok(())
    }
    fn deliver(
        &self,
        target: &DictationTarget,
        text: &str,
        allowed: &dyn Fn() -> bool,
    ) -> Result<DeliveryOutcome> {
        if text.trim().is_empty()
            || text.chars().count() > 200_000
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(AppError::invalid(
                "There is no usable dictation text to copy.",
            ));
        }
        if !allowed() {
            return Ok(DeliveryOutcome {
                copied: false,
                paste_attempted: false,
                reason: "cancelled".into(),
            });
        }
        let mut state = self.state.lock().map_err(|_| clipboard_error())?;
        let pending = if state.pending.as_ref().is_some_and(|p| p.id == target.id) {
            state.pending.take()
        } else {
            None
        };
        let eligibility = match &pending {
            Some(p) if p.created.elapsed() > Duration::from_secs(600) => Err("target_expired"),
            Some(p) => match &p.target {
                Some(t) => self.backend.validate(t),
                None => Err(p.reason),
            },
            None => Err("target_unavailable"),
        };
        // Never read, retain or restore an earlier clipboard value. One retained writer
        // keeps Linux clipboard ownership alive and serializes native clipboard access.
        if !allowed() {
            return Ok(DeliveryOutcome {
                copied: false,
                paste_attempted: false,
                reason: "cancelled".into(),
            });
        }
        state.clipboard.copy(text)?;
        let reason = match (
            eligibility,
            pending.as_ref().and_then(|p| p.target.as_ref()),
        ) {
            (Ok(()), Some(_)) if !allowed() => "cancelled",
            (Ok(()), Some(t)) => self
                .backend
                .paste_if_current(t, allowed)
                .err()
                .unwrap_or("paste_sent"),
            (Err(reason), _) => reason,
            _ => "target_unavailable",
        };
        Ok(DeliveryOutcome {
            copied: true,
            paste_attempted: reason == "paste_sent",
            reason: reason.into(),
        })
    }
}

/// Metadata-only field policy, shared with fake tests. Unknown/secure/terminal fields fail closed.
#[cfg(any(target_os = "macos", test))]
fn eligible_field(
    role: &str,
    subrole: Option<&str>,
    enabled: bool,
    focused: bool,
    settable: bool,
    bundle: &str,
) -> bool {
    let app = bundle.to_ascii_lowercase();
    let denied = [
        "terminal",
        "iterm",
        "ghostty",
        "alacritty",
        "kitty",
        "wezterm",
        "warp",
        "hyper",
        "tabby",
        "rio.terminal",
        "password",
        "keychain",
        "bitwarden",
        "1password",
        "lastpass",
    ];
    matches!(role, "AXTextField" | "AXTextArea")
        && enabled
        && focused
        && settable
        && !bundle.is_empty()
        && subrole.is_some_and(|role| role != "AXSecureTextField")
        && !denied.iter().any(|needle| app.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct Observed {
        focus: u32,
        candidate: bool,
        reason: &'static str,
        events: Vec<&'static str>,
        texts: Vec<String>,
        pastes: usize,
        clipboard_fails: bool,
        after_copy: Option<Box<dyn Fn() + Send>>,
    }
    struct FakeBackend(Arc<Mutex<Observed>>);
    struct FakeClipboard(Arc<Mutex<Observed>>);
    impl Backend for FakeBackend {
        type Target = u32;
        fn capture(&self) -> Capture<u32> {
            let mut s = self.0.lock().unwrap();
            s.events.push("capture");
            Capture {
                target: s.candidate.then_some(s.focus),
                reason: s.reason,
            }
        }
        fn validate(&self, target: &u32) -> std::result::Result<(), &'static str> {
            let mut s = self.0.lock().unwrap();
            s.events.push("validate");
            if !s.candidate {
                Err("protected_field")
            } else if s.focus != *target {
                Err("focus_changed")
            } else {
                Ok(())
            }
        }
        fn paste_if_current(
            &self,
            target: &u32,
            allowed: &dyn Fn() -> bool,
        ) -> std::result::Result<(), &'static str> {
            self.validate(target)?;
            if !allowed() {
                return Err("cancelled");
            }
            let mut s = self.0.lock().unwrap();
            s.events.push("paste");
            s.pastes += 1;
            Ok(())
        }
        fn status(&self) -> Value {
            json!({"pasteAvailable":true,"pastePermission":"granted"})
        }
    }
    impl Clipboard for FakeClipboard {
        fn copy(&mut self, text: &str) -> Result<()> {
            let hook = {
                let mut s = self.0.lock().unwrap();
                s.events.push("copy");
                if s.clipboard_fails {
                    return Err(clipboard_error());
                }
                s.texts.push(text.into());
                s.after_copy.take()
            };
            if let Some(hook) = hook {
                hook();
            }
            Ok(())
        }
    }
    fn fixture() -> (Delivery<FakeBackend, FakeClipboard>, Arc<Mutex<Observed>>) {
        let state = Arc::new(Mutex::new(Observed {
            focus: 1,
            candidate: true,
            reason: "ready",
            ..Default::default()
        }));
        (
            Delivery::new(FakeBackend(state.clone()), FakeClipboard(state.clone())),
            state,
        )
    }
    #[test]
    fn original_field_is_copied_then_revalidated_before_a_single_paste_attempt() {
        let (delivery, state) = fixture();
        let target = delivery.capture_target().unwrap();
        assert!(target.paste_candidate());
        let result = delivery
            .deliver(&target, "A plain sentence.", &|| true)
            .unwrap();
        assert!(result.copied && result.paste_attempted);
        assert_eq!(result.reason, "paste_sent");
        let s = state.lock().unwrap();
        assert_eq!(
            s.events,
            vec!["capture", "validate", "copy", "validate", "paste"]
        );
        assert_eq!(s.texts, vec!["A plain sentence."]);
        assert_eq!(s.pastes, 1);
        drop(s);
        let repeated = delivery
            .deliver(&target, "A plain sentence.", &|| true)
            .unwrap();
        assert!(repeated.copied && !repeated.paste_attempted);
        assert_eq!(repeated.reason, "target_unavailable");
        assert_eq!(state.lock().unwrap().pastes, 1);
    }
    #[test]
    fn focus_changes_before_or_during_copy_never_paste_into_another_field() {
        for during in [false, true] {
            let (delivery, state) = fixture();
            let target = delivery.capture_target().unwrap();
            if during {
                let moved = state.clone();
                state.lock().unwrap().after_copy =
                    Some(Box::new(move || moved.lock().unwrap().focus = 2));
            } else {
                state.lock().unwrap().focus = 2;
            }
            let result = delivery
                .deliver(&target, "Keep this text.", &|| true)
                .unwrap();
            assert!(result.copied && !result.paste_attempted);
            assert_eq!(result.reason, "focus_changed");
            assert_eq!(state.lock().unwrap().pastes, 0);
        }
    }
    #[test]
    fn protected_unknown_or_permission_denied_targets_remain_copy_only() {
        for reason in [
            "accessibility_permission",
            "no_editable_target",
            "protected_field",
            "paste_unsupported",
        ] {
            let (delivery, state) = fixture();
            {
                let mut s = state.lock().unwrap();
                s.candidate = false;
                s.reason = reason;
            }
            let target = delivery.capture_target().unwrap();
            assert!(!target.paste_candidate());
            let result = delivery.deliver(&target, "Saved words.", &|| true).unwrap();
            assert!(result.copied && !result.paste_attempted);
            assert_eq!(result.reason, reason);
            assert_eq!(state.lock().unwrap().events, vec!["capture", "copy"]);
        }
    }
    #[test]
    fn cancellation_before_copy_or_after_copy_never_sends_paste() {
        for after_copy in [false, true] {
            let (delivery, state) = fixture();
            let target = delivery.capture_target().unwrap();
            let active = Arc::new(AtomicBool::new(after_copy));
            if after_copy {
                let cancel = active.clone();
                state.lock().unwrap().after_copy =
                    Some(Box::new(move || cancel.store(false, Ordering::SeqCst)));
            }
            let result = delivery
                .deliver(&target, "Do not deliver late.", &|| {
                    active.load(Ordering::SeqCst)
                })
                .unwrap();
            assert_eq!(result.copied, after_copy);
            assert!(!result.paste_attempted);
            assert_eq!(result.reason, "cancelled");
            assert_eq!(state.lock().unwrap().pastes, 0);
        }
    }
    #[test]
    fn cancelled_empty_failed_and_replaced_targets_do_not_cause_automatic_paste() {
        let (delivery, state) = fixture();
        let old = delivery.capture_target().unwrap();
        let current = delivery.capture_target().unwrap();
        let stale = delivery.deliver(&old, "Prior result.", &|| true).unwrap();
        assert!(!stale.paste_attempted);
        for text in ["", " \n\t", "injected\u{1b}control", "null\0byte"] {
            assert!(delivery.deliver(&current, text, &|| true).is_err());
        }
        assert_eq!(
            state.lock().unwrap().texts.len(),
            1,
            "empty/control data never changes the clipboard"
        );
        state.lock().unwrap().clipboard_fails = true;
        assert_eq!(
            delivery
                .deliver(&current, "Local text remains saved.", &|| true)
                .unwrap_err()
                .code,
            "clipboard_unavailable"
        );
        assert_eq!(state.lock().unwrap().pastes, 0);
        let target = delivery.capture_target().unwrap();
        delivery.discard_target(&target).unwrap();
        state.lock().unwrap().clipboard_fails = false;
        assert!(
            !delivery
                .deliver(&target, "Manual copy remains possible.", &|| true)
                .unwrap()
                .paste_attempted
        );
    }
    #[test]
    fn field_policy_rejects_secure_disabled_unfocused_readonly_and_terminal_apps() {
        assert!(eligible_field(
            "AXTextArea",
            Some(""),
            true,
            true,
            true,
            "com.apple.TextEdit"
        ));
        assert!(eligible_field(
            "AXTextField",
            Some("AXSearchField"),
            true,
            true,
            true,
            "com.apple.Safari"
        ));
        for app in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "com.mitchellh.ghostty",
            "org.alacritty",
            "net.kovidgoyal.kitty",
            "com.github.wez.wezterm",
            "dev.warp.Warp-Stable",
            "co.zeit.hyper",
            "org.tabby",
            "com.1password.1password",
            "com.apple.keychainaccess",
            "",
        ] {
            assert!(
                !eligible_field("AXTextArea", Some(""), true, true, true, app),
                "{app}"
            );
        }
        assert!(!eligible_field(
            "AXTextField",
            Some("AXSecureTextField"),
            true,
            true,
            true,
            "com.apple.Safari"
        ));
        assert!(!eligible_field(
            "AXTextField",
            None,
            true,
            true,
            true,
            "com.apple.Safari"
        ));
        assert!(!eligible_field(
            "AXButton",
            Some(""),
            true,
            true,
            true,
            "com.apple.TextEdit"
        ));
        for flags in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert!(!eligible_field(
                "AXTextArea",
                Some(""),
                flags.0,
                flags.1,
                flags.2,
                "com.apple.TextEdit"
            ));
        }
    }
}
