//! Public Notion authorization at the native edge. Protocol and translation roles are
//! deliberately combined here because the interface is small; core workflows see only
//! SecretPort's resolved access token, never OAuth/broker models or refresh credentials.
use super::chatgpt::http_client;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use tuck_core::{
    ports::{Result, SecretPort},
    AppError,
};
use url::Url;
use uuid::Uuid;

const KIND: &str = "tuck-notion-oauth-v1";
const MAX_RESPONSE: u64 = 65_536;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Deserialize, Serialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    workspace_id: String,
    workspace_name: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Credentials {
    kind: String,
    broker_origin: String,
    tokens: Tokens,
}
#[derive(Default)]
struct State {
    generation: u64,
    connecting: bool,
}

pub struct NotionAuth {
    secrets: Arc<dyn SecretPort>,
    broker: Option<String>,
    client: Client,
    state: Mutex<State>,
    wake: Condvar,
    refresh_lock: Mutex<()>,
    lease: Option<CredentialLease>,
}

// The keyring namespace is shared by every library/process. Keep one controller lease
// for the host's lifetime so independent generations cannot race on rotating credentials.
// This file contains no credentials, and is intentionally never unlinked on release.
struct CredentialLease {
    path: PathBuf,
    held: Mutex<Option<File>>,
}
impl CredentialLease {
    fn system() -> Result<Self> {
        #[cfg(target_os = "windows")]
        let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        #[cfg(not(target_os = "windows"))]
        let root = std::env::var_os("HOME").map(|home| {
            #[cfg(target_os = "macos")]
            {
                PathBuf::from(home).join("Library").join("Caches")
            }
            #[cfg(not(target_os = "macos"))]
            {
                PathBuf::from(home).join(".cache")
            }
        });
        let root =
            root.ok_or_else(|| error("Tuck could not locate protected local connection storage."))?;
        Ok(Self::at(
            root.join("com.sglyon.tuck.credentials").join("notion.lock"),
        ))
    }
    fn at(path: PathBuf) -> Self {
        Self {
            path,
            held: Mutex::new(None),
        }
    }
    fn acquire(&self) -> Result<()> {
        let mut held = self
            .held
            .lock()
            .map_err(|_| error("Notion credential control is unavailable."))?;
        if held.is_some() {
            return Ok(());
        }
        let directory = self
            .path
            .parent()
            .ok_or_else(|| error("Invalid Notion credential control location."))?;
        protect_directory(directory)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&self.path)
            .map_err(|_| error("Tuck could not open protected Notion credential control."))?;
        let metadata = file
            .metadata()
            .map_err(|_| error("Tuck could not verify Notion credential control."))?;
        if !metadata.is_file() {
            return Err(error(
                "Notion credential control must be a regular protected file.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
                || metadata.nlink() != 1
            {
                return Err(error(
                    "Notion credential control is not owned and protected by this user.",
                ));
            }
        }
        file.try_lock().map_err(|_| error("Another Tuck window or diagnostic process controls this Notion connection. Close it before changing or using the connection here."))?;
        *held = Some(file);
        Ok(())
    }
}
fn protect_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|_| error("Tuck could not create protected Notion credential control."))?;
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| error("Tuck could not verify protected Notion credential control."))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(error(
            "Notion credential control requires a private directory.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(error(
                "Notion credential control directory must be private to this user.",
            ));
        }
    }
    Ok(())
}
fn error(message: &str) -> AppError {
    AppError::new("notion_connection_error", message)
}
fn canceled() -> AppError {
    error("The Notion connection was canceled. Your existing connection was kept.")
}
fn opaque(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8_192
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && !value.starts_with(['{', '[', '"'])
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| error("Secure randomness is unavailable."))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn broker_origin(value: &str) -> Result<String> {
    let parsed = Url::parse(value)
        .map_err(|_| error("The Tuck Notion connection service is not configured correctly."))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || parsed.origin().ascii_serialization() != value.trim_end_matches('/')
    {
        return Err(error(
            "The Tuck Notion connection service requires its registered HTTPS origin.",
        ));
    }
    Ok(parsed.origin().ascii_serialization())
}
fn read_json(response: Response) -> Result<Value> {
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("The Notion connection service returned an unreadable response."))?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err(error(
            "The Notion connection service returned an oversized response.",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| error("The Notion connection service returned an invalid response."))
}
fn credentials(raw: &str) -> Result<Option<Credentials>> {
    if !raw.starts_with('{') {
        return Ok(None);
    }
    let stored: Credentials = serde_json::from_str(raw).map_err(|_| {
        error("Your saved Notion connection could not be read. Reconnect in Settings.")
    })?;
    if stored.kind != KIND || broker_origin(&stored.broker_origin)? != stored.broker_origin {
        return Err(error(
            "Your saved Notion connection is invalid. Reconnect in Settings.",
        ));
    }
    validate_tokens(&stored.tokens)?;
    Ok(Some(stored))
}
fn validate_tokens(value: &Tokens) -> Result<()> {
    if !token(&value.access_token)
        || value
            .refresh_token
            .as_deref()
            .is_some_and(|value| !token(value))
        || value.token_type != "bearer"
        || Uuid::parse_str(&value.workspace_id).is_err()
        || value
            .workspace_name
            .as_ref()
            .is_some_and(|name| name.len() > 1_024 || name.chars().any(char::is_control))
    {
        return Err(error(
            "Notion returned an invalid connection. No new credentials were saved.",
        ));
    }
    Ok(())
}
fn authorization_url(value: &str, broker: &str) -> Result<String> {
    let url =
        Url::parse(value).map_err(|_| error("The Notion authorization address is invalid."))?;
    let pairs: Vec<_> = url.query_pairs().collect();
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_ref())
    };
    if url.scheme() != "https"
        || url.host_str() != Some("api.notion.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/v1/oauth/authorize"
        || url.fragment().is_some()
        || pairs.len() != 5
        || [
            "owner",
            "client_id",
            "redirect_uri",
            "response_type",
            "state",
        ]
        .iter()
        .any(|name| pairs.iter().filter(|(key, _)| key == name).count() != 1)
        || get("owner") != Some("user")
        || get("response_type") != Some("code")
        || get("client_id").is_none_or(|id| id.is_empty() || id.len() > 200)
        || get("state").is_none_or(|state| !opaque(state))
        || get("redirect_uri") != Some(format!("{broker}/v1/notion/callback").as_str())
    {
        return Err(error(
            "The Notion authorization address did not match the registered connection.",
        ));
    }
    Ok(url.into())
}

