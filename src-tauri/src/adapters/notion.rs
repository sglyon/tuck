use super::chatgpt::http_client;
use reqwest::{blocking::Client, Method};
use serde_json::{json, Map, Value};
use std::{io::Read, sync::Arc, time::Duration};
use tuck_core::{
    ports::{PublicationPort, Result, SecretPort},
    AppError,
};
use url::Url;
use uuid::Uuid;

const API_VERSION: &str = "2026-03-11";
const MANAGED_TITLE: &str = "Meeting notes · Tuck";
const MAX_RESPONSE: u64 = 2_000_000;

pub struct NotionPublisher {
    secrets: Arc<dyn SecretPort>,
    client: Client,
    base: String,
}
fn error(message: &str) -> AppError {
    AppError::new("publish_error", message)
}
fn uncertain(message: &str) -> AppError {
    AppError::new("publish_uncertain", message)
}
pub fn target_id(target: &str) -> Result<String> {
    let raw = target.trim();
    let candidate = if raw.starts_with("https://") || raw.starts_with("http://") {
        let url =
            Url::parse(raw).map_err(|_| error("Use a Notion database URL or data source ID."))?;
        if !notion_url(&url) {
            return Err(error("Use a Notion database URL or data source ID."));
        }
        let segment = url
            .path()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("");
        [36, 32]
            .iter()
            .find_map(|n| {
                segment
                    .get(segment.len().checked_sub(*n)?..)
                    .filter(|s| Uuid::parse_str(s).is_ok())
            })
            .ok_or_else(|| error("Copy the full Notion link, including its ID."))?
            .to_owned()
    } else {
        raw.to_owned()
    };
    Uuid::parse_str(&candidate)
        .map(|id| id.to_string())
        .map_err(|_| error("Enter a valid Notion database URL or data source ID."))
}
fn notion_url(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (host == "app.notion.com"
            || host == "notion.so"
            || host.ends_with(".notion.so")
            || host == "notion.site"
            || host.ends_with(".notion.site"))
}
fn rich(text: &str) -> Vec<Value> {
    text.chars()
        .collect::<Vec<_>>()
        .chunks(1900)
        .map(|c| json!({"type":"text","text":{"content":c.iter().collect::<String>()}}))
        .collect()
}
fn plain(rich: &Value) -> String {
    rich.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            r["text"]["content"]
                .as_str()
                .or_else(|| r["plain_text"].as_str())
        })
        .collect::<Vec<_>>()
        .join("")
}
fn title(meeting: &Value) -> String {
    meeting["title"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("Untitled meeting")
        .chars()
        .take(1900)
        .collect()
}
fn marker(meeting: &Value) -> String {
    format!("Tuck meeting: {}", meeting["id"].as_str().unwrap_or(""))
}
fn note_blocks(meeting: &Value) -> Result<Vec<Value>> {
    let mut content = format!(
        "# Meeting notes\n{}",
        meeting["summary"].as_str().unwrap_or("")
    );
    if let Some(notes) = meeting["notes"].as_str().filter(|s| !s.is_empty()) {
        content.push_str(&format!("\n\n## Personal notes\n{notes}"));
    }
    if let Some(tasks) = meeting["tasks"].as_array().filter(|a| !a.is_empty()) {
        content.push_str("\n\n## Actions");
        for task in tasks {
            let owner = task["owner"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            let date = task["dueDate"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" · due {s}"))
                .unwrap_or_default();
            content.push_str(&format!(
                "\n- [{}] {}{owner}{date}",
                if task["done"] == true { "x" } else { " " },
                task["title"].as_str().unwrap_or("")
            ));
        }
    }
    if content.len() > 350_000 {
        return Err(error("These notes exceed Notion's request limit. Shorten the notes or export Markdown; nothing was uploaded."));
    }
    let mut blocks = Vec::new();
    let mut in_code = false;
    for line in content.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut kind = "paragraph";
        let mut text = line;
        let mut checked = None;
        if !in_code {
            for (prefix, name) in [
                ("### ", "heading_3"),
                ("## ", "heading_2"),
                ("# ", "heading_1"),
            ] {
                if let Some(rest) = line.strip_prefix(prefix) {
                    kind = name;
                    text = rest;
                    break;
                }
            }
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed
                .strip_prefix("- [ ] ")
                .or_else(|| trimmed.strip_prefix("* [ ] "))
            {
                kind = "to_do";
                text = rest;
                checked = Some(false);
            } else if let Some(rest) = trimmed
                .strip_prefix("- [x] ")
                .or_else(|| trimmed.strip_prefix("- [X] "))
                .or_else(|| trimmed.strip_prefix("* [x] "))
                .or_else(|| trimmed.strip_prefix("* [X] "))
            {
                kind = "to_do";
                text = rest;
                checked = Some(true);
            } else if let Some(rest) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                kind = "bulleted_list_item";
                text = rest;
            }
        }
        for chunk in text.chars().collect::<Vec<_>>().chunks(20_000) {
            let mut value = json!({"rich_text":rich(&chunk.iter().collect::<String>())});
            if let Some(checked) = checked {
                value["checked"] = json!(checked);
            }
            let mut block = json!({"object":"block","type":kind});
            block[kind] = value;
            blocks.push(block);
        }
    }
    blocks.push(json!({"object":"block","type":"paragraph","paragraph":{"rich_text":rich(&marker(meeting)),"color":"gray"}}));
    // The enclosing toggle counts toward the total block limit too.
    if blocks.len() > 99 {
        return Err(error("These notes exceed Notion's request limit. Shorten the notes or export Markdown; nothing was uploaded."));
    }
    Ok(blocks)
}
fn managed_section(meeting: &Value) -> Result<Value> {
    Ok(
        json!({"object":"block","type":"toggle","toggle":{"rich_text":rich(MANAGED_TITLE),"children":note_blocks(meeting)?}}),
    )
}
fn within_limit(payload: Value) -> Result<Value> {
    if payload.to_string().len() > 500_000 {
        Err(error("These notes exceed Notion's request limit. Shorten the notes or export Markdown; nothing was uploaded."))
    } else {
        Ok(payload)
    }
}
pub fn page_payload(source: &str, title_property: &str, meeting: &Value) -> Result<Value> {
    within_limit(
        json!({"parent":{"type":"data_source_id","data_source_id":source},
        "properties":{title_property:{"type":"title","title":rich(&title(meeting))}},
        "children":[managed_section(meeting)?]}),
    )
}

