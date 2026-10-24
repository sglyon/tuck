use super::*;
pub(super) struct Platform;
impl Backend for Platform {
    type Target = ();
    fn capture(&self) -> Capture<()> {
        Capture {
            target: None,
            reason: "paste_unsupported",
        }
    }
    fn validate(&self, _: &()) -> std::result::Result<(), &'static str> {
        Err("paste_unsupported")
    }
    fn paste_if_current(
        &self,
        _: &(),
        _: &dyn Fn() -> bool,
    ) -> std::result::Result<(), &'static str> {
        Err("paste_unsupported")
    }
    fn status(&self) -> Value {
        json!({"pasteAvailable":false,"pastePermission":"unavailable","pasteMessage":"Dictation copies text here. Automatic paste is currently supported only on macOS."})
    }
}