impl NotionAuth {
    pub fn new(secrets: Arc<dyn SecretPort>) -> Result<Self> {
        // Release builds pin the operator's origin; a process environment cannot redirect tokens.
        let configured = option_env!("TUCK_NOTION_AUTH_BROKER_URL").map(str::to_owned);
        #[cfg(feature = "diagnostics")]
        let configured = configured.or_else(|| std::env::var("TUCK_NOTION_AUTH_BROKER_URL").ok());
        let mut auth = Self::with_broker(secrets, configured.as_deref())?;
        auth.lease = Some(CredentialLease::system()?);
        Ok(auth)
    }
    fn with_broker(secrets: Arc<dyn SecretPort>, configured: Option<&str>) -> Result<Self> {
        Ok(Self {
            secrets,
            broker: configured.map(broker_origin).transpose()?,
            client: http_client(Duration::from_secs(20))?,
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            refresh_lock: Mutex::new(()),
            lease: None,
        })
    }
    fn require_control(&self) -> Result<()> {
        if let Some(lease) = &self.lease {
            lease.acquire()?;
        }
        Ok(())
    }
    fn broker(&self) -> Result<&str> {
        self.broker.as_deref().ok_or_else(|| error("Notion sign-in is not available in this Tuck build. The Tuck operator must configure its public Notion connection service."))
    }
    fn post(&self, origin: &str, path: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{origin}/v1/notion/{path}"))
            .json(&body)
            .send()
            .map_err(|_| {
                error("Could not reach the Tuck Notion connection service. Try connecting again.")
            })?;
        if !response.status().is_success() {
            return Err(error(
                "The Notion connection could not be completed. Try connecting again.",
            ));
        }
        read_json(response)
    }
    fn current(&self, generation: u64) -> Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| error("Notion connection state is unavailable."))?;
        if state.generation != generation {
            return Err(canceled());
        }
        Ok(())
    }
    pub fn status(&self) -> Result<Value> {
        let state = self
            .state
            .lock()
            .map_err(|_| error("Notion connection state is unavailable."))?;
        let raw = self.secrets.get("notion")?;
        let saved = raw.as_deref().map(credentials).transpose()?.flatten();
        Ok(
            json!({"configured":self.broker.is_some(),"connected":raw.is_some(),"connecting":state.connecting,
            "mode":if saved.is_some(){Some("oauth")}else if raw.is_some(){Some("manual")}else{None},
            "workspaceName":saved.as_ref().and_then(|c|c.tokens.workspace_name.as_deref()),
            "workspaceId":saved.as_ref().map(|c|c.tokens.workspace_id.as_str()),
            "message":if self.broker.is_none(){Some("Notion sign-in is not available in this Tuck build.")}else{None}}),
        )
    }
    pub fn connect(&self) -> Result<Value> {
        let broker = self.broker()?.to_owned();
        self.require_control()?;
        let generation = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error("Notion connection state is unavailable."))?;
            state.generation = state.generation.wrapping_add(1);
            state.connecting = true;
            self.wake.notify_all();
            state.generation
        };
        let result = self.connect_inner(
            &broker,
            generation,
            &|path, body| self.post(&broker, path, body),
            |url| {
                webbrowser::open(url)
                    .map(|_| ())
                    .map_err(|_| error("Your browser could not open. Try connecting Notion again."))
            },
            Duration::from_secs(2),
        );
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error("Notion connection state is unavailable."))?;
            if state.generation == generation {
                state.connecting = false;
            }
        }
        result?;
        self.status()
    }
    fn connect_inner(
        &self,
        broker: &str,
        generation: u64,
        post: &impl Fn(&str, Value) -> Result<Value>,
        open: impl FnOnce(&str) -> Result<()>,
        poll_delay: Duration,
    ) -> Result<()> {
        let verifier = random()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let start = post("start", json!({"challenge":challenge}))?;
        let id = start["transactionId"]
            .as_str()
            .filter(|id| opaque(id))
            .ok_or_else(|| {
                error("The Notion connection service returned an invalid transaction.")
            })?;
        let proof = json!({"transactionId":id,"verifier":verifier});
        let result = (|| {
            self.current(generation)?;
            let url = authorization_url(start["authorizationUrl"].as_str().unwrap_or(""), broker)?;
            open(&url)?;
            let deadline = Instant::now() + CONNECT_TIMEOUT;
            while Instant::now() < deadline {
                self.current(generation)?;
                let response = post("complete", proof.clone())?;
                self.current(generation)?;
                if response["denied"] == true {
                    return Err(error(
                        "Notion access was declined. Your existing connection was kept.",
                    ));
                }
                if response["pending"] == true {
                    let state = self
                        .state
                        .lock()
                        .map_err(|_| error("Notion connection state is unavailable."))?;
                    if state.generation != generation {
                        return Err(canceled());
                    }
                    let _ = self.wake.wait_timeout(state, poll_delay);
                    continue;
                }
                let tokens: Tokens = serde_json::from_value(response).map_err(|_| {
                    error(
                        "Notion returned an unreadable connection. No new credentials were saved.",
                    )
                })?;
                validate_tokens(&tokens)?;
                let stored = Credentials {
                    kind: KIND.into(),
                    broker_origin: broker.into(),
                    tokens,
                };
                self.save_if_current(generation, &stored)?;
                return Ok(());
            }
            Err(error(
                "The Notion connection timed out. Try connecting again.",
            ))
        })();
        if result.is_err() {
            let _ = post("cancel", proof);
        }
        result
    }
    pub fn cancel(&self) -> Result<Value> {
        self.invalidate()?;
        self.status()
    }
    fn invalidate(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| error("Notion connection state is unavailable."))?;
        state.generation = state.generation.wrapping_add(1);
        state.connecting = false;
        self.wake.notify_all();
        Ok(())
    }
    // Credential replacement and generation comparison share the same short critical section.
    // Browser/network work never holds this lock, so sign-out and manual saves always win.
    fn save_if_current(&self, generation: u64, value: &Credentials) -> Result<()> {
        validate_tokens(&value.tokens)?;
        let encoded = serde_json::to_string(value)
            .map_err(|_| error("Could not encode Notion credentials."))?;
        let state = self
            .state
            .lock()
            .map_err(|_| error("Notion connection state is unavailable."))?;
        if state.generation != generation {
            return Err(canceled());
        }
        self.secrets.set("notion", &encoded)
    }
    pub fn set_manual_token(&self, value: &str) -> Result<()> {
        let value = value.trim();
        if !token(value) {
            return Err(AppError::invalid("Enter a valid Notion token."));
        }
        self.require_control()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| error("Notion connection state is unavailable."))?;
        state.generation = state.generation.wrapping_add(1);
        state.connecting = false;
        self.wake.notify_all();
        self.secrets.set("notion", value)
    }
    pub fn disconnect(&self) -> Result<Value> {
        self.require_control()?;
        let old = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error("Notion connection state is unavailable."))?;
            state.generation = state.generation.wrapping_add(1);
            state.connecting = false;
            self.wake.notify_all();
            let old = self.secrets.get("notion")?;
            self.secrets.delete("notion")?;
            old
        };
        let saved = old.as_deref().map(credentials).transpose();
        let warning = match saved {
            Ok(Some(Some(saved))) => self.revoke(&saved).is_err(),
            Err(_) => true,
            _ => false,
        };
        let mut status = self.status()?;
        if warning {
            status["revocationWarning"] = json!("Disconnected from this device. Tuck could not confirm removal in Notion; remove Tuck in Notion Connections to revoke remote access.");
        }
        Ok(status)
    }
    fn revoke(&self, saved: &Credentials) -> Result<()> {
        if self.broker()? != saved.broker_origin {
            return Err(error(
                "The Notion connection service changed. Remove Tuck in Notion Connections.",
            ));
        }
        self.post(
            &saved.broker_origin,
            "revoke",
            json!({"token":saved.tokens.access_token}),
        )?;
        Ok(())
    }
    pub fn access_token(&self) -> Result<String> {
        self.require_control()?;
        let (generation, raw) = {
            let state = self
                .state
                .lock()
                .map_err(|_| error("Notion connection state is unavailable."))?;
            (
                state.generation,
                self.secrets
                    .get("notion")?
                    .ok_or_else(|| error("Connect Notion in Settings first."))?,
            )
        };
        let Some(saved) = credentials(&raw)? else {
            return Ok(raw);
        };
        if self.broker()? != saved.broker_origin {
            return Err(error(
                "The Tuck Notion connection service changed. Reconnect Notion in Settings.",
            ));
        }
        // Probe before a publication starts. Only an actual 401 can rotate credentials;
        // no ambiguous write request is retried, and one resolved token covers the operation.
        let response = self
            .client
            .get("https://api.notion.com/v1/users/me")
            .bearer_auth(&saved.tokens.access_token)
            .header("Notion-Version", "2026-03-11")
            .send()
            .map_err(|_| {
                error("Could not verify your Notion connection. Try again when connected.")
            })?;
        self.current(generation)?;
        if response.status().is_success() {
            return Ok(saved.tokens.access_token);
        }
        if response.status().as_u16() != 401 {
            return Err(error(
                "Notion could not verify this connection. Check access and try again.",
            ));
        }
        let _refresh = self
            .refresh_lock
            .lock()
            .map_err(|_| error("Notion refresh is unavailable."))?;
        let latest = {
            let state = self
                .state
                .lock()
                .map_err(|_| error("Notion connection state is unavailable."))?;
            if state.generation != generation {
                return Err(canceled());
            }
            self.secrets.get("notion")?.ok_or_else(canceled)?
        };
        if latest != raw {
            // Another serialized refresh completed within this same connection generation.
            return credentials(&latest)?
                .map(|c| c.tokens.access_token)
                .ok_or_else(canceled);
        }
        let refresh = saved.tokens.refresh_token.as_deref().ok_or_else(|| {
            error("Your Notion connection needs renewed access. Reconnect Notion in Settings.")
        })?;
        let renewed = self.post(
            &saved.broker_origin,
            "refresh",
            json!({"refreshToken":refresh}),
        )?;
        let tokens: Tokens = serde_json::from_value(renewed)
            .map_err(|_| error("Notion could not renew this connection. Reconnect in Settings."))?;
        validate_tokens(&tokens)?;
        if tokens.workspace_id != saved.tokens.workspace_id {
            return Err(error(
                "Notion returned a different workspace during renewal. Reconnect in Settings.",
            ));
        }
        let new = Credentials { tokens, ..saved };
        self.save_if_current(generation, &new)?;
        Ok(new.tokens.access_token)
    }
}

