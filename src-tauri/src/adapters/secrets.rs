use tuck_core::{
    ports::{Result, SecretPort},
    AppError,
};

/// Native OS credentials only. A missing/unlocked store is never replaced by plaintext.
#[derive(Default)]
pub struct NativeSecrets;
impl NativeSecrets {
    fn entry(kind: &str) -> Result<keyring::Entry> {
        validate_kind(kind)?;
        keyring::Entry::new("com.sglyon.tuck", kind).map_err(|_| unavailable())
    }
}
fn unavailable() -> AppError {
    AppError::new(
        "credentials_unavailable",
        "Your system credential store is unavailable. Unlock it and try again.",
    )
}
impl SecretPort for NativeSecrets {
    fn get(&self, kind: &str) -> Result<Option<String>> {
        match Self::entry(kind)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(unavailable()),
        }
    }
    fn set(&self, kind: &str, value: &str) -> Result<()> {
        if value.trim().is_empty() || value.len() > 100_000 {
            return Err(AppError::invalid("Enter a valid credential."));
        }
        Self::entry(kind)?
            .set_password(value.trim())
            .map_err(|_| unavailable())
    }
    fn delete(&self, kind: &str) -> Result<()> {
        match Self::entry(kind)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(unavailable()),
        }
    }
}

fn validate_kind(kind: &str) -> Result<()> {
    if !matches!(kind, "openai" | "notion") && !kind.starts_with("chatgpt:") {
        return Err(AppError::invalid("Unknown credential kind."));
    }
    if kind.len() > 200 {
        return Err(AppError::invalid("Invalid credential identifier."));
    }
    if kind == "chatgpt:" || kind.chars().any(char::is_control) {
        return Err(AppError::invalid("Invalid credential identifier."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_are_validated_before_accessing_system_store() {
        for key in ["openai", "notion", "chatgpt:account-id"] {
            assert!(validate_kind(key).is_ok());
        }
        for key in ["unknown", "chatgpt:", "chatgpt:a\nother", &"x".repeat(201)] {
            assert!(validate_kind(key).is_err());
        }
        assert!(NativeSecrets.set("openai", "   ").is_err());
        assert!(NativeSecrets.set("openai", &"a".repeat(100_001)).is_err());
    }
}
