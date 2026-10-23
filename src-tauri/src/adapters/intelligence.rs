use super::chatgpt::{http_client, ChatGptAuth, RESOURCE};
use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Cursor, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};
use tuck_core::{
    ports::{IntelligencePort, Result, SecretPort},
    AppError,
};
use url::{Host, Url};

pub struct Intelligence {
    auth: Arc<ChatGptAuth>,
    secrets: Arc<dyn SecretPort>,
    client: Client,
    inference: InferenceGate,
}
// One model request at a time. A background check never queues behind manual work.
#[derive(Default)]
struct InferenceGate {
    active: Mutex<()>,
    manual_waiting: AtomicUsize,
}
impl InferenceGate {
    fn enter(&self, coach: bool) -> Result<MutexGuard<'_, ()>> {
        if coach {
            let busy = || {
                AppError::new(
                    "intelligence_busy",
                    "Waiting for your AI request to finish.",
                )
            };
            if self.manual_waiting.load(Ordering::SeqCst) != 0 {
                return Err(busy());
            }
            let guard = self.active.try_lock().map_err(|_| busy())?;
            if self.manual_waiting.load(Ordering::SeqCst) != 0 {
                return Err(busy());
            }
            Ok(guard)
        } else {
            self.manual_waiting.fetch_add(1, Ordering::SeqCst);
            let guard = self.active.lock();
            self.manual_waiting.fetch_sub(1, Ordering::SeqCst);
            guard.map_err(|_| {
                error("The AI request worker is unavailable. Restart Tuck to try again.")
            })
        }
    }
}
const COACH_INPUT_BYTES: usize = 12_000;
const COACH_RESPONSE_BYTES: u64 = 65_536;
const COACH_OUTPUT_CHARS: usize = 3_000;
const COACH_TIMEOUT: Duration = Duration::from_secs(20);
fn coach_response(response: Response, local: bool) -> Result<String> {
    let mut bytes = Vec::new();
    checked(response)?
        .take(COACH_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            error(
                "The agent check was interrupted. Recording continues; Tuck will try again later.",
            )
        })?;
    if bytes.len() as u64 > COACH_RESPONSE_BYTES {
        return Err(error(
            "The agent provider returned too much data. Try a smaller model response.",
        ));
    }
    let answer = if local {
        consume_local(Cursor::new(bytes), &mut |_| {})?
    } else {
        consume_sse(Cursor::new(bytes), &mut |_| {})?
    };
    if answer.chars().count() > COACH_OUTPUT_CHARS {
        return Err(error(
            "The agent response was too long. Tuck will check again later.",
        ));
    }
    Ok(answer)
}
fn hosted_body(
    model: &str,
    system: &str,
    messages: &[Value],
    web_search: bool,
    coach: bool,
) -> Value {
    let mut body =
        json!({"model":model,"instructions":system,"input":messages,"store":false,"stream":true});
    if coach {
        body["max_output_tokens"] = json!(1200);
    } else if web_search {
        body["tools"] = json!([{"type":"web_search"}]);
    }
    body
}
fn error(message: &str) -> AppError {
    AppError::new("intelligence_error", message)
}
pub fn local_endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| {
        error("Local AI must use a loopback address such as http://127.0.0.1:11434.")
    })?;
    let local = match url.host() {
        Some(Host::Domain(name)) => name == "localhost",
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !local
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(error(
            "Local AI must use a loopback address such as http://127.0.0.1:11434.",
        ));
    }
    Ok(url)
}
fn checked(response: Response) -> Result<Response> {
    match response.status().as_u16(){401|403=>Err(error("Your AI provider denied this request. Check your account, model, and permissions in Settings.")),429=>Err(error("Your AI usage limit was reached. Manage usage in Settings or switch to local AI.")),_=>response.error_for_status().map_err(|_|error("The AI provider rejected this request. Check your selected model and enabled web lookup."))}
}
fn failure(code: &str) -> AppError {
    error(match code {
        "subscription_sharing_usage_limit_exceeded"
        | "rate_limit_exceeded"
        | "insufficient_quota" => {
            "Your AI usage limit was reached. Manage usage in Settings or switch to local AI."
        }
        "subscription_sharing_usage_unavailable" => {
            "ChatGPT plan usage is unavailable. Check your account permissions in Settings."
        }
        _ => "The AI provider could not finish this answer. Your transcript is saved; try again.",
    })
}

