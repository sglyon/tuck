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
    fn contains(&self, kind: &str) -> Result<bool> {
        validate_kind(kind)?;
        #[cfg(target_os = "macos")]
        {
            metadata_presence(kind, |query| {
                use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
                let mut options = ItemSearchOptions::new();
                options
                    .class(ItemClass::generic_password())
                    .service(query.service)
                    .account(query.account)
                    .limit(Limit::Max(1))
                    .load_attributes(query.attributes)
                    .load_data(query.data)
                    .load_refs(query.references)
                    .skip_authenticated_items(query.skip_authentication);
                options
                    .search()
                    .map(|items| items.len())
                    .map_err(|error| error.code())
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.get(kind)
                .map(|value| value.is_some_and(|secret| !secret.is_empty()))
        }
    }
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

/// A fixed read-only query, separated so tests validate policy without touching a real keychain.
#[cfg(target_os = "macos")]
struct MetadataQuery<'a> {
    service: &'static str,
    account: &'a str,
    attributes: bool,
    data: bool,
    references: bool,
    skip_authentication: bool,
}
#[cfg(target_os = "macos")]
fn metadata_presence(
    kind: &str,
    search: impl FnOnce(MetadataQuery<'_>) -> std::result::Result<usize, i32>,
) -> Result<bool> {
    validate_kind(kind)?;
    // keyring-core's default get_attributes and macOS get_credential both read secret
    // data. Query attributes directly; UI-skip excludes anything requiring authentication.
    let query = MetadataQuery {
        service: "com.sglyon.tuck",
        account: kind,
        attributes: true,
        data: false,
        references: false,
        skip_authentication: true,
    };
    match search(query) {
        Ok(count) => Ok(count != 0),
        Err(-25300) => Ok(false), // errSecItemNotFound
        Err(_) => Err(unavailable()),
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
    #[cfg(target_os = "macos")]
    #[test]
    fn presence_uses_only_scoped_attributes_and_never_authentication_or_secret_data() {
        let present = metadata_presence("notion", |query| {
            assert_eq!(query.service, "com.sglyon.tuck");
            assert_eq!(query.account, "notion");
            assert!(query.attributes);
            assert!(!query.data);
            assert!(!query.references);
            assert!(query.skip_authentication);
            Ok(1)
        })
        .unwrap();
        assert!(present);
        assert!(!metadata_presence("openai", |_| Ok(0)).unwrap());
        assert!(!metadata_presence("openai", |_| Err(-25300)).unwrap());
        for code in [-25308, -25293, -50] {
            assert_eq!(
                metadata_presence("notion", |_| Err(code)).unwrap_err().code,
                "credentials_unavailable"
            );
        }
        assert!(
            metadata_presence("unknown", |_| panic!("Invalid scope reached OS query")).is_err()
        );
    }
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
