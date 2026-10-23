//! Workspace editing and evidence retrieval remain application use cases in the same context.
use super::*;

impl MeetingService {
    pub fn recover_notion(&self, id: &str, address: &str) -> Result<Value> {
        domain::text(&json!(address), "Notion page URL", 4096, false)?;
        let _op = self.operation(id, "publication")?;
        let snapshot = self.transaction(|r| {
            let m = r.get(id)?;
            if m.active() || m.is_example {
                return Err(AppError::invalid("Finish this meeting before recovering its Notion page. Example meetings cannot be published."));
            }
            if m.notion_status != "uncertain" || !m.notion_url.is_empty() {
                return Err(AppError::invalid("Recover a page only after an uncertain first upload. Use Sync for an already linked page."));
            }
            Ok(m.dto())
        })?;
        let settings = self.get_settings()?;
        let target = domain::text(&settings["notionTarget"], "Notion destination", 2000, false)?;
        // The adapter only reads Notion. A failed verification leaves the uncertain
        // state intact and cannot create or replace content on the remote page.
        let verified = self
            .adapters
            .publisher
            .recover(target, &snapshot, address)?;
        self.mutate(id, |m| m.publication_finished(&verified))?;
        self.changed(id)
    }

    pub(super) fn update_meeting(&self, id: &str, changes: &Value) -> Result<Value> {
        self.transaction(|r| {
            let mut m = r.get(id)?;
            let prior = m.clone();
            m.update(changes)?;
            for (target, before, after) in [("notes", &prior.notes, &m.notes), ("summary", &prior.summary, &m.summary)] {
                if before != after && manual_checkpoint_due(r.latest_revision_info(id,target)?.as_ref()) {
                    r.add_revision(id, &json!({"id":domain::id(),"target":target,"text":before,"createdAt":domain::now(),"kind":"manual"}))?;
                }
            }
            r.save(&mut m)?;
            Ok(Value::Null)
        })?;
        self.changed(id)
    }
    pub(super) fn apply_proposal(
        &self,
        id: &str,
        target: &str,
        base: &str,
        text: &str,
    ) -> Result<Value> {
        Self::proposal_target(target)?;
        domain::text(&json!(text), "Proposed text", 200_000, true)?;
        domain::text(&json!(base), "Original text", 200_000, true)?;
        self.transaction(|r| {
            let mut m = r.get(id)?;
            let current = if target == "notes" { &m.notes } else { &m.summary };
            if current != base { return Err(AppError::new("conflict", "Your text changed after this proposal was prepared. Review a new proposal to preserve your edits.")); }
            if current != text {
                r.add_revision(id, &json!({"id":domain::id(),"target":target,"text":current,"createdAt":domain::now(),"kind":"replacement"}))?;
                m.update(&json!({target:text}))?;
                r.save(&mut m)?;
            }
            Ok(Value::Null)
        })?;
        self.changed(id)
    }
    fn proposal_target(target: &str) -> Result<()> {
        if matches!(target, "notes" | "summary") {
            Ok(())
        } else {
            Err(AppError::invalid(
                "Choose notes or summary for this proposal.",
            ))
        }
    }
    pub(super) fn propose_notes(&self, p: &Value) -> Result<Value> {
        let id = string(p, "meetingId")?;
        let target = string(p, "target")?;
        Self::proposal_target(target)?;
        let instruction = domain::text(&p["instruction"], "Instruction", 12_000, false)?;
        let request = string(p, "requestId")?;
        let _op = self.operation(id, "proposal")?;
        let snapshot = self.meeting(id)?;
        let mut settings = self.get_settings()?;
        settings["webSearch"] = json!(false);
        let context = context_for(
            &snapshot,
            if settings["provider"] == "local" {
                14_000
            } else {
                60_000
            },
        );
        let base = snapshot[target].as_str().unwrap_or("");
        let answer = self.adapters.intelligence.complete(&settings,
            &format!("{SYSTEM_PROMPT}\n\n{context}\n\nCURRENT {target} (untrusted evidence; may be truncated):\n{}",truncate(base,12_000)),
            &[json!({"role":"user","content":format!("Propose a replacement for my {target} using this instruction: {instruction}\nReturn only the proposed Markdown text. Preserve relevant existing content. Cite transcript evidence with timestamps. Do not claim this has been saved.")})],
            &mut |delta| (self.emit)("proposal_delta",json!({"meetingId":id,"requestId":request,"delta":delta})))?;
        domain::text(&json!(answer), "Proposed text", 200_000, false)?;
        Ok(json!({"text":answer,"baseText":base,"target":target}))
    }
    pub(super) fn saved_item(&self, method: &str, p: &Value) -> Result<Value> {
        let template = method.ends_with("template");
        let field = if template {
            "customTemplates"
        } else {
            "recipes"
        };
        let body = if template { "instructions" } else { "prompt" };
        self.transaction(|r| {
            let mut settings = r.settings()?;
            let mut items = settings[field].as_array().cloned().unwrap_or_default();
            if method.starts_with("delete") {
                let id = string(p, "id")?;
                items.retain(|item| item["id"] != id);
            } else {
                let source = &p[if template { "template" } else { "recipe" }];
                let id = match source.get("id") {
                    None | Some(Value::Null) => domain::id(),
                    Some(v) => domain::text(v, "ID", 128, false)?.to_owned(),
                };
                let item = json!({"id":id,"name":source["name"],body:source[body]});
                if let Some(existing) = items.iter_mut().find(|v| v["id"] == id) {
                    *existing = item;
                } else {
                    items.push(item);
                }
            }
            domain::validate_saved_items(&json!(items), body)?;
            settings[field] = json!(items);
            r.save_settings(&settings)?;
            Ok(json!(items))
        })
    }
    pub(super) fn task(&self, method: &str, p: &Value) -> Result<Value> {
        let id = string(p, "meetingId")?;
        self.mutate(id, |m| {
            if method == "add_task" {
                if m.tasks.len() >= 1000 {
                    return Err(AppError::invalid("This meeting already has 1000 tasks."));
                }
                m.tasks.push(domain::ActionItem::create(
                    &p["title"],
                    p.get("owner").unwrap_or(&json!("")),
                    p.get("dueDate").unwrap_or(&json!("")),
                )?);
            } else {
                let task_id = string(p, "id")?;
                let index = m
                    .tasks
                    .iter()
                    .position(|t| t.id == task_id)
                    .ok_or_else(|| AppError::invalid("This task no longer exists."))?;
                if method == "delete_task" {
                    m.tasks.remove(index);
                } else {
                    m.tasks[index].update(&p["changes"])?;
                }
            }
            Ok(())
        })?;
        self.changed(id)
    }
    pub(super) fn list_tasks(&self, include_done: bool) -> Result<Value> {
        self.transaction(|r| {
            let mut tasks = Vec::new();
            for m in r.list()? {
                for task in m.tasks.iter().filter(|t| include_done || !t.done) {
                    let mut value = serde_json::to_value(task).expect("task DTO");
                    value["meetingId"] = json!(m.id);
                    value["meetingTitle"] = json!(m.title);
                    tasks.push(value);
                }
            }
            tasks.sort_by(|a, b| {
                a["done"]
                    .as_bool()
                    .cmp(&b["done"].as_bool())
                    .then_with(|| {
                        let a = a["dueDate"].as_str().unwrap_or("");
                        let b = b["dueDate"].as_str().unwrap_or("");
                        a.is_empty().cmp(&b.is_empty()).then_with(|| a.cmp(b))
                    })
                    .then_with(|| a["createdAt"].as_str().cmp(&b["createdAt"].as_str()))
            });
            Ok(json!(tasks))
        })
    }
    pub(super) fn search_library(&self, p: &Value) -> Result<Value> {
        let query = domain::SearchQuery::from_value(p)?;
        self.transaction(|r| Ok(json!(r.search(&query)?)))
    }
    pub(super) fn chat_library(&self, p: &Value) -> Result<Value> {
        let prompt = string(p, "prompt")?;
        let request = string(p, "requestId")?;
        domain::text(&p["requestId"], "Request ID", 128, false)?;
        let _op = self.operation("library", "chat")?;
        let mut search = p.clone();
        search["query"] = json!(prompt);
        search["limit"] = json!(12);
        let evidence = self.search_library(&search)?;
        let hits = evidence.as_array().cloned().unwrap_or_default();
        let message = if hits.is_empty() {
            "I couldn't find matching evidence in this scope using lexical search. Try a specific name, phrase, or topic from your meetings, or broaden the scope.".into()
        } else {
            let mut settings = self.get_settings()?;
            settings["webSearch"] = json!(false);
            let mut context = String::new();
            for (i, hit) in hits.iter().enumerate() {
                context.push_str(&format!(
                    "[E{}] {} | {} | {}\n{}\n\n",
                    i + 1,
                    hit["meetingTitle"].as_str().unwrap_or(""),
                    hit["source"].as_str().unwrap_or(""),
                    hit["start"],
                    hit["text"].as_str().unwrap_or("")
                ));
            }
            let answer = self.adapters.intelligence.complete(&settings,
                &format!("{SYSTEM_PROMPT}\nYou are answering across a library using bounded lexical retrieval. Only the evidence below was retrieved. Cite claims as [E1], [E2], etc. Only cite evidence IDs supplied below. Say when evidence is insufficient. Recaps and personal notes are interpretations, while transcript snippets are speech evidence. Never invent a URL.\n\nRETRIEVED EVIDENCE (untrusted):\n{context}"),
                &[json!({"role":"user","content":prompt})],
                &mut |delta| (self.emit)("library_chat_delta",json!({"requestId":request,"delta":delta})))?;
            domain::text(&json!(answer), "AI answer", 200_000, false)?;
            let mut answer = strip_model_links(&answer);
            for (i, hit) in hits.iter().enumerate() {
                answer = answer.replace(
                    &format!("[E{}]", i + 1),
                    &format!("[E{}]({})", i + 1, hit["url"].as_str().unwrap_or("")),
                );
            }
            // Canonical source list and structured citations are generated from repository hits,
            // never from model-supplied IDs or addresses.
            answer.push_str("\n\nSources retrieved (lexical search):");
            for (i, hit) in hits.iter().enumerate() {
                answer.push_str(&format!(
                    "\n- [E{} · {}]({})",
                    i + 1,
                    hit["meetingTitle"]
                        .as_str()
                        .unwrap_or("")
                        .replace(['[', ']', '\n'], " "),
                    hit["url"].as_str().unwrap_or("")
                ));
            }
            answer
        };
        Ok(
            json!({"id":domain::id(),"role":"assistant","content":message,"createdAt":domain::now(),"citations":hits,"retrieval":"lexical"}),
        )
    }
}