const MAX_ANSWER_CHARS: usize = 150_000;
const MAX_EVENT_BYTES: usize = 700_000;
fn append(
    parts: &mut String,
    size: &mut usize,
    delta: &str,
    on_delta: &mut dyn FnMut(&str),
) -> Result<()> {
    *size += delta.chars().count();
    if *size > MAX_ANSWER_CHARS {
        return Err(error(
            "The answer exceeded Tuck's size limit. Ask a narrower question.",
        ));
    }
    parts.push_str(delta);
    on_delta(delta);
    Ok(())
}
// Bound allocations even when a peer sends a giant line without a newline.
fn next_line(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let buffer = reader.fill_buf().map_err(|_| {
            error("The AI connection was interrupted. Your transcript is saved; try again.")
        })?;
        if buffer.is_empty() {
            break;
        }
        let count = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buffer.len(), |n| n + 1);
        if bytes.len() + count > MAX_EVENT_BYTES {
            return Err(error("The AI provider returned an oversized stream event."));
        }
        let ended = buffer[count - 1] == b'\n';
        bytes.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if ended {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    let line = String::from_utf8(bytes)
        .map_err(|_| error("The AI provider returned an unreadable stream."))?;
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_owned()))
}
/// Translates complete SSE events; incomplete transport never becomes a saved answer.
pub fn consume_sse(mut reader: impl BufRead, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
    let mut data = String::new();
    let mut answer = String::new();
    let mut size = 0;
    let mut completed = false;
    let mut sources = Vec::<(String, String)>::new();
    let mut event = |data: &str| -> Result<()> {
        if data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(());
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|_| error("The AI provider returned an unreadable stream."))?;
        let kind = event["type"]
            .as_str()
            .ok_or_else(|| error("The AI provider returned an invalid stream event."))?;
        if !event.is_object() || completed {
            return Err(error("The AI provider returned an invalid stream event."));
        }
        match kind {
            "response.output_text.delta" => append(
                &mut answer,
                &mut size,
                event["delta"]
                    .as_str()
                    .ok_or_else(|| error("The AI provider returned invalid answer text."))?,
                on_delta,
            )?,
            "response.output_text.annotation.added" => {
                let a = &event["annotation"];
                if a["type"] == "url_citation" {
                    if let Some(raw) = a["url"].as_str() {
                        if let Ok(url) = Url::parse(raw) {
                            if url.scheme() == "https"
                                && url.host_str().is_some()
                                && url.username().is_empty()
                                && url.password().is_none()
                            {
                                let title = a["title"]
                                    .as_str()
                                    .unwrap_or("Source")
                                    .chars()
                                    .take(1000)
                                    .collect::<String>()
                                    .replace(['[', ']', '\r', '\n'], "");
                                let pair =
                                    (title, url.as_str().replace(')', "%29").replace('(', "%28"));
                                if !sources.contains(&pair) {
                                    if sources.len() >= 100 {
                                        return Err(error(
                                            "The AI provider returned too many citations.",
                                        ));
                                    }
                                    sources.push(pair);
                                }
                            }
                        }
                    }
                }
            }
            "response.completed" => {
                if let Some(response) = event.get("response") {
                    if !response.is_object()
                        || response["status"] != "completed"
                        || !response["error"].is_null()
                    {
                        return Err(failure(
                            response["error"]["code"].as_str().unwrap_or("unknown"),
                        ));
                    }
                }
                completed = true;
            }
            "response.failed" | "response.incomplete" | "error" => {
                return Err(failure(
                    event["error"]["code"]
                        .as_str()
                        .or_else(|| event["response"]["error"]["code"].as_str())
                        .unwrap_or("unknown"),
                ))
            }
            _ => {}
        }
        Ok(())
    };
    while let Some(line) = next_line(&mut reader)? {
        if line.is_empty() {
            event(&data)?;
            data.clear();
        } else if let Some(part) = line.strip_prefix("data:") {
            if data.len() + part.len() + 1 > MAX_EVENT_BYTES {
                return Err(error("The AI provider returned an oversized stream event."));
            }
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(part.trim_start());
        }
    }
    if !data.is_empty() {
        event(&data)?;
    }
    drop(event);
    if !completed {
        return Err(error("The AI connection ended before the answer completed. Your transcript is saved; try again."));
    }
    if answer.trim().is_empty() {
        return Err(error(
            "The model completed without an answer. Choose a text model and try again.",
        ));
    }
    if !sources.is_empty() {
        let links = sources
            .iter()
            .map(|(name, url)| format!("[{name}]({url})"))
            .collect::<Vec<_>>()
            .join(" · ");
        append(
            &mut answer,
            &mut size,
            &format!("\n\nSources: {links}"),
            on_delta,
        )?;
    }
    Ok(answer.trim().to_owned())
}
pub fn consume_local(mut reader: impl BufRead, on_delta: &mut dyn FnMut(&str)) -> Result<String> {
    let mut answer = String::new();
    let mut size = 0;
    let mut complete = false;
    while let Some(line) = next_line(&mut reader)? {
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(&line)
            .map_err(|_| error("Local AI returned an unreadable answer."))?;
        if !event.is_object() || complete {
            return Err(error("Local AI returned an invalid stream event."));
        }
        if event.get("error").is_some() {
            return Err(error("Local AI could not finish this answer. Check that Ollama has enough available memory."));
        }
        if let Some(content) = event.get("message").and_then(|m| m.get("content")) {
            append(
                &mut answer,
                &mut size,
                content
                    .as_str()
                    .ok_or_else(|| error("Local AI returned invalid answer text."))?,
                on_delta,
            )?;
        }
        if event.get("done").is_some_and(|v| !v.is_boolean()) {
            return Err(error("Local AI returned an invalid stream event."));
        }
        if event["done"] == true {
            if event["done_reason"] == "length" {
                return Err(error(
                    "Local AI reached its context or answer limit. Ask a narrower question or choose another model.",
                ));
            }
            complete = true;
        }
    }
    if !complete || answer.trim().is_empty() {
        return Err(error(
            "Local AI stopped before completing an answer. Try again.",
        ));
    }
    Ok(answer.trim().to_owned())
}

