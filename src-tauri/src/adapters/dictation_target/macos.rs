use super::*;
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::NSString;
use std::ptr;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementGetTypeID() -> usize;
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: CFTypeRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> i32;
    fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> i32;
}
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

pub(super) struct Platform;
struct Element(CFType);
// AXUIElement is an immutable retained handle to a remote element. All access to held
// targets is serialized by Delivery's mutex; the native API supports background callers.
unsafe impl Send for Element {}
pub(super) struct Target {
    element: Element,
    pid: i32,
    bundle: String,
}
fn checked_element(value: CFType) -> std::result::Result<Element, &'static str> {
    if value.type_of() != unsafe { AXUIElementGetTypeID() } {
        return Err("target_unavailable");
    }
    Ok(Element(value))
}
fn attribute(element: &Element, name: &str) -> std::result::Result<Option<CFType>, ()> {
    let name = CFString::new(name);
    let mut value: CFTypeRef = ptr::null();
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            element.0.as_CFTypeRef(),
            name.as_concrete_TypeRef(),
            &mut value,
        )
    };
    if error == -25205 || error == -25212 {
        return Ok(None);
    } // unsupported / no value
    if error != 0 || value.is_null() {
        return Err(());
    }
    Ok(Some(unsafe { CFType::wrap_under_create_rule(value) }))
}
fn string_attribute(element: &Element, name: &str) -> std::result::Result<Option<String>, ()> {
    attribute(element, name)?
        .map(|value| {
            value
                .downcast::<CFString>()
                .map(|s| s.to_string())
                .ok_or(())
        })
        .transpose()
}
fn boolean_attribute(element: &Element, name: &str) -> std::result::Result<bool, ()> {
    attribute(element, name)?
        .and_then(|value| value.downcast::<CFBoolean>())
        .map(|value| value == CFBoolean::true_value())
        .ok_or(())
}
fn settable(element: &Element, name: &str) -> bool {
    let name = CFString::new(name);
    let mut answer = 0u8;
    unsafe {
        AXUIElementIsAttributeSettable(
            element.0.as_CFTypeRef(),
            name.as_concrete_TypeRef(),
            &mut answer,
        ) == 0
            && answer != 0
    }
}
fn bundle_for_pid(pid: i32) -> Option<String> {
    // Application identity only: never inspect window titles, text values or clipboard data.
    let app: Option<Retained<AnyObject>> = unsafe {
        msg_send![class!(NSRunningApplication),runningApplicationWithProcessIdentifier:pid]
    };
    let identifier: Option<Retained<NSString>> =
        unsafe { msg_send![app.as_ref()?, bundleIdentifier] };
    identifier.map(|id| id.to_string())
}
fn focused() -> std::result::Result<Target, &'static str> {
    if !unsafe { AXIsProcessTrusted() } {
        return Err("accessibility_permission");
    }
    if unsafe { IsSecureEventInputEnabled() } {
        return Err("protected_field");
    }
    let system = unsafe { AXUIElementCreateSystemWide() };
    if system.is_null() {
        return Err("target_unavailable");
    }
    let system = Element(unsafe { CFType::wrap_under_create_rule(system) });
    // A slow or non-cooperating app must not stall recording controls indefinitely.
    if unsafe { AXUIElementSetMessagingTimeout(system.0.as_CFTypeRef(), 0.2) } != 0 {
        return Err("target_unavailable");
    }
    let application = checked_element(
        attribute(&system, "AXFocusedApplication")
            .map_err(|_| "target_unavailable")?
            .ok_or("target_unavailable")?,
    )?;
    unsafe { AXUIElementSetMessagingTimeout(application.0.as_CFTypeRef(), 0.2) };
    let element = checked_element(
        attribute(&application, "AXFocusedUIElement")
            .map_err(|_| "target_unavailable")?
            .ok_or("no_editable_target")?,
    )?;
    unsafe { AXUIElementSetMessagingTimeout(element.0.as_CFTypeRef(), 0.2) };
    let mut pid = 0i32;
    if unsafe { AXUIElementGetPid(element.0.as_CFTypeRef(), &mut pid) } != 0 || pid <= 0 {
        return Err("target_unavailable");
    }
    let bundle = bundle_for_pid(pid).ok_or("target_unavailable")?;
    let role = string_attribute(&element, "AXRole")
        .map_err(|_| "target_unavailable")?
        .ok_or("no_editable_target")?;
    let subrole = string_attribute(&element, "AXSubrole").map_err(|_| "target_unavailable")?;
    let enabled = boolean_attribute(&element, "AXEnabled").unwrap_or(false);
    let focused = boolean_attribute(&element, "AXFocused").unwrap_or(false);
    // Inspect writability metadata only. Never request AXValue or AXSelectedText contents.
    let writable = settable(&element, "AXValue") || settable(&element, "AXSelectedText");
    // A missing subrole is normal for ordinary text areas. Text fields need an explicit
    // non-secure subrole, otherwise the adapter conservatively leaves text on the clipboard.
    let subrole = if role == "AXTextArea" && subrole.is_none() {
        Some("")
    } else {
        subrole.as_deref()
    };
    if !eligible_field(&role, subrole, enabled, focused, writable, &bundle) {
        return Err("no_editable_target");
    }
    Ok(Target {
        element,
        pid,
        bundle,
    })
}
fn current(target: &Target) -> std::result::Result<(), &'static str> {
    let latest = focused()?;
    if latest.pid != target.pid
        || latest.bundle != target.bundle
        || latest.element.0 != target.element.0
    {
        return Err("focus_changed");
    }
    Ok(())
}
impl Backend for Platform {
    type Target = Target;
    fn capture(&self) -> Capture<Target> {
        match focused() {
            Ok(target) => Capture {
                target: Some(target),
                reason: "ready",
            },
            Err(reason) => Capture {
                target: None,
                reason,
            },
        }
    }
    fn validate(&self, target: &Target) -> std::result::Result<(), &'static str> {
        current(target)
    }
    fn paste_if_current(
        &self,
        target: &Target,
        allowed: &dyn Fn() -> bool,
    ) -> std::result::Result<(), &'static str> {
        let source =
            CGEventSource::new(CGEventSourceStateID::Private).map_err(|_| "paste_unavailable")?;
        let down = CGEvent::new_keyboard_event(source.clone(), 9, true)
            .map_err(|_| "paste_unavailable")?;
        let up = CGEvent::new_keyboard_event(source, 9, false).map_err(|_| "paste_unavailable")?;
        down.set_flags(CGEventFlags::CGEventFlagCommand);
        up.set_flags(CGEventFlags::CGEventFlagCommand);
        // Last check follows clipboard writing and event construction. Never activate an
        // app or search for a replacement target. OS event delivery is not insertion proof.
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if !allowed() {
                return Err("cancelled");
            }
            let flags = unsafe { CGEventSourceFlagsState(1) }; // physical HID state, not synthetic event flags
            let held = CGEventFlags::CGEventFlagCommand
                | CGEventFlags::CGEventFlagShift
                | CGEventFlags::CGEventFlagAlternate
                | CGEventFlags::CGEventFlagControl
                | CGEventFlags::CGEventFlagSecondaryFn;
            if flags & held.bits() == 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err("modifiers_held");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        current(target)?;
        if !allowed() {
            return Err("cancelled");
        }
        down.post_to_pid(target.pid);
        up.post_to_pid(target.pid);
        Ok(())
    }
    fn status(&self) -> Value {
        let allowed = unsafe { AXIsProcessTrusted() };
        json!({"pasteAvailable":allowed,"pastePermission":if allowed{"granted"}else{"denied"},"pasteMessage":if allowed{"Tuck can send Paste when the original editable text field is still focused. Text is always copied."}else{"Allow Tuck in macOS Accessibility to paste into the same text field. Dictation still copies your text without this permission."}})
    }
}
