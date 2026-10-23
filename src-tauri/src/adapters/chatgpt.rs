//! Official public ChatGPT OAuth. Tokens stay in the native credential store.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tuck_core::{
    ports::{AccountPort, Result, SecretPort},
    AppError,
};
use url::Url;
use uuid::Uuid;

pub const AUTHORITY: &str = "https://auth.openai.com";
pub const RESOURCE: &str = "https://api.openai.com/v1";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/api/accounts/oauth/token";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: String,
    client_id: String,
    email: String,
    subject: Option<String>,
    label: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registrations {
    host_id: String,
    active_id: Option<String>,
    profiles: Vec<Profile>,
}

pub struct ChatGptAuth {
    state: Mutex<Registrations>,
    path: PathBuf,
    secrets: Arc<dyn SecretPort>,
    client: Client,
    sign_in_lock: Mutex<()>,
    refresh_lock: Mutex<()>,
    session_generation: AtomicU64,
}
fn error(message: &str) -> AppError {
    AppError::new("account_error", message)
}
fn epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
fn equal(left: &str, right: &str) -> bool {
    constant_time_eq::constant_time_eq(left.as_bytes(), right.as_bytes())
}
fn random_value() -> Result<String> {
    let mut data = [0u8; 48];
    getrandom::fill(&mut data).map_err(|_| error("Secure randomness is unavailable."))?;
    Ok(URL_SAFE_NO_PAD.encode(data))
}

pub fn http_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| error("The secure network client could not start."))
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Invalid account storage directory."))?;
    fs::create_dir_all(parent)
        .map_err(|_| error("Tuck could not create local account storage."))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| error("Tuck could not save account registration."))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| error("Could not protect account registration."))?;
    }
    serde_json::to_writer(&mut file, value)
        .map_err(|_| error("Could not encode account registration."))?;
    file.flush()
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| error("Could not save account registration."))?;
    file.persist(path)
        .map_err(|_| error("Could not replace account registration."))?;
    Ok(())
}

