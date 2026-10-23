//! OS delivery only. The core owns eligibility and durable at-most-once claims.
use serde_json::{json, Value};
use tuck_core::{ports::Result, AppError};

pub trait Notifications: Send + Sync {
    /// This operation must never request access or show a notification.
    fn permission(&self) -> Result<&'static str>;
    /// Called only by the explicit enable command.
    fn request_permission(&self) -> Result<&'static str>;
    /// Generic text is intentional: calendar details never reach the lock screen.
    fn show(&self) -> Result<()>;
}

pub fn unavailable() -> AppError {
    AppError::new("notification_unavailable", "Native reminders are unavailable in this host. Open the installed Tuck app to enable notifications.")
}

pub fn status(permission: &str, enabled: bool, running: bool, error: Option<&str>) -> Value {
    let message = error.or(match permission {
        "denied" => Some("Notifications are blocked. Allow Tuck in your system notification settings. In-app reminders remain available."),
        "unknown" => Some("Your system controls notification delivery. Tuck cannot verify whether notifications are allowed."),
        "unavailable" => Some("Native reminders are unavailable in this host. In-app reminders remain available."),
        _ => None,
    });
    json!({"available":permission != "unavailable", "permission":permission,"enabled":enabled,"running":running,"message":message})
}

#[cfg(not(target_os = "macos"))]
pub struct DesktopNotifications(pub tauri::AppHandle);
#[cfg(not(target_os = "macos"))]
impl Notifications for DesktopNotifications {
    fn permission(&self) -> Result<&'static str> {
        // The maintained plugin's desktop permission API always returns Granted;
        // it cannot observe OS policy on Windows/Linux. Never present that as a grant.
        Ok("unknown")
    }
    fn request_permission(&self) -> Result<&'static str> {
        self.permission()
    }
    fn show(&self) -> Result<()> {
        use tauri_plugin_notification::NotificationExt;
        self.0
            .notification()
            .builder()
            .title("Tuck")
            .body("A meeting starts soon. Open Tuck when you’re ready.")
            .show()
            .map_err(|_| {
                AppError::new(
                    "notification_failed",
                    "The system could not accept the meeting reminder.",
                )
            })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use objc2::{rc::autoreleasepool, runtime::Bool};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotificationRequest, UNNotificationSettings, UNUserNotificationCenter,
    };
    use std::{ptr::NonNull, sync::mpsc, time::Duration};

    pub struct MacNotifications;
    fn center() -> Result<objc2::rc::Retained<UNUserNotificationCenter>> {
        let bundle = NSBundle::mainBundle();
        // UserNotifications raises an ObjC exception without an application bundle.
        // Diagnostics and unbundled development must not call it.
        if bundle.bundleIdentifier().is_none() || !bundle.bundlePath().to_string().ends_with(".app")
        {
            return Err(unavailable());
        }
        Ok(UNUserNotificationCenter::currentNotificationCenter())
    }
    fn callback_error() -> AppError {
        AppError::new("notification_failed", "The system did not respond to the notification request. Check Tuck's notification settings.")
    }
    impl Notifications for MacNotifications {
        fn permission(&self) -> Result<&'static str> {
            autoreleasepool(|_| {
                let (send, receive) = mpsc::sync_channel(1);
                let callback = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                    // SAFETY: Apple's completion handler supplies a live settings object
                    // for the duration of this callback. We copy only its enum value.
                    let value = unsafe { settings.as_ref() }.authorizationStatus();
                    let permission = match value {
                        UNAuthorizationStatus::Authorized
                        | UNAuthorizationStatus::Provisional
                        | UNAuthorizationStatus::Ephemeral => "granted",
                        UNAuthorizationStatus::Denied => "denied",
                        UNAuthorizationStatus::NotDetermined => "notDetermined",
                        _ => "unknown",
                    };
                    let _ = send.try_send(permission);
                });
                center()?.getNotificationSettingsWithCompletionHandler(&callback);
                receive
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(|_| callback_error())
            })
        }
        fn request_permission(&self) -> Result<&'static str> {
            autoreleasepool(|_| {
                let (send, receive) = mpsc::sync_channel(1);
                let callback = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                    let _ = send.try_send(if error.is_null() {
                        Ok(if granted.as_bool() {
                            "granted"
                        } else {
                            "denied"
                        })
                    } else {
                        Err(callback_error())
                    });
                });
                center()?.requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert,
                    &callback,
                );
                // Explicit user requests may stay open while the person considers them.
                receive
                    .recv_timeout(Duration::from_secs(60))
                    .map_err(|_| callback_error())?
            })
        }
        fn show(&self) -> Result<()> {
            autoreleasepool(|_| {
                let content = UNMutableNotificationContent::new();
                content.setTitle(&NSString::from_str("Tuck"));
                content.setBody(&NSString::from_str(
                    "A meeting starts soon. Open Tuck when you’re ready.",
                ));
                let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                    &NSString::from_str(&uuid::Uuid::new_v4().to_string()),
                    &content,
                    None,
                );
                let (send, receive) = mpsc::sync_channel(1);
                let callback = RcBlock::new(move |error: *mut NSError| {
                    let _ = send.try_send(error.is_null());
                });
                center()?.addNotificationRequest_withCompletionHandler(&request, Some(&callback));
                if receive
                    .recv_timeout(Duration::from_secs(4))
                    .map_err(|_| callback_error())?
                {
                    Ok(())
                } else {
                    Err(callback_error())
                }
            })
        }
    }
}
#[cfg(target_os = "macos")]
pub use macos::MacNotifications;