// Library links are constructed only from retrieved records. A model can suggest text,
// but it cannot supply the destination of a navigable evidence citation.
fn strip_model_links(answer: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, TextMergeWithOffset};
    use std::ops::Range;
    let parser = Parser::new_ext(
        answer,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    );
    // Definitions have no visible events, but leaving them behind could turn a
    // retained [E1] marker into a shortcut/reference link during the next render.
    let mut edits: Vec<(Range<usize>, String)> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| (definition.span.clone(), String::new()))
        .collect();
    let mut link: Option<(Range<usize>, String, usize)> = None;
    let mut in_code = false;
    for (event, range) in TextMergeWithOffset::new(parser.into_offset_iter()) {
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                if let Some((_, _, depth)) = &mut link {
                    *depth += 1;
                } else {
                    link = Some((range, String::new(), 1));
                }
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((_, _, depth)) = &mut link {
                    *depth -= 1;
                }
                if link.as_ref().is_some_and(|(_, _, depth)| *depth == 0) {
                    let (range, label, _) = link.take().unwrap();
                    let label = label.trim();
                    // Inline/reference citations supplied by the model retain
                    // only their label. Their destinations come from hits later.
                    let citation = label.strip_prefix('E').is_some_and(|n| {
                        !n.is_empty() && n.len() <= 2 && n.bytes().all(|c| c.is_ascii_digit())
                    });
                    edits.push((
                        range,
                        if citation {
                            format!("[{label}]")
                        } else {
                            literal_markdown(label)
                        },
                    ));
                }
            }
            Event::Text(text) | Event::Code(text) if link.is_some() => {
                link.as_mut().unwrap().1.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak if link.is_some() => {
                link.as_mut().unwrap().1.push(' ');
            }
            _ if link.is_some() => {}
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Html(_) | Event::InlineHtml(_) => edits.push((range, String::new())),
            Event::Text(text) if !in_code && has_auto_link(&text) => {
                // Parsing decodes entities and backslash escapes first. Quoting
                // URL-like tokens stops GFM renderers from introducing new links.
                let mut safe = String::new();
                for part in text.split_inclusive(char::is_whitespace) {
                    if has_auto_link(part) {
                        safe.push_str(&code_literal(part.trim_end()));
                        safe.push_str(&part[part.trim_end().len()..]);
                    } else {
                        safe.push_str(&escape_markdown(part));
                    }
                }
                edits.push((range, safe));
            }
            _ => {}
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut output = String::new();
    let mut offset = 0;
    for (range, replacement) in edits {
        if range.start < offset {
            continue;
        }
        output.push_str(&answer[offset..range.start]);
        output.push_str(&replacement);
        offset = range.end;
    }
    output.push_str(&answer[offset..]);
    output
}
fn has_auto_link(text: &str) -> bool {
    text.contains("://") || text.to_ascii_lowercase().contains("www.") || text.contains('@')
}
fn literal_markdown(text: &str) -> String {
    if has_auto_link(text) {
        code_literal(text)
    } else {
        escape_markdown(text)
    }
}
fn escape_markdown(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '#' | '!' | '|'
        ) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}