impl SecretPort for NotionAuth {
    fn get(&self, kind: &str) -> Result<Option<String>> {
        if kind == "notion" {
            return self.access_token().map(Some);
        }
        self.secrets.get(kind)
    }
    fn set(&self, kind: &str, value: &str) -> Result<()> {
        if kind == "notion" {
            return self.set_manual_token(value);
        }
        self.secrets.set(kind, value)
    }
    fn delete(&self, kind: &str) -> Result<()> {
        if kind == "notion" {
            self.disconnect()?;
            return Ok(());
        }
        self.secrets.delete(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Secrets(Mutex<HashMap<String, String>>);
    impl SecretPort for Secrets {
        fn get(&self, kind: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(kind).cloned())
        }
        fn set(&self, kind: &str, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(kind.into(), value.into());
            Ok(())
        }
        fn delete(&self, kind: &str) -> Result<()> {
            self.0.lock().unwrap().remove(kind);
            Ok(())
        }
    }
    fn setup() -> (NotionAuth, Arc<Secrets>) {
        let secrets = Arc::new(Secrets::default());
        (
            NotionAuth::with_broker(secrets.clone(), Some("https://auth.example.com")).unwrap(),
            secrets,
        )
    }
    fn stored() -> Credentials {
        Credentials {
            kind: KIND.into(),
            broker_origin: "https://auth.example.com".into(),
            tokens: Tokens {
                access_token: "ntn_access".into(),
                refresh_token: Some("nrt_refresh".into()),
                token_type: "bearer".into(),
                workspace_id: "00000000-0000-4000-8000-000000000001".into(),
                workspace_name: Some("My workspace".into()),
            },
        }
    }
    #[test]
    fn manual_compatibility_and_status_never_expose_credentials() {
        let (auth, _) = setup();
        auth.set_manual_token("ntn_manual").unwrap();
        assert_eq!(auth.access_token().unwrap(), "ntn_manual");
        let status = auth.status().unwrap();
        assert_eq!(status["mode"], "manual");
        assert_eq!(status["connected"], true);
        assert!(!status.to_string().contains("ntn_manual"));
    }
    #[test]
    fn oauth_status_projects_workspace_without_network_or_tokens() {
        let (auth, _) = setup();
        auth.save_if_current(0, &stored()).unwrap();
        let status = auth.status().unwrap();
        assert_eq!(status["workspaceName"], "My workspace");
        assert_eq!(status["workspaceId"], stored().tokens.workspace_id);
        assert_eq!(status["mode"], "oauth");
        assert!(!status.to_string().contains("ntn_access"));
        assert!(!status.to_string().contains("nrt_refresh"));
    }
    #[test]
    fn unavailable_build_keeps_existing_manual_workflow() {
        let secrets = Arc::new(Secrets::default());
        let auth = NotionAuth::with_broker(secrets, None).unwrap();
        assert_eq!(auth.status().unwrap()["configured"], false);
        assert!(auth.connect().is_err());
        auth.set_manual_token("manual").unwrap();
        assert_eq!(auth.access_token().unwrap(), "manual");
    }
    #[test]
    fn manual_token_cannot_forge_oauth_bundle() {
        let (auth, _) = setup();
        for value in [
            serde_json::to_string(&stored()).unwrap(),
            "{bad}".into(),
            "[token]".into(),
            "a\nb".into(),
            "\"token\"".into(),
            "a b".into(),
            "".into(),
            "x".repeat(8193),
        ] {
            assert!(auth.set_manual_token(&value).is_err());
        }
        assert_eq!(auth.status().unwrap()["connected"], false);
    }
    #[test]
    fn cancellation_signout_and_manual_save_defeat_stale_connect_or_refresh() {
        for mutation in ["cancel", "disconnect", "manual"] {
            let (auth, secrets) = setup();
            auth.set_manual_token("initial").unwrap();
            let generation = auth.state.lock().unwrap().generation;
            auth.state.lock().unwrap().connecting = true;
            match mutation {
                "cancel" => {
                    auth.cancel().unwrap();
                }
                "disconnect" => {
                    auth.disconnect().unwrap();
                }
                _ => {
                    auth.set_manual_token("replacement").unwrap();
                }
            }
            assert!(auth.save_if_current(generation, &stored()).is_err());
            assert_eq!(auth.status().unwrap()["connecting"], false);
            assert_eq!(
                secrets.get("notion").unwrap().as_deref(),
                match mutation {
                    "cancel" => Some("initial"),
                    "disconnect" => None,
                    _ => Some("replacement"),
                }
            );
        }
    }
    #[test]
    fn repeated_connect_generations_cannot_replace_the_latest_flow() {
        let (auth, secrets) = setup();
        auth.state.lock().unwrap().generation = 2;
        assert!(auth.save_if_current(1, &stored()).is_err());
        assert!(secrets.get("notion").unwrap().is_none());
        auth.save_if_current(2, &stored()).unwrap();
        assert!(credentials(&secrets.get("notion").unwrap().unwrap())
            .unwrap()
            .is_some());
    }
    #[test]
    fn broker_origin_and_authorization_destination_are_pinned() {
        for origin in [
            "http://auth.example.com",
            "https://user@auth.example.com",
            "https://auth.example.com/path",
            "https://auth.example.com/?x=1",
            "https://auth.example.com/#x",
        ] {
            assert!(broker_origin(origin).is_err());
        }
        let broker = "https://auth.example.com";
        let mut url = Url::parse("https://api.notion.com/v1/oauth/authorize").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("owner", "user"),
            ("client_id", "id"),
            (
                "redirect_uri",
                "https://auth.example.com/v1/notion/callback",
            ),
            ("response_type", "code"),
            ("state", &"x".repeat(43)),
        ]);
        assert!(authorization_url(url.as_str(), broker).is_ok());
        let original = url.clone();
        url.set_host(Some("evil.example")).unwrap();
        assert!(authorization_url(url.as_str(), broker).is_err());
        url = original.clone();
        url.set_fragment(Some("token"));
        assert!(authorization_url(url.as_str(), broker).is_err());
        url = original.clone();
        url.query_pairs_mut().append_pair("state", &"x".repeat(43));
        assert!(authorization_url(url.as_str(), broker).is_err());
        assert!(authorization_url(original.as_str(), "https://other.example.com").is_err());
    }
    #[test]
    fn malformed_provider_and_saved_credentials_are_rejected() {
        let mut saved = stored();
        saved.tokens.workspace_id = "bad".into();
        assert!(validate_tokens(&saved.tokens).is_err());
        saved = stored();
        saved.tokens.access_token = "a\nb".into();
        assert!(validate_tokens(&saved.tokens).is_err());
        saved = stored();
        saved.tokens.workspace_name = Some("untrusted\nname".into());
        assert!(validate_tokens(&saved.tokens).is_err());
        assert!(credentials("{broken}").is_err());
        saved = stored();
        saved.kind = "other".into();
        assert!(credentials(&serde_json::to_string(&saved).unwrap()).is_err());
    }
    #[test]
    fn credential_controller_lease_excludes_independent_owners_and_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private").join("notion.lock");
        let first = CredentialLease::at(path.clone());
        let second = CredentialLease::at(path.clone());
        first.acquire().unwrap();
        first.acquire().unwrap();
        assert!(second.acquire().is_err());
        assert_eq!(fs::read(&path).unwrap(), Vec::<u8>::new());
        drop(first);
        second.acquire().unwrap();
        let third = CredentialLease::at(path.clone());
        assert!(third.acquire().is_err());
        drop(second);
        third.acquire().unwrap();
        assert!(path.exists());
    }
    #[cfg(unix)]
    #[test]
    fn credential_controller_rejects_symlinked_and_public_locations() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        protect_directory(&private).unwrap();
        let actual = private.join("actual");
        fs::write(&actual, []).unwrap();
        let linked = private.join("notion.lock");
        symlink(&actual, &linked).unwrap();
        assert!(CredentialLease::at(linked).acquire().is_err());
        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(CredentialLease::at(actual).acquire().is_err());
    }
    #[test]
    fn nullable_refresh_credential_is_accepted_without_fabricating_a_refresh_token() {
        let mut saved = stored();
        saved.tokens.refresh_token = None;
        validate_tokens(&saved.tokens).unwrap();
        let encoded = serde_json::to_string(&saved).unwrap();
        assert!(credentials(&encoded)
            .unwrap()
            .unwrap()
            .tokens
            .refresh_token
            .is_none());
        let (auth, _) = setup();
        auth.save_if_current(0, &saved).unwrap();
        assert_eq!(auth.status().unwrap()["connected"], true);
    }
    fn start_reply() -> Value {
        let id = "t".repeat(43);
        let mut url = Url::parse("https://api.notion.com/v1/oauth/authorize").unwrap();
        url.query_pairs_mut().extend_pairs([
            ("owner", "user"),
            ("client_id", "client"),
            (
                "redirect_uri",
                "https://auth.example.com/v1/notion/callback",
            ),
            ("response_type", "code"),
            ("state", &id),
        ]);
        json!({"transactionId":id,"authorizationUrl":url.to_string()})
    }
    #[test]
    fn actual_native_connect_polls_pending_and_saves_verified_tokens_only_to_keyring() {
        let (auth, secrets) = setup();
        let requests = Mutex::new(Vec::new());
        let polled = Mutex::new(false);
        auth.connect_inner(
            "https://auth.example.com",
            0,
            &|path, body| {
                requests.lock().unwrap().push((path.to_owned(), body));
                match path {
                    "start" => Ok(start_reply()),
                    "complete" if !*polled.lock().unwrap() => {
                        *polled.lock().unwrap() = true;
                        Ok(json!({"pending":true}))
                    }
                    "complete" => Ok(serde_json::to_value(stored().tokens).unwrap()),
                    _ => panic!("Unexpected broker request"),
                }
            },
            |url| {
                assert!(url.starts_with("https://api.notion.com/v1/oauth/authorize?"));
                assert!(!url.contains("verifier"));
                assert!(!url.contains("ntn_"));
                Ok(())
            },
            Duration::from_millis(1),
        )
        .unwrap();
        let calls = requests.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec!["start", "complete", "complete"]
        );
        let proof = &calls[1].1;
        assert_eq!(
            calls[0].1["challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(
                proof["verifier"].as_str().unwrap().as_bytes()
            ))
        );
        assert_eq!(proof, &calls[2].1);
        assert_eq!(auth.status().unwrap()["workspaceName"], "My workspace");
        let raw = secrets.get("notion").unwrap().unwrap();
        assert!(raw.contains("ntn_access"));
        assert!(!auth.status().unwrap().to_string().contains("ntn_access"));
    }
    #[test]
    fn actual_native_connect_cannot_overwrite_cancel_manual_or_signout_during_deferred_exchange() {
        use std::sync::mpsc;
        for mutation in ["cancel", "manual", "disconnect"] {
            let (auth, secrets) = setup();
            auth.set_manual_token("initial").unwrap();
            let generation = auth.state.lock().unwrap().generation;
            let auth = Arc::new(auth);
            let worker_auth = auth.clone();
            let (waiting_tx, waiting_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let canceled = Arc::new(Mutex::new(false));
            let canceled_copy = canceled.clone();
            let worker = std::thread::spawn(move || {
                worker_auth.connect_inner(
                    "https://auth.example.com",
                    generation,
                    &|path, _body| match path {
                        "start" => Ok(start_reply()),
                        "complete" => {
                            waiting_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            Ok(serde_json::to_value(stored().tokens).unwrap())
                        }
                        "cancel" => {
                            *canceled_copy.lock().unwrap() = true;
                            Ok(json!({"canceled":true}))
                        }
                        _ => panic!("Unexpected broker request"),
                    },
                    |_| Ok(()),
                    Duration::from_millis(1),
                )
            });
            waiting_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            match mutation {
                "cancel" => {
                    auth.cancel().unwrap();
                }
                "manual" => auth.set_manual_token("replacement").unwrap(),
                _ => {
                    auth.disconnect().unwrap();
                }
            }
            release_tx.send(()).unwrap();
            assert!(worker.join().unwrap().is_err());
            assert!(*canceled.lock().unwrap());
            assert_eq!(
                secrets.get("notion").unwrap().as_deref(),
                match mutation {
                    "cancel" => Some("initial"),
                    "manual" => Some("replacement"),
                    _ => None,
                }
            );
        }
    }
}