/// Only explicitly mapped columns are touched. Notion user IDs are never inferred from names.
fn mapped_properties(
    source: &Value,
    mapping: &Value,
    meeting: &Value,
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    let Some(mapping) = mapping.as_object() else {
        return Ok(out);
    };
    for (semantic, name) in mapping {
        let name = name
            .as_str()
            .ok_or_else(|| error("Notion property mappings must be column names."))?;
        if name.is_empty() {
            continue;
        }
        if out.contains_key(name) {
            return Err(error(
                "Choose a different Notion column for each mapped field.",
            ));
        }
        let kind = source["properties"][name]["type"].as_str().unwrap_or("");
        let value = match (semantic.as_str(), kind) {
            ("date", "date") => {
                let date = meeting["calendarEvent"]["start"].as_str().or_else(|| meeting["createdAt"].as_str()).unwrap_or("");
                chrono::DateTime::parse_from_rfc3339(date).map_err(|_| error("This meeting has no valid date to send to Notion."))?;
                json!({"date":{"start":date}})
            }
            ("folder", "select") => {
                let folder = meeting["folder"].as_str().unwrap_or("");
                if folder.is_empty() { json!({"select":null}) } else { json!({"select":{"name":folder}}) }
            }
            ("folder", "rich_text") => json!({"rich_text":rich(meeting["folder"].as_str().unwrap_or(""))}),
            ("meetingId", "rich_text") => json!({"rich_text":rich(meeting["id"].as_str().unwrap_or(""))}),
            ("tags" | "people", "rich_text" | "multi_select") => {
                let values = meeting[semantic].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>();
                if values.len() > 100 { return Err(error("Notion allows at most 100 values in this column.")); }
                if kind == "multi_select" {
                    if values.iter().any(|s| s.contains(',') || s.chars().count() > 100) {
                        return Err(error("Notion tags and people labels must be at most 100 characters and contain no commas. Use a text column instead."));
                    }
                    json!({"multi_select":values.iter().map(|name|json!({"name":name})).collect::<Vec<_>>()})
                } else { json!({"rich_text":rich(&values.join(", "))}) }
            }
            _ => return Err(error("A mapped Notion column is missing or has an incompatible type. Review the destination in Settings.")),
        };
        out.insert(name.into(), value);
    }
    Ok(out)
}
fn same_blocks(actual: &[Value], expected: &[Value]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, b)| {
            let kind = b["type"].as_str().unwrap_or("");
            a["type"] == kind
                && plain(&a[kind]["rich_text"]) == plain(&b[kind]["rich_text"])
                && (kind != "to_do" || a[kind]["checked"] == b[kind]["checked"])
        })
}