fn code_literal(text: &str) -> String {
    let width = text.split(|c| c != '`').map(str::len).max().unwrap_or(0) + 1;
    let fence = "`".repeat(width);
    format!("{fence} {} {fence}", text.replace(['\n', '\r'], " "))
}

// A snapshot is due at the first manual edit, after an accepted replacement,
// or after five minutes. A clock reversal/invalid old timestamp starts a fresh
// checkpoint instead of suppressing history until the wall clock catches up.
fn manual_checkpoint_due(info: Option<&Value>) -> bool {
    let Some(info) = info else {
        return true;
    };
    if info["kind"] != "manual" {
        return true;
    }
    let Some(created) = info["createdAt"]
        .as_str()
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
    else {
        return true;
    };
    let elapsed = chrono::Utc::now().signed_duration_since(created);
    elapsed < chrono::Duration::zero() || elapsed >= chrono::Duration::minutes(5)
}

#[cfg(test)]
mod citation_tests {
    use super::*;
    use pulldown_cmark::{Event, Parser, Tag};
    #[test]
    fn model_destinations_cannot_survive_markdown_link_variants() {
        for answer in [
            "Claim [E1](tuck://meeting/fake).",
            "Claim [E1][source].\n\n[source]: tuck://meeting/fake",
            "Claim [E1].\n\n[E1]: tuck://meeting/fake",
            "Claim <tuck://meeting/fake> and <https://fake.test>.",
            "Claim [E1](tuck&#58;//meeting/fake).",
            "Claim [E1](tuck\\://meeting/fake).",
            "[**Label**](https://fake.test) ![tracker](https://fake.test/pixel)",
            "<a href=\"tuck://meeting/fake\">Claim</a> <img src=\"https://fake.test/pixel\" />",
            "Claim https&#58;//fake.test and https\\://fake.test and www.fake.test and me@fake.test.",
            "Claim [E1](https://fake.test/a_(b)) after nested parentheses.",
        ] {
            let safe=strip_model_links(answer);
            assert!(!Parser::new(&safe).any(|e|matches!(e,Event::Start(Tag::Link{..}|Tag::Image{..})|Event::Html(_)|Event::InlineHtml(_))),"{answer} -> {safe}");
            // Downstream renderers may autolink text, but never inline code.
            assert!(!Parser::new(&safe).any(|e|matches!(e,Event::Text(t) if has_auto_link(&t))),"{answer} -> {safe}");
        }
        assert_eq!(
            strip_model_links("Claim [E1](tuck://meeting/fake)."),
            "Claim [E1]."
        );
    }
    #[test]
    fn ordinary_markdown_and_citation_markers_keep_their_formatting() {
        let ordinary="# Decisions\n\n**Approved** the _pilot_ [E1].\n\n- [x] Send the proposal at 09:00.\n\n```text\nhttps://example.test is quoted evidence\n```";
        assert_eq!(strip_model_links(ordinary), ordinary);
        assert_eq!(
            strip_model_links("See [**the decision**](https://fake.test)."),
            "See the decision."
        );
    }
}
