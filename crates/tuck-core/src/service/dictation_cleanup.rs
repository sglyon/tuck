use super::*;
use crate::dictation_cleanup::{self, MAX_PROMPT_CHARS, MAX_TRANSCRIPT_CHARS, RULES};

impl MeetingService {
    pub fn format_dictation(&self, text: &str, prompt: &str) -> Result<String> {
        domain::text(&json!(text), "Dictation", MAX_TRANSCRIPT_CHARS, false)?;
        domain::text(&json!(prompt), "Cleanup prompt", MAX_PROMPT_CHARS, false)?;
        let settings = self.get_settings()?;
        if settings["globalDictation"]["cleanupEnabled"] != true {
            return Err(AppError::unavailable("Dictation cleanup is off."));
        }
        if settings["provider"] != "local" && settings["hostedConsent"] != true {
            return Err(AppError::unavailable("Allow hosted transcript processing in Intelligence settings to use cleanup with this provider."));
        }
        let system = format!("{RULES}\n\nFormatting preferences:\n{prompt}");
        let candidate = self.adapters.intelligence.complete_dictation(
            &settings,
            &system,
            &[json!({"role":"user","content":text})],
        )?;
        let current = self.get_settings()?;
        for key in ["provider", "model", "hostedConsent", "localUrl"] {
            if current[key] != settings[key] {
                return Err(AppError::unavailable("AI settings changed during cleanup."));
            }
        }
        dictation_cleanup::validate(text, &candidate)
    }
    pub fn save_dictation_cleanup(&self, id: &str, original: &str, cleaned: &str) -> Result<()> {
        let cleaned = dictation_cleanup::validate(original, cleaned)?;
        self.transaction(|repo| {
            let mut meeting = repo.get(id)?;
            if !meeting.is_dictation()
                || meeting.status != "complete"
                || meeting.notes.trim() != original.trim()
            {
                return Err(AppError::unavailable(
                    "The saved voice note changed while cleanup was running.",
                ));
            }
            meeting.update(&json!({"notes":cleaned}))?;
            repo.save(&mut meeting)?;
            Ok(Value::Null)
        })?;
        self.changed(id)?;
        Ok(())
    }
}