impl NotionPublisher {
    pub fn new(secrets: Arc<dyn SecretPort>) -> Result<Self> {
        Ok(Self {
            secrets,
            client: http_client(Duration::from_secs(35))?,
            base: "https://api.notion.com/v1".into(),
        })
    }
    fn token(&self) -> Result<String> {
        self.secrets
            .get("notion")?
            .ok_or_else(|| error("Connect Notion in Settings first."))
    }
    fn send(
        &self,
        token: &str,
        method: Method,
        path: &str,
        payload: Option<&Value>,
        writing: bool,
    ) -> Result<Value> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .header("Notion-Version", API_VERSION);
        if let Some(payload) = payload {
            request = request.json(payload);
        }
        let response = request.send().map_err(|_| if writing {
            uncertain("The connection ended while updating Notion. Your notes are saved locally. Check Notion before retrying a new upload; an existing page can be safely reconciled.")
        } else { error("Could not reach Notion. Your notes are saved locally.") })?;
        let status = response.status().as_u16();
        if writing {
            creation_status(status)?;
        } else if !(200..=299).contains(&status) {
            return Err(read_status(status));
        }
        let mut bytes = Vec::new();
        response.take(MAX_RESPONSE + 1).read_to_end(&mut bytes).map_err(|_| if writing { uncertain("Notion may have received this update. Check the page, then retry updating the same page.") } else { error("Notion returned an unreadable response.") })?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err(if writing {
                uncertain("Notion returned an oversized response after this update.")
            } else {
                error("Notion returned an oversized response.")
            });
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            if writing {
                uncertain(
                    "Notion may have received this update but returned an unreadable response.",
                )
            } else {
                error("Notion returned an unreadable response.")
            }
        })
    }
    fn source(&self, token: &str, target: &str) -> Result<Value> {
        let mut id = target_id(target)?;
        let response = self
            .client
            .get(format!("{}/data_sources/{id}", self.base))
            .bearer_auth(token)
            .header("Notion-Version", API_VERSION)
            .send()
            .map_err(|_| error("Could not reach Notion. Your notes are saved locally."))?;
        if response.status().as_u16() != 404 {
            return read_json(response);
        }
        let database = self.send(token, Method::GET, &format!("/databases/{id}"), None, false)?;
        let sources = database["data_sources"]
            .as_array()
            .ok_or_else(|| error("Notion returned no data sources for this database."))?;
        if sources.len() != 1 {
            return Err(error("This database has multiple data sources. Choose one in the destination picker or paste its data source ID."));
        }
        id = target_id(sources[0]["id"].as_str().unwrap_or(""))?;
        self.send(
            token,
            Method::GET,
            &format!("/data_sources/{id}"),
            None,
            false,
        )
    }
    fn children(&self, token: &str, parent: &str, max: usize) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        let mut cursor = None::<String>;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10 {
            let mut path = format!("/blocks/{parent}/children?page_size=100");
            if let Some(cursor) = &cursor {
                path.push_str("&start_cursor=");
                path.push_str(
                    &url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect::<String>(),
                );
            }
            let page = self.send(token, Method::GET, &path, None, false)?;
            let items = page["results"].as_array().ok_or_else(|| {
                error("Notion returned an unreadable page. No content was changed.")
            })?;
            if result.len() + items.len() > max {
                return Err(error("This Notion page is too large to reconcile safely. Use a separate page for this meeting."));
            }
            result.extend(items.iter().cloned());
            if page["has_more"] == false {
                return Ok(result);
            }
            let next = page["next_cursor"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 8192)
                .ok_or_else(|| error("Notion returned an unreadable page cursor."))?;
            if !seen.insert(next.to_owned()) {
                return Err(error("Notion returned a repeated page cursor."));
            }
            cursor = Some(next.into());
        }
        Err(error(
            "Notion returned too many page cursors. No page content was changed.",
        ))
    }
    fn publish_snapshot(&self, target: &str, meeting: &Value, settings: &Value) -> Result<String> {
        let selected = tuck_core::domain::publication_snapshot(meeting, settings)?;
        let meeting = &selected;
        let token = self.token()?;
        let source = self.source(&token, target)?;
        let id = target_id(source["id"].as_str().unwrap_or(""))?;
        let title_column = source["properties"]
            .as_object()
            .and_then(|p| {
                p.iter()
                    .find(|(_, v)| v["type"] == "title")
                    .map(|(name, _)| name.as_str())
            })
            .ok_or_else(|| error("This Notion destination has no title column."))?;
        let mut payload = page_payload(&id, title_column, meeting)?;
        let mapped = mapped_properties(&source, &settings["notionPropertyMapping"], meeting)?;
        payload["properties"]
            .as_object_mut()
            .unwrap()
            .extend(mapped);
        let payload = within_limit(payload)?;
        if let Some(address) = meeting["notionUrl"].as_str().filter(|s| !s.is_empty()) {
            let page_id = target_id(address)?;
            let page = self.send(
                &token,
                Method::GET,
                &format!("/pages/{page_id}"),
                None,
                false,
            )?;
            if page["in_trash"] == true || page["archived"] == true {
                return Err(error(
                    "This Notion page is in the trash. Restore it before updating.",
                ));
            }
            if page["parent"]["data_source_id"]
                .as_str()
                .and_then(|s| target_id(s).ok())
                .as_deref()
                != Some(&id)
            {
                return Err(error("This meeting's page belongs to a different Notion destination. Select its original destination to update it."));
            }
            let children = self.children(&token, &page_id, 500)?;
            let candidates = children
                .iter()
                .filter(|b| {
                    b["type"] == "toggle" && plain(&b["toggle"]["rich_text"]) == MANAGED_TITLE
                })
                .collect::<Vec<_>>();
            if candidates.len() > 20 {
                return Err(error(
                    "This page has too many Tuck sections to reconcile safely.",
                ));
            }
            let expected = payload["children"][0]["toggle"]["children"]
                .as_array()
                .unwrap();
            let mut owned = Vec::new();
            let mut keep = None;
            for block in candidates {
                let block_id = target_id(block["id"].as_str().unwrap_or(""))?;
                let content = self.children(&token, &block_id, 100)?;
                if content.last().is_some_and(|b| {
                    b["type"] == "paragraph"
                        && plain(&b["paragraph"]["rich_text"]) == marker(meeting)
                }) {
                    if keep.is_none() && same_blocks(&content, expected) {
                        keep = Some(block_id.clone());
                    }
                    owned.push(block_id);
                }
            }
            // Append the full replacement before retiring any previous Tuck section.
            // On an ambiguous response, a retry reads and matches the actual blocks first.
            if keep.is_none() {
                let appended = self.send(
                    &token,
                    Method::PATCH,
                    &format!("/blocks/{page_id}/children"),
                    Some(&json!({"children":payload["children"]})),
                    true,
                )?;
                let block_id = appended["results"][0]["id"].as_str().and_then(|s|target_id(s).ok())
                    .ok_or_else(|| uncertain("Notion may have added the updated notes. Retry updating this same page to reconcile it."))?;
                let stored = self.children(&token, &block_id, 100).map_err(|_| uncertain("Notion may have added the updated notes. Retry updating this page to verify them; previous notes were kept."))?;
                if !same_blocks(&stored, expected) {
                    return Err(uncertain("Notion returned an incomplete notes section. Previous notes were kept; retry updating this same page to reconcile it."));
                }
                keep = Some(block_id);
            }
            self.send(
                &token,
                Method::PATCH,
                &format!("/pages/{page_id}"),
                Some(&json!({"properties":payload["properties"]})),
                true,
            )?;
            for old in owned
                .iter()
                .filter(|old| Some(old.as_str()) != keep.as_deref())
            {
                // Only the sections whose footer identifies this exact local meeting are retired.
                // All other page content, including older flat exports, stays untouched.
                self.send(&token, Method::DELETE, &format!("/blocks/{old}"), None, true)
                    .map_err(|_| uncertain("Your current notes reached Notion, but an older Tuck section may still be visible. Retry updating this page to finish cleanup."))?;
            }
            Ok(address.into())
        } else {
            let created = self.send(&token, Method::POST, "/pages", Some(&payload), true)?;
            created_url(&created)
        }
    }
}
fn read_status(status: u16) -> AppError {
    match status {
        401 | 403 | 404 => error("Tuck cannot access this Notion destination. Check the token and share the database with the integration."),
        429 | 529 => error("Notion is busy or rate limiting requests. Your notes are saved locally; try later."),
        _ => error("Notion rejected the request. Your notes are saved locally."),
    }
}
fn read_json(response: reqwest::blocking::Response) -> Result<Value> {
    let status = response.status().as_u16();
    if !(200..=299).contains(&status) {
        return Err(read_status(status));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("Notion returned an unreadable response."))?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err(error("Notion returned an oversized response."));
    }
    serde_json::from_slice(&bytes).map_err(|_| error("Notion returned an unreadable response."))
}
fn creation_status(status: u16) -> Result<()> {
    match status {
        200..=299 => Ok(()),
        401 | 403 | 404 | 429 | 529 => Err(read_status(status)),
        400..=499 if status != 408 => Err(error("Notion rejected these notes. Check destination access and property requirements; your notes are saved locally.")),
        _ => Err(uncertain("Notion may have received these notes. Check the destination before publishing a new page; an existing page can be safely reconciled.")),
    }
}
fn created_url(created: &Value) -> Result<String> {
    // Generated links can change independently of the API version. Use the stable
    // record ID and a known human URL instead of parsing an API-generated link.
    created["id"].as_str().and_then(|id| Uuid::parse_str(id).ok())
        .map(|id| format!("https://app.notion.com/p/{id}"))
        .ok_or_else(|| uncertain("Notion accepted the page but returned no usable ID. Check your database before publishing again."))
}
impl PublicationPort for NotionPublisher {
    fn publish(&self, target: &str, meeting: &Value) -> Result<String> {
        self.publish_snapshot(target, meeting, &json!({}))
    }
    fn publish_with_settings(
        &self,
        target: &str,
        meeting: &Value,
        settings: &Value,
    ) -> Result<String> {
        self.publish_snapshot(target, meeting, settings)
    }
    fn destinations(&self, query: &str) -> Result<Value> {
        if query.chars().count() > 200 {
            return Err(error("Keep the Notion search under 200 characters."));
        }
        let token = self.token()?;
        let result = self.send(&token, Method::POST, "/search", Some(&json!({"query":query,"filter":{"property":"object","value":"data_source"},"page_size":100})), false)?;
        let rows = result["results"]
            .as_array()
            .ok_or_else(|| error("Notion returned unreadable search results."))?;
        let destinations = rows.iter().filter(|v|v["object"] == "data_source" && v["in_trash"] != true && v["archived"] != true).filter_map(|v| {
            let id = target_id(v["id"].as_str()?).ok()?;
            let name = plain(&v["title"]);
            Some(json!({"id":id,"title":if name.is_empty(){"Untitled database"}else{&name},"url":format!("https://app.notion.com/p/{id}")}))
        }).collect::<Vec<_>>();
        Ok(json!({"destinations":destinations,"hasMore":result["has_more"] == true}))
    }
    fn destination(&self, target: &str) -> Result<Value> {
        let source = self.source(&self.token()?, target)?;
        let properties = source["properties"]
            .as_object()
            .ok_or_else(|| error("Notion returned unreadable database columns."))?
            .iter()
            .map(|(name, p)| json!({"name":name,"type":p["type"]}))
            .collect::<Vec<_>>();
        Ok(
            json!({"id":target_id(source["id"].as_str().unwrap_or(""))?,"title":plain(&source["title"]),"properties":properties}),
        )
    }
    fn recover(&self, target: &str, meeting: &Value, address: &str) -> Result<String> {
        let token = self.token()?;
        let source = self.source(&token, target)?;
        let source_id = target_id(source["id"].as_str().unwrap_or(""))?;
        let page_id = target_id(address)?;
        let page = self.send(
            &token,
            Method::GET,
            &format!("/pages/{page_id}"),
            None,
            false,
        )?;
        if page["in_trash"] == true
            || page["archived"] == true
            || page["parent"]["data_source_id"]
                .as_str()
                .and_then(|s| target_id(s).ok())
                .as_deref()
                != Some(&source_id)
        {
            return Err(error(
                "Choose this meeting's page in the configured Notion destination.",
            ));
        }
        let children = self.children(&token, &page_id, 500)?;
        let candidates = children
            .iter()
            .filter(|b| b["type"] == "toggle" && plain(&b["toggle"]["rich_text"]) == MANAGED_TITLE)
            .collect::<Vec<_>>();
        if candidates.len() > 20 {
            return Err(error(
                "This page has too many Tuck sections to verify safely.",
            ));
        }
        for candidate in candidates {
            let id = target_id(candidate["id"].as_str().unwrap_or(""))?;
            let content = self.children(&token, &id, 100)?;
            if content.last().is_some_and(|b| {
                b["type"] == "paragraph" && plain(&b["paragraph"]["rich_text"]) == marker(meeting)
            }) {
                return Ok(format!("https://app.notion.com/p/{page_id}"));
            }
        }
        Err(error("This page does not contain this meeting's Tuck notes. Check the page link; nothing was changed."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meeting() -> Value {
        json!({"id":"12345678-1234-1234-1234-123456789012","title":"Call","createdAt":"2026-10-23T12:00:00Z","notes":"* [X] Send proposal","summary":"## Decision\nPilot","segments":[{"text":"private transcript"}],"audio":"raw audio"})
    }
    #[test]
    fn resolve_url_and_reject_unsafe_urls() {
        let id = meeting()["id"].as_str().unwrap().to_owned();
        assert_eq!(
            target_id(&format!(
                "https://www.notion.so/Meetings-{}?v=view",
                id.replace('-', "")
            ))
            .unwrap(),
            id
        );
        for raw in [
            format!("https://evil.test/{id}"),
            format!("http://www.notion.so/{id}"),
            format!("https://www.notion.so:444/{id}"),
            format!("https://user@www.notion.so/{id}"),
        ] {
            assert!(target_id(&raw).is_err());
        }
    }
    #[test]
    fn text_only_snapshot_and_real_title_property() {
        let value = page_payload("source", "Meeting / Topic", &meeting()).unwrap();
        let raw = value.to_string();
        assert!(!raw.contains("private transcript"));
        assert!(!raw.contains("raw audio"));
        assert_eq!(
            value["properties"]["Meeting / Topic"]["title"][0]["text"]["content"],
            "Call"
        );
        let children = value["children"][0]["toggle"]["children"]
            .as_array()
            .unwrap();
        assert!(children.iter().any(|b| b["to_do"]["checked"] == true));
        assert_eq!(
            plain(&children.last().unwrap()["paragraph"]["rich_text"]),
            marker(&meeting())
        );
    }
    #[test]
    fn limits_fail_without_truncating() {
        for content in [
            (0..101).map(|_| "line").collect::<Vec<_>>().join("\n"),
            "\u{0001}".repeat(100_000),
        ] {
            assert!(page_payload("source", "Name", &json!({"id":"id","notes":content})).is_err());
        }
        let mut m = meeting();
        m["notes"] = json!("界".repeat(50_000));
        let payload = page_payload("source", "Name", &m).unwrap();
        for block in payload["children"][0]["toggle"]["children"]
            .as_array()
            .unwrap()
        {
            assert!(block[block["type"].as_str().unwrap()]["rich_text"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["text"]["content"].as_str().unwrap().chars().count() <= 2000));
        }
    }
    #[test]
    fn mapped_columns_are_explicit_and_type_checked() {
        let source = json!({"properties":{"When":{"type":"date"},"Team":{"type":"multi_select"},"Project":{"type":"select"},"Link":{"type":"rich_text"}}});
        let mut m = meeting();
        m["tags"] = json!(["Sales"]);
        m["folder"] = json!("Acme");
        let p = mapped_properties(
            &source,
            &json!({"date":"When","tags":"Team","folder":"Project","meetingId":"Link"}),
            &m,
        )
        .unwrap();
        assert_eq!(p["Team"]["multi_select"][0]["name"], "Sales");
        assert_eq!(p["Project"]["select"]["name"], "Acme");
        assert!(mapped_properties(&source, &json!({"people":"When"}), &m).is_err());
        assert!(mapped_properties(&source, &json!({"tags":"Team","people":"Team"}), &m).is_err());
        m["tags"] = json!(["a,b"]);
        assert!(mapped_properties(&source, &json!({"tags":"Team"}), &m).is_err());
    }
    #[test]
    fn reconciliation_compares_actual_notes_not_only_marker() {
        let expected = note_blocks(&meeting()).unwrap();
        let mut actual = expected.clone();
        actual[0]["id"] = json!("server-fields");
        assert!(same_blocks(&actual, &expected));
        actual[1]["heading_2"]["rich_text"] = json!(rich("Edited in Notion"));
        assert!(!same_blocks(&actual, &expected));
    }
    #[test]
    fn ambiguous_write_outcomes_are_preserved() {
        for status in [302, 408, 500, 502, 503, 504] {
            assert_eq!(
                creation_status(status).unwrap_err().code,
                "publish_uncertain"
            );
        }
        for status in [400, 401, 403, 404, 409, 429, 529] {
            assert_eq!(creation_status(status).unwrap_err().code, "publish_error");
        }
        assert!(creation_status(200).is_ok());
        assert!(created_url(&json!({"url":"https://evil.test/page"})).is_err());
        assert!(created_url(
            &json!({"id":meeting()["id"],"url":"https://future-link-format.test/page"})
        )
        .is_ok());
    }

    const SOURCE: &str = "11111111-1111-4111-8111-111111111111";
    const PAGE: &str = "22222222-2222-4222-8222-222222222222";
    const OLD: &str = "33333333-3333-4333-8333-333333333333";
    const NEW: &str = "44444444-4444-4444-8444-444444444444";
    const OTHER: &str = "55555555-5555-4555-8555-555555555555";
    struct Secrets;
    impl SecretPort for Secrets {
        fn get(&self, _: &str) -> Result<Option<String>> {
            Ok(Some("test-token".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        fn delete(&self, _: &str) -> Result<()> {
            unreachable!()
        }
    }
    type Step = (&'static str, String, u16, Value);
    fn script(steps: Vec<Step>) -> (NotionPublisher, std::thread::JoinHandle<Vec<Value>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let worker = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (method, path, status, response) in steps {
                let mut request = server
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .expect("expected Notion request");
                assert_eq!(request.method().as_str(), method);
                assert_eq!(request.url(), path);
                assert!(request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization")
                        && h.value.as_str() == "Bearer test-token"));
                assert!(request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Notion-Version") && h.value.as_str() == API_VERSION));
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                seen.push(json!({"method":method,"path":path,"body":serde_json::from_str::<Value>(&body).unwrap_or(Value::Null)}));
                request
                    .respond(
                        tiny_http::Response::from_string(response.to_string())
                            .with_status_code(status)
                            .with_header(
                                tiny_http::Header::from_bytes("Content-Type", "application/json")
                                    .unwrap(),
                            ),
                    )
                    .unwrap();
            }
            seen
        });
        (
            NotionPublisher {
                secrets: Arc::new(Secrets),
                client: http_client(Duration::from_secs(3)).unwrap(),
                base: format!("http://{address}"),
            },
            worker,
        )
    }
    fn source_step() -> Step {
        (
            "GET",
            format!("/data_sources/{SOURCE}"),
            200,
            json!({"id":SOURCE,"title":rich("Meetings"),"properties":{"Topic":{"type":"title"}}}),
        )
    }
    #[test]
    fn configured_content_selection_reaches_http_without_excluded_text() {
        let settings = json!({"notionContent":{"recap":false,"notes":true,"tasks":false}});
        let mut m = meeting();
        m["summary"] = json!("Excluded recap");
        m["tasks"] = json!([{"title":"Excluded action"}]);
        let (publisher, worker) = script(vec![
            source_step(),
            (
                "POST",
                "/pages".into(),
                200,
                json!({"id":PAGE,"url":format!("https://app.notion.com/p/{PAGE}")}),
            ),
        ]);
        publisher
            .publish_with_settings(SOURCE, &m, &settings)
            .unwrap();
        let seen = worker.join().unwrap();
        let body = seen[1]["body"].to_string();
        assert!(body.contains("Send proposal"));
        for excluded in [
            "Excluded recap",
            "Excluded action",
            "private transcript",
            "raw audio",
        ] {
            assert!(!body.contains(excluded));
        }
    }
    fn page_step() -> Step {
        (
            "GET",
            format!("/pages/{PAGE}"),
            200,
            json!({"id":PAGE,"parent":{"data_source_id":SOURCE}}),
        )
    }
    fn block(id: &str) -> Value {
        json!({"id":id,"type":"toggle","toggle":{"rich_text":rich(MANAGED_TITLE)}})
    }
    fn list_step(parent: &str, children: Vec<Value>) -> Step {
        (
            "GET",
            format!("/blocks/{parent}/children?page_size=100"),
            200,
            json!({"results":children,"has_more":false,"next_cursor":null}),
        )
    }
    fn published_meeting() -> Value {
        let mut m = meeting();
        m["notionUrl"] = json!(format!("https://app.notion.com/p/{PAGE}"));
        m
    }
    #[test]
    fn updates_keep_unowned_content_and_verify_replacement_before_retiring_old() {
        let m = published_meeting();
        let mut old = m.clone();
        old["summary"] = json!("Old recap");
        let mut other = m.clone();
        other["id"] = json!(OTHER);
        let (publisher, worker) = script(vec![
            source_step(),
            page_step(),
            list_step(
                PAGE,
                vec![
                    block(OLD),
                    json!({"id":"user-paragraph","type":"paragraph"}),
                    block(OTHER),
                ],
            ),
            list_step(OLD, note_blocks(&old).unwrap()),
            list_step(OTHER, note_blocks(&other).unwrap()),
            (
                "PATCH",
                format!("/blocks/{PAGE}/children"),
                200,
                json!({"results":[{"id":NEW}]}),
            ),
            list_step(NEW, note_blocks(&m).unwrap()),
            ("PATCH", format!("/pages/{PAGE}"), 200, json!({"id":PAGE})),
            (
                "DELETE",
                format!("/blocks/{OLD}"),
                200,
                json!({"id":OLD,"in_trash":true}),
            ),
        ]);
        assert_eq!(publisher.publish(SOURCE, &m).unwrap(), m["notionUrl"]);
        let requests = worker.join().unwrap();
        let writes = requests
            .iter()
            .filter(|r| r["method"] != "GET")
            .collect::<Vec<_>>();
        assert_eq!(writes.len(), 3);
        assert_eq!(
            writes[0]["body"]["children"][0]["toggle"]["rich_text"][0]["text"]["content"],
            MANAGED_TITLE
        );
        assert!(!writes[0].to_string().contains("private transcript"));
    }
    #[test]
    fn retry_after_ambiguous_append_reuses_actual_identical_section() {
        let m = published_meeting();
        let (publisher, worker) = script(vec![
            source_step(),
            page_step(),
            list_step(PAGE, vec![]),
            (
                "PATCH",
                format!("/blocks/{PAGE}/children"),
                503,
                json!({"code":"service_unavailable"}),
            ),
            source_step(),
            page_step(),
            list_step(PAGE, vec![block(NEW)]),
            list_step(NEW, note_blocks(&m).unwrap()),
            ("PATCH", format!("/pages/{PAGE}"), 200, json!({"id":PAGE})),
        ]);
        assert_eq!(
            publisher.publish(SOURCE, &m).unwrap_err().code,
            "publish_uncertain"
        );
        assert!(publisher.publish(SOURCE, &m).is_ok());
        let seen = worker.join().unwrap();
        assert_eq!(
            seen.iter()
                .filter(
                    |r| r["method"] == "PATCH" && r["path"].as_str().unwrap().contains("/children")
                )
                .count(),
            1
        );
        assert!(!seen.iter().any(|r| r["method"] == "DELETE"));
    }
    #[test]
    fn incomplete_append_never_retires_the_prior_snapshot() {
        let m = published_meeting();
        let mut old = m.clone();
        old["summary"] = json!("Old recap");
        let (publisher, worker) = script(vec![
            source_step(),
            page_step(),
            list_step(PAGE, vec![block(OLD)]),
            list_step(OLD, note_blocks(&old).unwrap()),
            (
                "PATCH",
                format!("/blocks/{PAGE}/children"),
                200,
                json!({"results":[{"id":NEW}]}),
            ),
            list_step(NEW, vec![]),
        ]);
        assert_eq!(
            publisher.publish(SOURCE, &m).unwrap_err().code,
            "publish_uncertain"
        );
        assert!(!worker
            .join()
            .unwrap()
            .iter()
            .any(|r| r["method"] == "DELETE"));
    }
    #[test]
    fn discovery_searches_shared_data_sources_only() {
        let (publisher, worker) = script(vec![(
            "POST",
            "/search".into(),
            200,
            json!({"has_more":true,"results":[{"object":"data_source","id":SOURCE,"title":rich("Sales notes")},{"object":"page","id":PAGE,"title":rich("Private page")}] }),
        )]);
        let result = publisher.destinations("Sales").unwrap();
        assert_eq!(result["destinations"].as_array().unwrap().len(), 1);
        assert_eq!(result["destinations"][0]["title"], "Sales notes");
        assert_eq!(result["hasMore"], true);
        assert_eq!(
            worker.join().unwrap()[0]["body"]["filter"]["value"],
            "data_source"
        );
    }
    #[test]
    fn recovering_an_uncertain_create_is_read_only_and_meeting_specific() {
        let m = meeting();
        let (publisher, worker) = script(vec![
            source_step(),
            page_step(),
            list_step(PAGE, vec![block(NEW)]),
            list_step(NEW, note_blocks(&m).unwrap()),
        ]);
        assert_eq!(
            publisher
                .recover(SOURCE, &m, &format!("https://app.notion.com/p/{PAGE}"))
                .unwrap(),
            format!("https://app.notion.com/p/{PAGE}")
        );
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
        let mut wrong = m.clone();
        wrong["id"] = json!(OTHER);
        let (publisher, worker) = script(vec![
            source_step(),
            page_step(),
            list_step(PAGE, vec![block(NEW)]),
            list_step(NEW, note_blocks(&wrong).unwrap()),
        ]);
        assert!(publisher
            .recover(SOURCE, &m, &format!("https://app.notion.com/p/{PAGE}"))
            .is_err());
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
    }
    #[test]
    fn empty_but_advancing_pagination_is_bounded() {
        let steps = (0..10)
            .map(|i| {
                (
                    "GET",
                    format!(
                        "/blocks/{PAGE}/children?page_size=100{}",
                        if i == 0 {
                            String::new()
                        } else {
                            format!("&start_cursor={i}")
                        }
                    ),
                    200,
                    json!({"results":[],"has_more":true,"next_cursor":(i+1).to_string()}),
                )
            })
            .collect();
        let (publisher, worker) = script(steps);
        assert!(publisher.children("test-token", PAGE, 500).is_err());
        assert_eq!(worker.join().unwrap().len(), 10);
    }
}