impl Intelligence {
    pub fn new(auth: Arc<ChatGptAuth>, secrets: Arc<dyn SecretPort>) -> Result<Self> {
        Ok(Self {
            auth,
            secrets,
            client: http_client(Duration::from_secs(180))?,
            inference: InferenceGate::default(),
        })
    }
}
impl IntelligencePort for Intelligence {
    fn models(&self, settings: &Value) -> Result<Vec<Value>> {
        let provider = settings["provider"].as_str().unwrap_or("local");
        if provider == "chatgpt" {
            return self.auth.models();
        }
        let response = if provider == "local" {
            let endpoint = local_endpoint(
                settings["localUrl"]
                    .as_str()
                    .unwrap_or("http://127.0.0.1:11434"),
            )?;
            self.client
                .get(
                    endpoint
                        .join("api/tags")
                        .map_err(|_| error("Invalid local AI endpoint."))?,
                )
                .send()
        } else if provider == "openai" {
            let key = self
                .secrets
                .get("openai")?
                .ok_or_else(|| error("Add your OpenAI API key in Settings first."))?;
            self.client
                .get(format!("{RESOURCE}/models"))
                .bearer_auth(key)
                .send()
        } else {
            return Err(error("Choose a supported AI provider in Settings."));
        };
        let raw: Value = checked(response.map_err(|_| {
            error(if provider == "local" {
                "Could not reach Ollama. Start it, install a chat model, then load models."
            } else {
                "Could not load AI models. Check your connection and credentials."
            })
        })?)?
        .json()
        .map_err(|_| error("The AI provider returned an unreadable model list."))?;
        let values = raw[if provider == "local" {
            "models"
        } else {
            "data"
        }]
        .as_array()
        .ok_or_else(|| error("The AI provider returned an unreadable model list."))?;
        Ok(values
            .iter()
            .filter_map(|m| {
                let id = m[if provider == "local" { "name" } else { "id" }].as_str()?;
                if provider == "openai"
                    && (!["gpt-", "o1", "o3", "o4"].iter().any(|p| id.starts_with(p))
                        || [
                            "audio",
                            "realtime",
                            "transcribe",
                            "tts",
                            "image",
                            "search-preview",
                        ]
                        .iter()
                        .any(|p| id.contains(p)))
                {
                    return None;
                }
                Some(json!({"id":id,"name":id}))
            })
            .collect())
    }
    fn complete(
        &self,
        settings: &Value,
        system: &str,
        messages: &[Value],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        self.complete_with_policy(settings, system, messages, on_delta, false)
    }
    fn complete_coach(&self, settings: &Value, system: &str, messages: &[Value]) -> Result<String> {
        self.complete_with_policy(settings, system, messages, &mut |_| {}, true)
    }
}
impl Intelligence {
    fn complete_with_policy(
        &self,
        settings: &Value,
        system: &str,
        messages: &[Value],
        on_delta: &mut dyn FnMut(&str),
        coach: bool,
    ) -> Result<String> {
        let _request = self.inference.enter(coach)?;
        if coach
            && system.len()
                + serde_json::to_vec(messages)
                    .map_err(|_| error("Invalid agent context."))?
                    .len()
                > COACH_INPUT_BYTES
        {
            return Err(error("The selected agent instructions are too long. Shorten them or choose fewer agents."));
        }
        let provider = settings["provider"].as_str().unwrap_or("local");
        let model = settings["model"].as_str().unwrap_or("").trim();
        if model.is_empty() {
            return Err(error("Choose an AI model in Settings first."));
        }
        let messages = messages
            .iter()
            .filter(|m| {
                matches!(m["role"].as_str(), Some("user" | "assistant"))
                    && m["content"].as_str().is_some_and(|s| !s.is_empty())
            })
            .map(|m| json!({"role":m["role"],"content":m["content"]}))
            .collect::<Vec<_>>();
        if messages.is_empty() {
            return Err(error("Enter a question first."));
        }
        if provider == "local" {
            if !coach && settings["webSearch"] == true {
                return Err(error(
                    "Web lookup needs a hosted AI provider. Turn it off to use local AI.",
                ));
            }
            let endpoint = local_endpoint(
                settings["localUrl"]
                    .as_str()
                    .unwrap_or("http://127.0.0.1:11434"),
            )?;
            let mut history = vec![json!({"role":"system","content":system})];
            history.extend(messages);
            // Leave CPU headroom for speech processing and an active video call.
            // Model size/GPU memory remain the user's selected model's responsibility.
            let body = json!({"model":model,"messages":history,"stream":true,"think":false,"keep_alive":"2m","options":{"num_ctx":16384,"num_thread":2,"num_predict":if coach {1200} else {4096}}});
            let response = self
                .client
                .post(
                    endpoint
                        .join("api/chat")
                        .map_err(|_| error("Invalid local AI endpoint."))?,
                )
                .timeout(if coach {
                    COACH_TIMEOUT
                } else {
                    Duration::from_secs(180)
                })
                .json(&body)
                .send()
                .map_err(|_| {
                    error("Could not reach local AI. Start Ollama and check the model in Settings.")
                })?;
            if response.status().as_u16() == 404 {
                return Err(error("This local model is not installed. Install it in Ollama, then load models in Settings."));
            }
            return if coach {
                coach_response(response, true)
            } else {
                consume_local(BufReader::new(checked(response)?), on_delta)
            };
        }
        if !matches!(provider, "chatgpt" | "openai") {
            return Err(error("Choose a supported AI provider in Settings."));
        }
        if settings["hostedConsent"] != true {
            return Err(error(
                "Allow hosted transcript processing in Settings before using this provider.",
            ));
        }
        let token = if provider == "chatgpt" {
            self.auth.access_token()?
        } else {
            self.secrets
                .get("openai")?
                .ok_or_else(|| error("Add your OpenAI API key in Settings first."))?
        };
        let body = hosted_body(
            model,
            system,
            &messages,
            !coach && settings["webSearch"] == true,
            coach,
        );
        let response = self
            .client
            .post(format!("{RESOURCE}/responses"))
            .bearer_auth(token)
            .timeout(if coach {
                COACH_TIMEOUT
            } else {
                Duration::from_secs(180)
            })
            .json(&body)
            .send()
            .map_err(|_| {
                error("The AI connection was interrupted. Your transcript is saved; try again.")
            })?;
        if coach {
            coach_response(response, false)
        } else {
            consume_sse(BufReader::new(checked(response)?), on_delta)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn loopback_only() {
        for url in [
            "http://127.0.0.1:11434",
            "http://localhost:11434",
            "http://[::1]:11434",
        ] {
            assert!(local_endpoint(url).is_ok(), "{url}");
        }
        for url in [
            "https://example.com",
            "http://127.0.0.1.evil.test",
            "http://user:secret@127.0.0.1",
            "file:///tmp/a",
            "http://localhost/path",
            "http://localhost?key=x",
        ] {
            assert!(local_endpoint(url).is_err(), "{url}");
        }
    }
    #[test]
    fn stream_must_complete() {
        let partial = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\n";
        assert!(consume_sse(Cursor::new(partial), &mut |_| {}).is_err());
        let whole = format!("{partial}data: {{\"type\":\"response.completed\"}}\n\n");
        assert_eq!(
            consume_sse(Cursor::new(whole), &mut |_| {}).unwrap(),
            "Partial"
        );
    }
    #[test]
    fn late_usage_failure_preserves_no_completed_answer() {
        let raw="data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}\n\n";
        let err = consume_sse(Cursor::new(raw), &mut |_| {}).unwrap_err();
        assert!(err.message.contains("usage limit"));
    }
    #[test]
    fn local_requires_done() {
        assert!(consume_local(
            Cursor::new("{\"message\":{\"content\":\"Answer\"}}\n"),
            &mut |_| {}
        )
        .is_err());
        assert_eq!(
            consume_local(
                Cursor::new("{\"message\":{\"content\":\"Answer\"}}\n{\"done\":true}\n"),
                &mut |_| {}
            )
            .unwrap(),
            "Answer"
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use std::io::Cursor;
    fn hosted(events: &[Value]) -> String {
        events
            .iter()
            .map(|v| format!("data: {v}\r\n\r\n"))
            .collect()
    }
    #[test]
    fn malformed_events_never_become_an_answer() {
        for bad in [
            json!([]),
            json!(null),
            json!({}),
            json!({"type":"response.output_text.delta","delta":17}),
            json!({"type":"response.completed","response":{"status":"incomplete"}}),
        ] {
            let raw = hosted(&[
                json!({"type":"response.output_text.delta","delta":"Partial"}),
                bad,
                json!({"type":"response.completed"}),
            ]);
            assert!(consume_sse(Cursor::new(raw), &mut |_| {}).is_err());
        }
    }
    #[test]
    fn trailing_delta_or_failure_after_completion_is_rejected() {
        for trailing in [
            json!({"type":"response.output_text.delta","delta":"Late"}),
            json!({"type":"response.failed"}),
        ] {
            let raw = hosted(&[
                json!({"type":"response.output_text.delta","delta":"Answer"}),
                json!({"type":"response.completed","response":{"status":"completed","error":null}}),
                trailing,
            ]);
            assert!(consume_sse(Cursor::new(raw), &mut |_| {}).is_err());
        }
    }
    #[test]
    fn multiline_sse_and_safe_citation_links() {
        let raw="data: {\"type\":\"response.output_text.delta\",\ndata: \"delta\":\"Answer\"}\n\ndata: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"url\":\"https://example.com/a(b)\",\"title\":\"[Source]\\nTitle\"}}\n\ndata: {\"type\":\"response.completed\"}\n\ndata: [DONE]\n\n";
        let answer = consume_sse(Cursor::new(raw), &mut |_| {}).unwrap();
        assert_eq!(
            answer,
            "Answer\n\nSources: [SourceTitle](https://example.com/a%28b%29)"
        );
    }
    #[test]
    fn stream_allocation_and_character_limits() {
        assert!(consume_sse(Cursor::new("x".repeat(MAX_EVENT_BYTES + 1)), &mut |_| {}).is_err());
        let raw = hosted(&[
            json!({"type":"response.output_text.delta","delta":"a".repeat(MAX_ANSWER_CHARS+1)}),
            json!({"type":"response.completed"}),
        ]);
        assert!(consume_sse(Cursor::new(raw), &mut |_| {}).is_err());
        let raw = hosted(&[
            json!({"type":"response.output_text.delta","delta":"界".repeat(MAX_ANSWER_CHARS)}),
            json!({"type":"response.completed"}),
        ]);
        assert_eq!(
            consume_sse(Cursor::new(raw), &mut |_| {})
                .unwrap()
                .chars()
                .count(),
            MAX_ANSWER_CHARS
        );
    }
    #[test]
    fn local_rejects_malformed_payload_and_context_exhaustion() {
        for bad in [
            json!([]),
            json!({"message":{"content":false}}),
            json!({"done":"true"}),
            json!({"done":true,"done_reason":"length"}),
        ] {
            let raw = format!(
                "{}\n{}\n{}\n",
                json!({"message":{"content":"Partial"}}),
                bad,
                json!({"done":true})
            );
            assert!(consume_local(Cursor::new(raw), &mut |_| {}).is_err());
        }
    }
    #[test]
    fn local_request_reserves_cpu_headroom_and_bounds_generation() {
        struct NoHostedSecrets;
        impl SecretPort for NoHostedSecrets {
            fn get(&self, _: &str) -> Result<Option<String>> {
                panic!("Local AI must not read hosted credentials")
            }
            fn set(&self, _: &str, _: &str) -> Result<()> {
                unreachable!()
            }
            fn delete(&self, _: &str) -> Result<()> {
                unreachable!()
            }
        }
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr().to_ip().unwrap());
        let worker = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "/api/chat");
            assert!(!request
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization")));
            let mut raw = String::new();
            request.as_reader().read_to_string(&mut raw).unwrap();
            let payload: Value = serde_json::from_str(&raw).unwrap();
            request.respond(tiny_http::Response::from_string("{\"message\":{\"content\":\"Answer\"},\"done\":false}\n{\"done\":true,\"done_reason\":\"stop\"}\n")).unwrap();
            payload
        });
        let data = tempfile::tempdir().unwrap();
        let secrets: Arc<dyn SecretPort> = Arc::new(NoHostedSecrets);
        let intelligence = Intelligence::new(
            Arc::new(ChatGptAuth::new(data.path(), secrets.clone()).unwrap()),
            secrets,
        )
        .unwrap();
        assert_eq!(
            intelligence
                .complete(
                    &json!({"provider":"local","model":"test-local","localUrl":endpoint}),
                    "Context",
                    &[json!({"role":"user","content":"Question"})],
                    &mut |_| {}
                )
                .unwrap(),
            "Answer"
        );
        let payload = worker.join().unwrap();
        assert_eq!(payload["options"]["num_thread"], 2);
        assert_eq!(payload["options"]["num_predict"], 4096);
        assert_eq!(payload["options"]["num_ctx"], 16384);
        assert_eq!(payload["think"], false);
        assert_eq!(payload["keep_alive"], "2m");
    }
}

#[cfg(test)]
mod coach_tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Instant};

    struct NoSecrets;
    impl SecretPort for NoSecrets {
        fn get(&self, _: &str) -> Result<Option<String>> {
            panic!("Local/background budget rejection must not read hosted secrets")
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        fn delete(&self, _: &str) -> Result<()> {
            unreachable!()
        }
    }
    fn adapter(path: &std::path::Path) -> Intelligence {
        let secrets: Arc<dyn SecretPort> = Arc::new(NoSecrets);
        Intelligence::new(
            Arc::new(ChatGptAuth::new(path, secrets.clone()).unwrap()),
            secrets,
        )
        .unwrap()
    }
    #[test]
    fn background_cannot_enable_tools_or_provider_storage() {
        let body = hosted_body(
            "selected-model",
            "Evidence policy",
            &[json!({"role":"user","content":"Recent speech"})],
            true,
            true,
        );
        assert_eq!(body["model"], "selected-model");
        assert_eq!(body["store"], false);
        assert_eq!(body["max_output_tokens"], 1200);
        assert!(body.get("tools").is_none());
        let manual = hosted_body("selected-model", "Context", &[], true, false);
        assert_eq!(manual["tools"][0]["type"], "web_search");
        assert!(manual.get("max_output_tokens").is_none());
    }
    #[test]
    fn background_skips_when_manual_is_active_or_waiting_and_never_overlaps() {
        let gate = Arc::new(InferenceGate::default());
        let held = gate.enter(true).unwrap();
        assert_eq!(gate.enter(true).unwrap_err().code, "intelligence_busy");
        let (send, recv) = mpsc::channel();
        let other = gate.clone();
        let waiter = thread::spawn(move || {
            let _guard = other.enter(false).unwrap();
            send.send(()).unwrap();
        });
        let until = Instant::now() + Duration::from_secs(2);
        while gate.manual_waiting.load(Ordering::SeqCst) == 0 && Instant::now() < until {
            thread::yield_now();
        }
        assert_eq!(gate.manual_waiting.load(Ordering::SeqCst), 1);
        assert_eq!(gate.enter(true).unwrap_err().code, "intelligence_busy");
        drop(held);
        recv.recv_timeout(Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
        let manual = gate.enter(false).unwrap();
        assert_eq!(gate.enter(true).unwrap_err().code, "intelligence_busy");
        drop(manual);
        assert!(gate.enter(true).is_ok());
    }
    #[test]
    fn encoded_unicode_budget_rejects_before_credentials_or_network() {
        let data = tempfile::tempdir().unwrap();
        let adapter = adapter(data.path());
        let result = adapter.complete_coach(
            &json!({"provider":"openai","model":"selected","hostedConsent":true}),
            "Policy",
            &[json!({"role":"user","content":"語".repeat(5000)})],
        );
        assert!(result.unwrap_err().message.contains("too long"));
    }
    #[test]
    fn local_background_has_smaller_output_budget_and_complete_bounded_transport() {
        let cases = vec![
            (
                "{\"message\":{\"content\":\"null\"},\"done\":true}\n".to_owned(),
                true,
            ),
            (
                "{\"message\":{\"content\":\"null\"},\"done\":false}\n".to_owned(),
                false,
            ),
            (
                format!(
                    "{{\"message\":{{\"content\":\"{}\"}},\"done\":true}}\n",
                    "x".repeat(3001)
                ),
                false,
            ),
            (" ".repeat(COACH_RESPONSE_BYTES as usize + 1), false),
        ];
        for (response, success) in cases {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", server.server_addr().to_ip().unwrap());
            let worker = thread::spawn(move || {
                let mut request = server
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                assert_eq!(request.url(), "/api/chat");
                let mut raw = String::new();
                request.as_reader().read_to_string(&mut raw).unwrap();
                let payload: Value = serde_json::from_str(&raw).unwrap();
                request
                    .respond(tiny_http::Response::from_string(response))
                    .unwrap();
                payload
            });
            let data = tempfile::tempdir().unwrap();
            let result = adapter(data.path()).complete_coach(
                &json!({"provider":"local","model":"fixture","localUrl":endpoint,"webSearch":true}),
                "Policy",
                &[json!({"role":"user","content":"Recent speech"})],
            );
            assert_eq!(result.is_ok(), success, "{result:?}");
            if success {
                assert_eq!(result.unwrap(), "null");
            }
            let body = worker.join().unwrap();
            assert_eq!(body["options"]["num_predict"], 1200);
            assert_eq!(body["options"]["num_thread"], 2);
            assert_eq!(body["options"]["num_ctx"], 16384);
            assert_eq!(body["think"], false);
            assert!(body.get("tools").is_none());
        }
    }
}