impl ChatGptAuth {
    pub fn new(data_dir: &Path, secrets: Arc<dyn SecretPort>) -> Result<Self> {
        let path = data_dir.join("chatgpt-registrations.json");
        let state = if path.exists() {
            serde_json::from_slice(&fs::read(&path).map_err(|_|error("Could not read ChatGPT account registrations."))?).map_err(|_|error("ChatGPT account data could not be read. Restore its local backup before reconnecting."))?
        } else {
            Registrations {
                host_id: format!("urn:uuid:{}", Uuid::new_v4()),
                active_id: None,
                profiles: vec![],
            }
        };
        validate_registrations(&state)?;
        atomic_json(&path, &state)?;
        Ok(Self {
            state: Mutex::new(state),
            path,
            secrets,
            client: http_client(Duration::from_secs(30))?,
            sign_in_lock: Mutex::new(()),
            refresh_lock: Mutex::new(()),
            session_generation: AtomicU64::new(0),
        })
    }
    fn profile(&self, id: Option<&str>) -> Result<Profile> {
        let state = self
            .state
            .lock()
            .map_err(|_| error("Account state is unavailable."))?;
        let selected = id.or(state.active_id.as_deref());
        state
            .profiles
            .iter()
            .find(|p| Some(p.id.as_str()) == selected)
            .cloned()
            .ok_or_else(|| error("Connect your ChatGPT account in Settings first."))
    }
    fn credentials(&self, profile: &Profile) -> Result<Value> {
        match self.secrets.get(&format!("chatgpt:{}", profile.id))? {
            Some(raw) => {
                let value: Value = serde_json::from_str(&raw).map_err(|_| {
                    error("Your ChatGPT credentials could not be read. Reconnect in Settings.")
                })?;
                if !value.is_object() {
                    return Err(error(
                        "Your ChatGPT credentials could not be read. Reconnect in Settings.",
                    ));
                }
                Ok(value)
            }
            None => Ok(json!({})),
        }
    }
    fn save_credentials(&self, profile: &Profile, tokens: &Value) -> Result<()> {
        self.secrets
            .set(&format!("chatgpt:{}", profile.id), &tokens.to_string())
    }
    fn verify_identity(&self, token: &str, client_id: &str, nonce: &str) -> Result<Value> {
        let failure = || {
            error("ChatGPT identity verification failed. No new credentials were saved; reconnect and try again.")
        };
        let metadata: Value = self
            .client
            .get(format!("{AUTHORITY}/.well-known/openid-configuration"))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|_| failure())?;
        let jwks_uri = metadata["jwks_uri"].as_str().ok_or_else(failure)?;
        let url = Url::parse(jwks_uri).map_err(|_| failure())?;
        if metadata["issuer"] != AUTHORITY || !authority_url(&url) {
            return Err(failure());
        }
        let jwks: JwkSet = self
            .client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|_| failure())?;
        verify_token(token, &jwks, client_id, nonce)
    }
    pub fn access_token(&self) -> Result<String> {
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| error("Account refresh is unavailable."))?;
        let profile = self.profile(None)?;
        let mut credentials = self.credentials(&profile)?;
        if !has_plan(&credentials) {
            return Err(error(
                "ChatGPT plan usage is not enabled. Reconnect and approve plan usage.",
            ));
        }
        if credentials["expiresAt"].as_f64().unwrap_or(0.) <= epoch() + 90. {
            let refresh = credentials["refresh_token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| error("Your ChatGPT connection expired. Reconnect in Settings."))?;
            let renewed: Value = self
                .client
                .post(TOKEN_ENDPOINT)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", profile.client_id.as_str()),
                    ("refresh_token", refresh),
                    ("resource", RESOURCE),
                ])
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json())
                .map_err(|_| {
                    error("Your ChatGPT connection could not be renewed. Reconnect in Settings.")
                })?;
            credentials = merge_refresh(&credentials, &renewed, &profile.client_id, epoch())?;
            self.save_credentials(&profile, &credentials)?;
            if !has_plan(&credentials) {
                return Err(error(
                    "ChatGPT plan permission is no longer enabled. Reconnect in Settings.",
                ));
            }
        }
        credentials["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| error("Reconnect your ChatGPT account in Settings."))
    }
    pub fn models(&self) -> Result<Vec<Value>> {
        let raw: Value = self
            .client
            .get(format!("{RESOURCE}/models"))
            .bearer_auth(self.access_token()?)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|_| {
                error("Could not load models from ChatGPT. Check your account and connection.")
            })?;
        model_choices(&raw)
    }
    fn sign_in_inner(&self, id: Option<&str>) -> Result<Vec<Value>> {
        let generation = self.session_generation.load(Ordering::SeqCst);
        let selected = id.map(|id| self.profile(Some(id))).transpose()?;
        let old = selected
            .as_ref()
            .map(|p| self.credentials(p))
            .transpose()?
            .unwrap_or(json!({}));
        let (state, nonce, verifier) = (random_value()?, random_value()?, random_value()?);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let server = tiny_http::Server::http(("127.0.0.1", 0))
            .map_err(|_| error("Tuck could not open the local sign-in callback. Try again."))?;
        let address = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| error("Invalid local callback address."))?;
        let redirect = format!("http://127.0.0.1:{}/auth/callback", address.port());
        let host = self
            .state
            .lock()
            .map_err(|_| error("Account state unavailable."))?
            .host_id
            .clone();
        let initial = selected.is_none();
        let requested_client = selected
            .as_ref()
            .map(|p| p.client_id.as_str())
            .unwrap_or("dynamic_agent_client");
        let mut authorize = Url::parse(&format!("{AUTHORITY}/api/accounts/authorize"))
            .map_err(|_| error("Invalid sign-in address."))?;
        {
            let mut q = authorize.query_pairs_mut();
            q.extend_pairs([
                ("client_id", requested_client),
                ("ext_agent_host_id", host.as_str()),
                ("response_type", "code"),
                ("redirect_uri", redirect.as_str()),
                (
                    "scope",
                    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
                ),
                ("resource", RESOURCE),
                ("state", state.as_str()),
                ("nonce", nonce.as_str()),
                ("code_challenge_method", "S256"),
                ("code_challenge", challenge.as_str()),
            ]);
            if initial {
                q.append_pair("agent_name_hint", "Tuck");
            } else {
                if let Some(hint) = old["id_token"].as_str() {
                    q.append_pair("id_token_hint", hint);
                }
                if let Some(p) = selected.as_ref() {
                    if !p.email.is_empty() {
                        q.append_pair("login_hint", &p.email);
                    }
                }
            }
        }
        webbrowser::open(authorize.as_str()).map_err(|_| {
            error("Tuck could not open your browser. Set a default browser and try again.")
        })?;
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut callback = None;
        while Instant::now() < deadline {
            if let Some(request) = server
                .recv_timeout(Duration::from_millis(500))
                .map_err(|_| error("The local sign-in callback stopped."))?
            {
                let parsed = Url::parse(&format!("http://127.0.0.1{}", request.url())).ok();
                let pairs = parsed
                    .as_ref()
                    .map(|u| {
                        u.query_pairs()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let value = |name: &str| {
                    let values = pairs.iter().filter(|(k, _)| k == name).collect::<Vec<_>>();
                    if values.len() == 1 {
                        Some(values[0].1.clone())
                    } else {
                        None
                    }
                };
                let valid = request.method() == &tiny_http::Method::Get
                    && parsed
                        .as_ref()
                        .is_some_and(|u| u.path() == "/auth/callback")
                    && equal(&value("state").unwrap_or_default(), &state);
                let body = if valid {
                    "Return to Tuck to finish connecting."
                } else {
                    "Invalid sign-in link. Start again in Tuck."
                };
                let response = tiny_http::Response::from_string(body)
                    .with_status_code(if valid { 200 } else { 400 })
                    .with_header(
                        tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap(),
                    );
                let _ = request.respond(response);
                if valid {
                    callback = Some((value("code"), value("client_id"), value("error")));
                    break;
                }
            }
        }
        let (code, returned_client, oauth_error) =
            callback.ok_or_else(|| error("ChatGPT sign-in timed out. Start again in Settings."))?;
        if oauth_error.is_some() {
            return Err(error(
                "ChatGPT sign-in was not approved. Your existing account has not changed.",
            ));
        }
        let issued = returned_client
            .or_else(|| selected.as_ref().map(|p| p.client_id.clone()))
            .ok_or_else(|| error("ChatGPT registration did not return a client ID."))?;
        if issued.trim().is_empty()
            || issued == "dynamic_agent_client"
            || selected.as_ref().is_some_and(|p| p.client_id != issued)
        {
            return Err(error(
                "ChatGPT returned an unexpected registration. Start sign-in again.",
            ));
        }
        let code = code
            .filter(|s| !s.is_empty())
            .ok_or_else(|| error("ChatGPT did not return an authorization code."))?;
        let pending = {
            let mut saved = self
                .state
                .lock()
                .map_err(|_| error("Account state unavailable."))?;
            let found = saved
                .profiles
                .iter()
                .find(|p| p.client_id == issued)
                .cloned();
            let pending = found.unwrap_or_else(|| Profile {
                id: Uuid::new_v4().to_string(),
                client_id: issued.clone(),
                email: String::new(),
                subject: None,
                label: format!("ChatGPT account {}", saved.profiles.len() + 1),
            });
            if !saved.profiles.iter().any(|p| p.id == pending.id) {
                saved.profiles.push(pending.clone());
            }
            atomic_json(&self.path, &*saved)?;
            pending
        };
        let tokens:Value=self.client.post(TOKEN_ENDPOINT).form(&[("grant_type","authorization_code"),("client_id",issued.as_str()),("code",code.as_str()),("code_verifier",verifier.as_str()),("redirect_uri",redirect.as_str()),("resource",RESOURCE)]).send().and_then(|r|r.error_for_status()).and_then(|r|r.json()).map_err(|_|error("ChatGPT could not complete sign-in. Retry with this saved account in Settings."))?;
        let identity = self.verify_identity(
            tokens["id_token"]
                .as_str()
                .ok_or_else(|| error("ChatGPT returned incomplete credentials."))?,
            &issued,
            &nonce,
        )?;
        self.finish_sign_in(&pending, tokens, &identity, generation)?;
        self.accounts()
    }
    // Serialize credential replacement with refresh/sign-out; never restore a session after sign-out.
    fn finish_sign_in(
        &self,
        pending: &Profile,
        mut tokens: Value,
        identity: &Value,
        generation: u64,
    ) -> Result<()> {
        let _refresh = self
            .refresh_lock
            .lock()
            .map_err(|_| error("Account state unavailable."))?;
        if self.session_generation.load(Ordering::SeqCst) != generation {
            return Err(error(
                "The account was signed out during this sign-in. Start again in Settings.",
            ));
        }
        validate_token_response(&tokens, &pending.client_id)?;
        tokens["scopes"] = json!(tokens["scope"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>());
        tokens["expiresAt"] = json!(epoch() + token_lifetime(&tokens)?);
        let mut saved = self
            .state
            .lock()
            .map_err(|_| error("Account state unavailable."))?;
        let mut candidate = saved.clone();
        let canonical = candidate
            .profiles
            .iter_mut()
            .find(|p| p.id == pending.id && p.client_id == pending.client_id)
            .ok_or_else(|| error("Account registration changed during sign-in."))?;
        let subject = identity["sub"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| error("ChatGPT identity verification failed."))?;
        if canonical
            .subject
            .as_deref()
            .is_some_and(|sub| sub != subject)
        {
            return Err(error(
                "This sign-in belongs to a different account. Add it separately.",
            ));
        }
        canonical.subject = Some(subject.to_owned());
        canonical.email = identity["email"].as_str().unwrap_or("").to_owned();
        // Persist verified identity before credentials, so a crash cannot erase their account binding.
        atomic_json(&self.path, &candidate)?;
        *saved = candidate.clone();
        self.save_credentials(pending, &tokens)?;
        candidate.active_id = Some(pending.id.clone());
        atomic_json(&self.path, &candidate)?;
        *saved = candidate;
        Ok(())
    }
}
fn has_plan(tokens: &Value) -> bool {
    tokens["scopes"]
        .as_array()
        .is_some_and(|s| s.iter().any(|s| s == PLAN_SCOPE))
}
pub fn model_choices(raw: &Value) -> Result<Vec<Value>> {
    let models = raw["models"]
        .as_array()
        .ok_or_else(|| error("ChatGPT returned an unreadable model catalog."))?;
    Ok(models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| Some(json!({"id":m["slug"].as_str()?,"name":m["display_name"].as_str()?})))
        .collect())
}
impl AccountPort for ChatGptAuth {
    fn accounts(&self) -> Result<Vec<Value>> {
        let saved = self
            .state
            .lock()
            .map_err(|_| error("Account state unavailable."))?;
        saved.profiles.iter().map(|p|{let tokens=self.credentials(p)?;let connected=tokens["access_token"].as_str().is_some_and(|s|!s.is_empty());
            Ok(json!({"id":p.id,"email":p.email,"label":p.label,"connected":connected,"sharing":connected&&has_plan(&tokens),"active":saved.active_id.as_ref()==Some(&p.id)}))}).collect()
    }
    fn sign_in(&self, id: Option<&str>) -> Result<Vec<Value>> {
        let _guard = self
            .sign_in_lock
            .try_lock()
            .map_err(|_| error("A ChatGPT sign-in is already open in your browser."))?;
        self.sign_in_inner(id)
    }
    fn select(&self, id: &str) -> Result<Vec<Value>> {
        let _refresh = self
            .refresh_lock
            .lock()
            .map_err(|_| error("Account state unavailable."))?;
        self.profile(Some(id))?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error("Account state unavailable."))?;
            let mut candidate = state.clone();
            candidate.active_id = Some(id.to_string());
            atomic_json(&self.path, &candidate)?;
            *state = candidate;
        }
        self.accounts()
    }
    fn sign_out(&self, id: &str) -> Result<Value> {
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| error("Account state unavailable."))?;
        let profile = self.profile(Some(id))?;
        self.session_generation.fetch_add(1, Ordering::SeqCst);
        let tokens = self.credentials(&profile)?;
        let mut revoked = tokens["refresh_token"].as_str().is_none();
        if let Some(refresh) = tokens["refresh_token"].as_str() {
            let metadata = self
                .client
                .get(format!("{AUTHORITY}/.well-known/openid-configuration"))
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.json::<Value>());
            if let Ok(meta) = metadata {
                if let Some(endpoint) = meta["revocation_endpoint"].as_str() {
                    if let Ok(url) = Url::parse(endpoint) {
                        if meta["issuer"] == AUTHORITY && authority_url(&url) {
                            for attempt in 0..3 {
                                match self
                                    .client
                                    .post(url.clone())
                                    .form(&[
                                        ("token", refresh),
                                        ("token_type_hint", "refresh_token"),
                                        ("client_id", profile.client_id.as_str()),
                                    ])
                                    .send()
                                {
                                    Ok(r) if r.status().as_u16() == 200 => {
                                        revoked = true;
                                        break;
                                    }
                                    Ok(r) if !r.status().is_server_error() => break,
                                    _ => {}
                                }
                                if attempt < 2 {
                                    std::thread::sleep(Duration::from_millis(250 * (1 << attempt)));
                                }
                            }
                        }
                    }
                }
            }
        }
        self.secrets.delete(&format!("chatgpt:{}", profile.id))?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| error("Account state unavailable."))?;
            if state.active_id.as_deref() == Some(id) {
                state.active_id = None;
            }
            atomic_json(&self.path, &*state)?;
        }
        Ok(json!({"accounts":self.accounts()?,"remoteRevoked":revoked}))
    }
}

fn authority_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("auth.openai.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
fn validate_registrations(state: &Registrations) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    let mut clients = std::collections::HashSet::new();
    if state.host_id.trim().is_empty()
        || state.profiles.iter().any(|p| {
            p.id.trim().is_empty()
                || p.id.len() > 128
                || p.client_id.trim().is_empty()
                || p.client_id == "dynamic_agent_client"
                || !ids.insert(&p.id)
                || !clients.insert(&p.client_id)
        })
        || state
            .active_id
            .as_ref()
            .is_some_and(|id| !state.profiles.iter().any(|p| &p.id == id))
    {
        return Err(error(
            "ChatGPT account data could not be read. Restore its local backup before reconnecting.",
        ));
    }
    Ok(())
}
fn token_lifetime(tokens: &Value) -> Result<f64> {
    match tokens.get("expires_in") {
        None => Ok(3600.),
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.)
            .ok_or_else(|| {
                error("ChatGPT returned an invalid credential lifetime. Reconnect in Settings.")
            }),
    }
}
fn validate_token_response(tokens: &Value, client_id: &str) -> Result<()> {
    if !tokens.is_object()
        || tokens["access_token"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        || tokens
            .get("refresh_token")
            .is_some_and(|v| v.as_str().is_none_or(str::is_empty))
        || tokens.get("scope").is_some_and(|v| !v.is_string())
        || tokens
            .get("token_type")
            .is_some_and(|v| v.as_str().is_none_or(|s| !s.eq_ignore_ascii_case("bearer")))
        || tokens
            .get("client_id")
            .is_some_and(|v| v.as_str() != Some(client_id))
    {
        return Err(error(
            "ChatGPT returned incomplete or mismatched credentials. Reconnect in Settings.",
        ));
    }
    token_lifetime(tokens)?;
    Ok(())
}
fn merge_refresh(current: &Value, renewed: &Value, client_id: &str, at: f64) -> Result<Value> {
    validate_token_response(renewed, client_id)?;
    let mut result = current.clone();
    let obj = result.as_object_mut().ok_or_else(|| {
        error("Your ChatGPT credentials could not be read. Reconnect in Settings.")
    })?;
    for (k, v) in renewed
        .as_object()
        .ok_or_else(|| error("Invalid ChatGPT credentials."))?
    {
        // A retained ID token is an identity hint that was verified during sign-in. A refresh's
        // replacement is not needed for inference and has no fresh browser nonce to validate.
        if !matches!(k.as_str(), "id_token" | "scopes" | "expiresAt") {
            obj.insert(k.clone(), v.clone());
        }
    }
    if let Some(scope) = renewed["scope"].as_str() {
        result["scopes"] = json!(scope.split_whitespace().collect::<Vec<_>>());
    }
    result["expiresAt"] = json!(at + token_lifetime(renewed)?);
    Ok(result)
}
fn verify_token(token: &str, jwks: &JwkSet, client_id: &str, nonce: &str) -> Result<Value> {
    let failure = || {
        error("ChatGPT identity verification failed. No new credentials were saved; reconnect and try again.")
    };
    let header = decode_header(token).map_err(|_| failure())?;
    if header.alg != Algorithm::RS256 || nonce.is_empty() {
        return Err(failure());
    }
    let key = jwks
        .find(header.kid.as_deref().ok_or_else(failure)?)
        .ok_or_else(failure)?;
    let decoding = DecodingKey::from_jwk(key).map_err(|_| failure())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.leeway = 0;
    validation.validate_nbf = true;
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[AUTHORITY]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = decode::<Value>(token, &decoding, &validation)
        .map_err(|_| failure())?
        .claims;
    if claims["sub"].as_str().is_none_or(str::is_empty)
        || !claims["nonce"].as_str().is_some_and(|s| equal(s, nonce))
        || claims
            .get("azp")
            .is_some_and(|v| v.as_str() != Some(client_id))
        || (claims["aud"].as_array().is_some_and(|a| a.len() > 1) && claims["azp"] != client_id)
    {
        return Err(failure());
    }
    Ok(claims)
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Secrets(Mutex<HashMap<String, String>>);
    impl SecretPort for Secrets {
        fn get(&self, k: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> Result<()> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> Result<()> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }
    fn profile(id: &str, client: &str, subject: &str) -> Profile {
        Profile {
            id: id.into(),
            client_id: client.into(),
            email: format!("{id}@example.test"),
            subject: Some(subject.into()),
            label: id.into(),
        }
    }
    fn setup() -> (tempfile::TempDir, ChatGptAuth, Arc<Secrets>) {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(Secrets::default());
        let auth = ChatGptAuth::new(dir.path(), secrets.clone()).unwrap();
        {
            let mut state = auth.state.lock().unwrap();
            state.profiles = vec![
                profile("a", "client-a", "alice"),
                profile("b", "client-b", "bob"),
            ];
            state.active_id = Some("a".into());
            atomic_json(&auth.path, &*state).unwrap();
        }
        for id in ["a", "b"] {
            secrets.set(&format!("chatgpt:{id}"),&json!({"access_token":format!("token-{id}"),"scopes":[PLAN_SCOPE],"expiresAt":epoch()+3600.,"id_token":format!("verified-{id}")}).to_string()).unwrap();
        }
        (dir, auth, secrets)
    }
    #[test]
    fn local_accounts_preserve_host_and_redact_credentials() {
        let (dir, auth, secrets) = setup();
        let host = auth.state.lock().unwrap().host_id.clone();
        let accounts = auth.accounts().unwrap();
        assert_eq!(accounts[0]["active"], true);
        assert!(!json!(accounts).to_string().contains("token-a"));
        assert_eq!(auth.access_token().unwrap(), "token-a");
        auth.select("b").unwrap();
        assert_eq!(auth.access_token().unwrap(), "token-b");
        let replacement = ChatGptAuth::new(dir.path(), secrets).unwrap();
        assert_eq!(replacement.state.lock().unwrap().host_id, host);
        let raw = fs::read_to_string(dir.path().join("chatgpt-registrations.json")).unwrap();
        assert!(!raw.contains("token-a"));
        assert!(!raw.contains("verified-a"));
    }
    #[test]
    fn account_subject_and_signout_generation_prevent_credential_overwrite() {
        let (_dir, auth, secrets) = setup();
        let pending = auth.profile(Some("a")).unwrap();
        let tokens = json!({"access_token":"replacement","scope":PLAN_SCOPE,"expires_in":3600});
        assert!(auth
            .finish_sign_in(&pending, tokens.clone(), &json!({"sub":"bob"}), 0)
            .is_err());
        assert!(secrets
            .get("chatgpt:a")
            .unwrap()
            .unwrap()
            .contains("token-a"));
        // No renewable session in these fake credentials, so sign-out performs no network request.
        let signed_out = auth.sign_out("a").unwrap();
        assert_eq!(signed_out["remoteRevoked"], true);
        assert!(auth
            .finish_sign_in(&pending, tokens.clone(), &json!({"sub":"alice"}), 0)
            .is_err());
        assert!(secrets.get("chatgpt:a").unwrap().is_none());
        auth.finish_sign_in(
            &pending,
            tokens,
            &json!({"sub":"alice","email":"alice@example.test"}),
            1,
        )
        .unwrap();
        assert_eq!(auth.access_token().unwrap(), "replacement");
        assert!(secrets
            .get("chatgpt:b")
            .unwrap()
            .unwrap()
            .contains("token-b"));
    }
    #[test]
    fn refresh_rotation_retains_verified_identity_and_authoritative_scopes() {
        let current = json!({"access_token":"old","refresh_token":"refresh-old","id_token":"verified","scopes":[PLAN_SCOPE]});
        let renewed = json!({"access_token":"new","refresh_token":"rotated","id_token":"unverified","expires_in":600,"scopes":["untrusted"]});
        let merged = merge_refresh(&current, &renewed, "client", 100.).unwrap();
        assert_eq!(merged["refresh_token"], "rotated");
        assert_eq!(merged["id_token"], "verified");
        assert!(has_plan(&merged));
        assert_eq!(merged["expiresAt"], 700.);
        let revoked = merge_refresh(
            &merged,
            &json!({"access_token":"revoked","scope":"openid","expires_in":120}),
            "client",
            200.,
        )
        .unwrap();
        assert!(!has_plan(&revoked));
        for malformed in [
            json!(null),
            json!([]),
            json!({"access_token":"new","scope":false}),
            json!({"access_token":"new","expires_in":-1}),
            json!({"access_token":"new","client_id":"other"}),
            json!({"access_token":"new","refresh_token":""}),
        ] {
            assert!(merge_refresh(&current, &malformed, "client", 100.).is_err());
        }
    }
    #[test]
    fn malformed_credentials_and_registrations_fail_closed() {
        let (dir, auth, secrets) = setup();
        secrets.set("chatgpt:a", "[]").unwrap();
        assert!(auth.access_token().is_err());
        assert!(auth.accounts().is_err());
        let mut state = auth.state.lock().unwrap().clone();
        state.profiles[1].client_id = "client-a".into();
        atomic_json(&auth.path, &state).unwrap();
        assert!(ChatGptAuth::new(dir.path(), secrets).is_err());
    }
    #[test]
    fn pkce_is_fresh_base64url_sha256() {
        let a = random_value().unwrap();
        let b = random_value().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(a.as_bytes()));
        assert_eq!(challenge.len(), 43);
    }
    fn signed(claims: &Value) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
        header.kid = Some("test-key".into());
        jsonwebtoken::encode(
            &header,
            claims,
            &jsonwebtoken::EncodingKey::from_rsa_der(
                &base64::engine::general_purpose::STANDARD
                    .decode(TEST_KEY)
                    .unwrap(),
            ),
        )
        .unwrap()
    }
    #[test]
    fn jwt_checks_signature_issuer_audience_expiry_nonce_and_identity() {
        let jwks: JwkSet = serde_json::from_str(TEST_JWKS).unwrap();
        let claims = json!({"iss":AUTHORITY,"aud":"client-a","sub":"alice","nonce":"nonce","exp":epoch() as u64+3600});
        assert_eq!(
            verify_token(&signed(&claims), &jwks, "client-a", "nonce").unwrap()["sub"],
            "alice"
        );
        for (name, value) in [
            ("iss", json!("https://evil.test")),
            ("aud", json!("client-b")),
            ("exp", json!(epoch() as u64 - 1)),
            ("nonce", json!("other")),
            ("nonce", json!(null)),
            ("sub", json!("")),
            ("nbf", json!(epoch() as u64 + 3600)),
            ("azp", json!("client-b")),
        ] {
            let mut bad = claims.clone();
            bad[name] = value;
            assert!(
                verify_token(&signed(&bad), &jwks, "client-a", "nonce").is_err(),
                "{name}"
            );
        }
        let mut multi = claims.clone();
        multi["aud"] = json!(["client-a", "client-b"]);
        assert!(verify_token(&signed(&multi), &jwks, "client-a", "nonce").is_err());
        multi["azp"] = json!("client-a");
        assert!(verify_token(&signed(&multi), &jwks, "client-a", "nonce").is_ok());
        let token = signed(&claims);
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
        parts[1] = URL_SAFE_NO_PAD.encode(json!({"sub":"mallory"}).to_string());
        assert!(verify_token(&parts.join("."), &jwks, "client-a", "nonce").is_err());
        let hs = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"wrong-algorithm"),
        )
        .unwrap();
        assert!(verify_token(&hs, &jwks, "client-a", "nonce").is_err());
    }
    // Generated, disposable RSA fixture. Never used outside signature regression tests.
    const TEST_KEY: &str = "MIIEpAIBAAKCAQEA6JF+SUNx74BZkMWW5XO1CaK9wJtqboMmBrAtC5/3yvLi2RfIPiZo0JrxuiX8DOGZYfnKSADCo51iSk/M3nMq6Py/gdKnChVL1e8otsBlQmVRE4IdeMh9paY1vDcnPjVBn917Aoh0DM3Ifg3Jqw5VQ7kJNNtFNuH6ROjo1rWHZXOIpBRgnfIGj4EhVcP736m/Ht/lwKy/3K9bBd5CB3TUV7ZQLX3n0FYUF1x7iJYqeJwO1+WkcznxbkCBeHMM83Vt6mkrGsxZhEC7dI96l2+2wg0ANT/YzIbA3mFQRWf8y0XKyT4Uq8I4shwFW9uB5QLUYhijIMmbyG3bFk7KifMPxwIDAQABAoIBACGy2fR+Ctd+0WY6Uzvqe6wql3R+BeUp02E7DD6NI90VkPbkTMtrSlsWZc9Ta1sRzLqsdKSL7lLB3nj7vijqJtNoCCHPZw0oJSXEcob53Xz07FNiJ6jAOFXqfbUeMsKp+dkoa7IOw+4KWrJKh327OnHxMQXRKI15dLJLzgKKftTIAFE7ALMqXl0c5oYSeLYMZqNJ3B3IfRc8Ta/XfWq1UKxWX7oFCfNi8UtB/TfB+eO3OJENJicVT9cDnOXsWTtmjXyLeLLhXPFVmOGOfxbwjQh7hEOA4ZCeS7edu/Ab1YBALWScvnYKfWoQYkXgM3YAfPPQOFCZkvUEueLyB12to+ECgYEA+Gz3y2w0RbOXIt1PiP+nBKN7QvcCVHGl9ZPga0C8gQPMh1f941f3nwifGi4uTceV9GJRItENCelZg3NnD675umknM2150030iwdpyhKjYu0+O+DAu8Uo8dukjmHQFUzIiX3COKzIAYTtPYj+N2Vuc5grsUDJk7D3QsZzbeXMg+ECgYEA76jBJrp1R6H/ARmFg8AX8/qVCZkhCOgZTud+SJawCf4k8Cs/QWy/lBXqoDjcJHsKnaF69BU9GElsubqgXimGucUjwjoyDID3ZSqAMYLcw4S1eloClRnPRFAZjiECqjqs7UWMoBrmAvPSLvIJr8i0iTfPzKxIohRDhm8akOqqCKcCgYEA15R4VKdJlVMLzM2Qv6pbHcNgq/P+qZkWE86H6Yr8Cllpm+nYAPnNgMzShL2eoRCaKMFI95XCYt+FNnVm8k09mW62/gH6vkvf8uYz/4+9CZQndX7MwiWOKpQYcRagpFtFi0r+yH/n8KmR1tyS/y+eceGTyPYX93qSl/fvI2dP2wECgYAoMjWzOTLSgOk0VgN+xYk+8phss7JGvCcChPE5W4zLIEUxww8o+gRn4gUoOmEx/TEanyrwJV/J4wbJX/Pl1zExl08YFIqwbjEQVsCJjVlZsbh+fPpR57JL0uN2YzL/Ojgm07NyZxCNDjgCh7X4NcGSAjpoKb1E8gEhWyEpjTJXoQKBgQCArgZm7gyljk79NQ8hwv1SFFAol3aS7Bp8LB+z89hTyk5+ld28OSeB/L0Sx8DyL1DjHnLjAi9u4QDg+qFBolAJgM+FMu+MML17GWy7lvnP07Z/1QV85bwa79LGyNsm6wYDlqGHffP1X7Ne1TfjmjYbGP6/q1BsA/YeqQh66fnu8Q==";
    const TEST_JWKS: &str = r#"{"keys": [{"kty": "RSA", "kid": "test-key", "alg": "RS256", "use": "sig", "n": "6JF-SUNx74BZkMWW5XO1CaK9wJtqboMmBrAtC5_3yvLi2RfIPiZo0JrxuiX8DOGZYfnKSADCo51iSk_M3nMq6Py_gdKnChVL1e8otsBlQmVRE4IdeMh9paY1vDcnPjVBn917Aoh0DM3Ifg3Jqw5VQ7kJNNtFNuH6ROjo1rWHZXOIpBRgnfIGj4EhVcP736m_Ht_lwKy_3K9bBd5CB3TUV7ZQLX3n0FYUF1x7iJYqeJwO1-WkcznxbkCBeHMM83Vt6mkrGsxZhEC7dI96l2-2wg0ANT_YzIbA3mFQRWf8y0XKyT4Uq8I4shwFW9uB5QLUYhijIMmbyG3bFk7KifMPxw", "e": "AQAB"}]}"#;
}
