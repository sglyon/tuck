//! A small translation of Tuck's standard meeting destination into Notion's schema.
//! Creation intent and ambiguous-outcome recovery belong to the core, not this adapter.
use super::*;

const TEMPLATE_TITLE: &str = "Tuck meetings";
const TEMPLATE_MARKER: &str = "Tuck meeting database · template v1";
const COLUMNS: [(&str, &str); 7] = [
    ("Name", "title"),
    ("Meeting date", "date"),
    ("Folder", "select"),
    ("Tags", "multi_select"),
    ("People", "multi_select"),
    ("Tuck meeting ID", "rich_text"),
    ("Added to Notion", "created_time"),
];

fn setup_error(message: &str) -> AppError {
    AppError::new("notion_setup_error", message)
}

fn setup_uncertain() -> AppError {
    AppError::new("notion_setup_uncertain", "Notion may have created your Tuck database. Check again to find it safely; Tuck will not create another database automatically.")
}
fn setup_uncertain_at(database_id: &str) -> AppError {
    AppError::new("notion_setup_uncertain", format!("Notion created a database but Tuck could not finish verifying it: https://app.notion.com/p/{database_id}. Check again to find it safely; Tuck will not create another database automatically."))
}
fn id(value: &Value) -> Result<String> {
    value
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(|s| s.to_string())
        .ok_or_else(|| setup_error("Notion returned an unreadable database ID."))
}
fn active(value: &Value) -> bool {
    value["archived"] != true && value["in_trash"] != true
}
fn schema() -> Value {
    let mut properties = Map::new();
    for (name, kind) in COLUMNS {
        properties.insert(name.into(), json!({"type":kind,kind: {}}));
    }
    json!(properties)
}
fn template_payload(parent: &str) -> Value {
    json!({
        "parent":{"type":"page_id","page_id":parent},
        "title":rich(TEMPLATE_TITLE),
        "description":rich(&format!("{TEMPLATE_MARKER}\nMeeting recaps, personal notes, and action items published by Tuck. People are labels, not workspace identities. Recordings and transcripts stay in Tuck.")),
        "is_inline":true,
        "initial_data_source":{"properties":schema()},
        "icon":{"type":"emoji","emoji":"📝"},
        "show_data_source_title":false
    })
}

fn empty_setting(value: &Value) -> bool {
    value.is_null()
        || value.as_array().is_some_and(Vec::is_empty)
        || value.as_object().is_some_and(Map::is_empty)
}

/// Existing user layouts must not be silently restyled by a setup retry.
fn untouched_initial_view(view: &Value) -> bool {
    matches!(
        view["name"].as_str(),
        Some("" | "Default view" | "Table" | "Table view")
    ) && view["created_time"].as_str().is_some_and(|created| {
        !created.is_empty()
            && chrono::DateTime::parse_from_rfc3339(created).is_ok()
            && view["last_edited_time"].as_str() == Some(created)
    }) && empty_setting(&view["filter"])
        && empty_setting(&view["sorts"])
        && empty_setting(&view["quick_filters"])
        && empty_setting(&view["configuration"]["group_by"])
        && empty_setting(&view["configuration"]["subtasks"])
}

fn matches_template_view(view: &Value, desired: &Value) -> bool {
    view["name"] == desired["name"]
        && empty_setting(&view["filter"])
        && empty_setting(&view["configuration"]["group_by"])
        && view["sorts"] == desired["sorts"]
        && view["configuration"]["type"] == "table"
        && view["configuration"]["wrap_cells"] == false
        && view["configuration"]["properties"]
            .as_array()
            .is_some_and(|actual| {
                let expected = desired["configuration"]["properties"].as_array().unwrap();
                actual.len() == expected.len()
                    && actual.iter().zip(expected).all(|(a, b)| {
                        a["property_id"] == b["property_id"]
                            && a["visible"] == b["visible"]
                            && a["width"].as_f64() == b["width"].as_f64()
                            && a["wrap"] == b["wrap"]
                    })
            })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutOperation {
    Inspect,
    Initialize,
    Finish,
}
enum LayoutState {
    Ready,
    Pending,
    Customized,
}

impl NotionPublisher {
    pub(super) fn template_parents(&self, query: &str) -> Result<Value> {
        if query.chars().count() > 200 {
            return Err(setup_error("Keep the Notion search under 200 characters."));
        }
        let token = self.token()?;
        let response = self.send(
            &token,
            Method::POST,
            "/search",
            Some(&json!({
                "query":query,"filter":{"property":"object","value":"page"},"page_size":100
            })),
            false,
        )?;
        let rows = response["results"]
            .as_array()
            .filter(|r| r.len() <= 100)
            .ok_or_else(|| setup_error("Notion returned unreadable page results."))?;
        let has_more = response["has_more"]
            .as_bool()
            .ok_or_else(|| setup_error("Notion returned unreadable page results."))?;
        let mut seen = std::collections::HashSet::new();
        let pages = rows.iter().filter(|p| p["object"] == "page" && active(p)).filter_map(|page| {
            let page_id = id(&page["id"]).ok()?;
            if !seen.insert(page_id.clone()) { return None; }
            let name = page["properties"].as_object().and_then(|p| p.values().find(|v| v["type"] == "title"))
                .map(|p| plain(&p["title"])).unwrap_or_default();
            Some(json!({"id":page_id,"title":if name.is_empty(){"Untitled page"}else{&name},"url":format!("https://app.notion.com/p/{page_id}")}))
        }).collect::<Vec<_>>();
        Ok(json!({"pages":pages,"hasMore":has_more}))
    }

    fn verify_template(&self, token: &str, database: &Value, parent: &str) -> Result<Value> {
        let database_id = id(&database["id"])?;
        if database["object"] != "database"
            || !active(database)
            || id(&database["parent"]["page_id"]).ok().as_deref() != Some(parent)
            || plain(&database["description"]).lines().next() != Some(TEMPLATE_MARKER)
        {
            return Err(setup_error(
                "This database is not a Tuck template in the selected page. Nothing was changed.",
            ));
        }
        let sources = database["data_sources"].as_array().filter(|s| s.len() == 1)
            .ok_or_else(|| setup_error("This Tuck database has changed data sources. Choose it as an existing database to review its columns."))?;
        let source_id = id(&sources[0]["id"])?;
        let source = self.send(
            token,
            Method::GET,
            &format!("/data_sources/{source_id}"),
            None,
            false,
        )?;
        if source["object"] != "data_source"
            || !active(&source)
            || id(&source["id"])? != source_id
            || id(&source["parent"]["database_id"])? != database_id
        {
            return Err(setup_error(
                "Notion returned a different data source. Nothing was changed.",
            ));
        }
        let properties = source["properties"]
            .as_object()
            .ok_or_else(|| setup_error("Notion returned unreadable database columns."))?;
        for (name, kind) in COLUMNS {
            if properties.get(name).is_none_or(|p| p["type"] != kind) {
                return Err(setup_error("This Tuck database's standard columns have changed. Choose it as an existing database to review its columns; Tuck has not overwritten them."));
            }
        }
        let columns = properties
            .iter()
            .map(|(name, p)| json!({"name":name,"type":p["type"]}))
            .collect::<Vec<_>>();
        Ok(
            json!({"id":source_id,"databaseId":database_id,"title":plain(&database["title"]),
            "url":format!("https://app.notion.com/p/{database_id}"),"properties":columns,
            "mapping":{"date":"Meeting date","folder":"Folder","tags":"Tags","people":"People","meetingId":"Tuck meeting ID"},
            "templateVersion":1,"source":source}),
        )
    }

    /// Inspect the initial view; writes occur only at creation or through the
    /// explicit guarded finishing action. A PATCH cannot duplicate views.
    fn initial_template_view(
        &self,
        token: &str,
        database: &Value,
        operation: LayoutOperation,
    ) -> Result<LayoutState> {
        let database_id = database["databaseId"].as_str().unwrap();
        let views = self.send(
            token,
            Method::GET,
            &format!("/views?database_id={database_id}&page_size=100"),
            None,
            false,
        )?;
        if views["has_more"] != false {
            return Err(setup_error(
                "The database is ready. Its initial table view could not be configured.",
            ));
        }
        let views = views["results"].as_array().ok_or_else(|| {
            setup_error("The database is ready. Its initial table view could not be configured.")
        })?;
        if views.len() != 1 {
            return if operation == LayoutOperation::Initialize {
                Err(setup_error(
                    "The database is ready. Its initial table view could not be configured.",
                ))
            } else {
                Ok(LayoutState::Customized)
            };
        }
        // List views returns references containing only object/id. Retrieve the
        // full view before checking ownership, type, or presentation settings.
        let view_id = id(&views[0]["id"])?;
        let view = self.send(
            token,
            Method::GET,
            &format!("/views/{view_id}"),
            None,
            false,
        )?;
        if view["object"] != "view"
            || view["parent"]["type"] != "database_id"
            || id(&view["parent"]["database_id"])? != database_id
            || id(&view["data_source_id"])? != database["id"].as_str().unwrap()
        {
            return Err(setup_error(
                "The database is ready. Its initial table view could not be configured.",
            ));
        }
        if view["type"] != "table" {
            return if operation == LayoutOperation::Initialize {
                Err(setup_error(
                    "The database is ready. Its initial table view could not be configured.",
                ))
            } else {
                Ok(LayoutState::Customized)
            };
        }
        if id(&view["id"])? != view_id {
            return Err(setup_error(
                "Notion returned a different table view. No layout was changed.",
            ));
        }
        let columns = COLUMNS.iter().map(|(name,_)| {
            let property_id = database["source"]["properties"][name]["id"].as_str()
                .filter(|s| !s.is_empty() && s.len() <= 100)
                .ok_or_else(|| setup_error("The database is ready. Notion did not return its column IDs."))?;
            Ok(json!({"property_id":property_id,"visible":*name != "Tuck meeting ID" && *name != "Added to Notion","width":if *name == "Name"{320}else{180},"wrap":*name == "Name"}))
        }).collect::<Result<Vec<_>>>()?;
        let desired = json!({
            "name":"Recent meetings","sorts":[{"property":"Meeting date","direction":"descending"}],
            "configuration":{"type":"table","properties":columns,"wrap_cells":false}
        });
        if matches_template_view(&view, &desired) {
            return Ok(LayoutState::Ready);
        }
        if operation != LayoutOperation::Initialize && !untouched_initial_view(&view) {
            return Ok(LayoutState::Customized);
        }
        if operation == LayoutOperation::Inspect {
            return Ok(LayoutState::Pending);
        }
        self.send(
            token,
            Method::PATCH,
            &format!("/views/{view_id}"),
            Some(&desired),
            true,
        )?;
        Ok(LayoutState::Ready)
    }

    fn template_layout_result(&self, token: &str, result: &mut Value, operation: LayoutOperation) {
        match self.initial_template_view(token, result, operation) {
            Ok(LayoutState::Ready) => {
                result["layoutReady"] = json!(true);
            }
            Ok(LayoutState::Pending) => {
                result["layoutReady"] = json!(false);
                result["setupWarning"] = json!("Your database and columns are ready. Finish setup to apply Tuck’s Recent meetings table layout.");
            }
            Ok(LayoutState::Customized) => {
                result["layoutReady"] = json!(false);
                result["layoutCustomized"] = json!(true);
                result["layoutMessage"] =
                    json!("Using your current Notion view. Tuck kept its custom layout.");
            }
            Err(_) => {
                result["layoutReady"] = json!(false);
                result["setupWarning"] = json!("Your database and columns are ready. Notion could not apply the Recent meetings table layout; you can try finishing setup again.");
            }
        }
    }

    pub(super) fn template_provision(&self, parent: &str, allow_create: bool) -> Result<Value> {
        self.template_provision_mode(parent, allow_create, false)
    }

    pub(super) fn template_finish_provision(&self, parent: &str) -> Result<Value> {
        self.template_provision_mode(parent, false, true)
    }

    fn template_provision_mode(
        &self,
        parent: &str,
        allow_create: bool,
        finish_layout: bool,
    ) -> Result<Value> {
        let parent = target_id(parent)
            .map_err(|_| setup_error("Choose a shared Notion page for your Tuck database."))?;
        // One credential snapshot is used for the entire operation.
        let token = self.token()?;
        let page = self.send(
            &token,
            Method::GET,
            &format!("/pages/{parent}"),
            None,
            false,
        )?;
        if page["object"] != "page" || !active(&page) || id(&page["id"])? != parent {
            return Err(setup_error(
                "Choose an available Notion page shared with Tuck.",
            ));
        }
        let children = self.children(&token, &parent, 500)?;
        let children = children
            .iter()
            .filter(|b| b["type"] == "child_database" && active(b))
            .collect::<Vec<_>>();
        if children.len() > 20 {
            return Err(setup_error("This page contains too many databases to check safely. Choose a dedicated Tuck page."));
        }
        let mut owned = None;
        for child in children {
            let child_id = id(&child["id"])?;
            let database = self.send(
                &token,
                Method::GET,
                &format!("/databases/{child_id}"),
                None,
                false,
            )?;
            if id(&database["id"])? != child_id {
                return Err(setup_error(
                    "Notion returned a different database. Nothing was changed.",
                ));
            }
            if plain(&database["description"]).lines().next() == Some(TEMPLATE_MARKER) {
                if owned.is_some() {
                    return Err(setup_error("This page has more than one Tuck database. Choose the one you want as an existing database."));
                }
                owned = Some(self.verify_template(&token, &database, &parent)?);
            }
        }
        if let Some(mut result) = owned {
            self.template_layout_result(
                &token,
                &mut result,
                if finish_layout {
                    LayoutOperation::Finish
                } else {
                    LayoutOperation::Inspect
                },
            );
            result.as_object_mut().unwrap().remove("source");
            result["reused"] = json!(true);
            return Ok(result);
        }
        if !allow_create {
            return Err(AppError::new("notion_setup_missing", "The Tuck database is not visible yet. Check again in a moment, or choose an existing database. Tuck has not created another database."));
        }
        let created = self
            .send(
                &token,
                Method::POST,
                "/databases",
                Some(&template_payload(&parent)),
                true,
            )
            .map_err(|e| {
                if e.code == "publish_uncertain" {
                    setup_uncertain()
                } else {
                    setup_error("Notion could not create your Tuck database. Check Tuck's access to this page and try again.")
                }
            })?;
        // Any failure after a successful POST must retain the core's uncertain
        // journal. Search recovery can find the description marker later.
        let database_id = id(&created["id"]).map_err(|_| setup_uncertain())?;
        let database = self
            .send(
                &token,
                Method::GET,
                &format!("/databases/{database_id}"),
                None,
                false,
            )
            .map_err(|_| setup_uncertain_at(&database_id))?;
        if id(&database["id"]).ok().as_deref() != Some(&database_id) {
            return Err(setup_uncertain_at(&database_id));
        }
        let mut result = self
            .verify_template(&token, &database, &parent)
            .map_err(|_| setup_uncertain_at(&database_id))?;
        self.template_layout_result(&token, &mut result, LayoutOperation::Initialize);
        result.as_object_mut().unwrap().remove("source");
        result["reused"] = json!(false);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{script, Step};
    use super::*;

    const PARENT: &str = "11111111-1111-4111-8111-111111111111";
    const DATABASE: &str = "22222222-2222-4222-8222-222222222222";
    const SOURCE: &str = "33333333-3333-4333-8333-333333333333";
    const VIEW: &str = "44444444-4444-4444-8444-444444444444";
    const OTHER: &str = "55555555-5555-4555-8555-555555555555";

    fn parent_step() -> Step {
        (
            "GET",
            format!("/pages/{PARENT}"),
            200,
            json!({"object":"page","id":PARENT}),
        )
    }
    fn children_step(children: Value) -> Step {
        (
            "GET",
            format!("/blocks/{PARENT}/children?page_size=100"),
            200,
            json!({"results":children,"has_more":false}),
        )
    }
    fn child() -> Value {
        json!({"type":"child_database","id":DATABASE})
    }
    fn database() -> Value {
        json!({"object":"database","id":DATABASE,"parent":{"type":"page_id","page_id":PARENT},
            "title":rich(TEMPLATE_TITLE),"description":rich(&format!("{TEMPLATE_MARKER}\nNotes for Tuck")),"data_sources":[{"id":SOURCE}]})
    }
    fn database_step(value: Value) -> Step {
        ("GET", format!("/databases/{DATABASE}"), 200, value)
    }
    fn source() -> Value {
        let mut properties = Map::new();
        for (index, (name, kind)) in COLUMNS.iter().enumerate() {
            properties.insert(
                (*name).into(),
                json!({"id":format!("column{index}"),"type":kind}),
            );
        }
        json!({"object":"data_source","id":SOURCE,"parent":{"database_id":DATABASE},"properties":properties})
    }
    fn source_step(value: Value) -> Step {
        ("GET", format!("/data_sources/{SOURCE}"), 200, value)
    }
    fn create_step(status: u16) -> Step {
        (
            "POST",
            "/databases".into(),
            status,
            json!({"object":"database","id":DATABASE}),
        )
    }
    fn view_step() -> Step {
        (
            "GET",
            format!("/views?database_id={DATABASE}&page_size=100"),
            200,
            json!({"object":"list","results":[{"object":"view","id":VIEW}],"has_more":false,"next_cursor":null}),
        )
    }
    fn initial_view() -> Value {
        json!({"object":"view","id":VIEW,"parent":{"type":"database_id","database_id":DATABASE},
            "data_source_id":SOURCE,"type":"table","name":"Default view","filter":null,"sorts":[],
            "configuration":{"type":"table"},"created_time":"2026-10-23T12:00:00.000Z","last_edited_time":"2026-10-23T12:00:00.000Z"})
    }
    fn detail_step(view: Value) -> Step {
        ("GET", format!("/views/{VIEW}"), 200, view)
    }
    fn configured_view() -> Value {
        let mut view = initial_view();
        view["name"] = json!("Recent meetings");
        view["sorts"] = json!([{"property":"Meeting date","direction":"descending"}]);
        view["configuration"] = json!({"type":"table","wrap_cells":false,"properties":
            COLUMNS.iter().enumerate().map(|(index,(name,_))| json!({"property_id":format!("column{index}"),"visible":*name != "Tuck meeting ID" && *name != "Added to Notion","width":if *name == "Name"{320}else{180},"wrap":*name == "Name"})).collect::<Vec<_>>()});
        view
    }
    #[test]
    fn discovers_only_shared_active_pages_and_respects_bounds() {
        let (publisher, worker) = script(vec![(
            "POST",
            "/search".into(),
            200,
            json!({"results":[
            {"object":"page","id":PARENT,"properties":{"A title":{"type":"title","title":rich("Tuck")}}},
            {"object":"page","id":PARENT},
            {"object":"data_source","id":SOURCE},
            {"object":"page","id":OTHER,"in_trash":true}
        ],"has_more":true}),
        )]);
        assert!(publisher.parents(&"x".repeat(201)).is_err());
        let result = publisher.parents("Tuck").unwrap();
        assert_eq!(result["pages"].as_array().unwrap().len(), 1);
        assert_eq!(result["pages"][0]["title"], "Tuck");
        assert_eq!(result["hasMore"], true);
        let seen = worker.join().unwrap();
        assert_eq!(seen[0]["body"]["filter"]["value"], "page");
        assert_eq!(seen[0]["body"]["page_size"], 100);
    }
    #[test]
    fn creates_standard_schema_and_configures_initial_view_without_fake_meetings() {
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([])),
            create_step(200),
            database_step(database()),
            source_step(source()),
            view_step(),
            detail_step(initial_view()),
            ("PATCH", format!("/views/{VIEW}"), 200, json!({"id":VIEW})),
        ]);
        let result = publisher.provision(PARENT, true).unwrap();
        assert_eq!(result["id"], SOURCE);
        assert_eq!(result["databaseId"], DATABASE);
        assert_eq!(result["mapping"]["people"], "People");
        assert_eq!(result["templateVersion"], 1);
        assert_eq!(result["reused"], false);
        assert!(result.get("source").is_none());
        let seen = worker.join().unwrap();
        let payload = &seen[2]["body"];
        assert_eq!(payload["parent"]["page_id"], PARENT);
        assert_eq!(
            payload["initial_data_source"]["properties"]
                .as_object()
                .unwrap()
                .len(),
            7
        );
        assert_eq!(
            payload["initial_data_source"]["properties"]["People"],
            json!({"type":"multi_select","multi_select":{}})
        );
        assert_eq!(
            plain(&payload["description"]).lines().next(),
            Some(TEMPLATE_MARKER)
        );
        assert!(seen.iter().all(|r| r["path"] != "/pages"));
        let view = &seen[7]["body"];
        assert_eq!(
            view["sorts"][0],
            json!({"property":"Meeting date","direction":"descending"})
        );
        assert_eq!(view["configuration"]["properties"][5]["visible"], false);
        assert_eq!(
            view["configuration"]["properties"][0]["property_id"],
            "column0"
        );
    }
    #[test]
    fn check_only_reuses_verified_template_and_preserves_customized_views() {
        let mut custom = initial_view();
        custom["name"] = json!("My team view");
        for (view, ready, warning, customized) in [
            (initial_view(), false, true, false),
            (configured_view(), true, false, false),
            (custom, false, false, true),
        ] {
            let (publisher, worker) = script(vec![
                parent_step(),
                children_step(json!([child()])),
                database_step(database()),
                source_step(source()),
                view_step(),
                detail_step(view),
            ]);
            let result = publisher.provision(PARENT, false).unwrap();
            assert_eq!(result["reused"], true);
            assert_eq!(result["layoutReady"], ready);
            assert_eq!(result.get("setupWarning").is_some(), warning);
            assert_eq!(result["layoutCustomized"] == true, customized);
            assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
        }
        let (publisher, worker) = script(vec![parent_step(), children_step(json!([]))]);
        assert_eq!(
            publisher.provision(PARENT, false).unwrap_err().code,
            "notion_setup_missing"
        );
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
    }
    #[test]
    fn same_title_is_not_ownership_and_changed_template_is_not_overwritten() {
        let mut ordinary = database();
        ordinary["description"] = json!([]);
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([child()])),
            database_step(ordinary),
        ]);
        assert_eq!(
            publisher.provision(PARENT, false).unwrap_err().code,
            "notion_setup_missing"
        );
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
        let mut changed = source();
        changed["properties"]["People"]["type"] = json!("people");
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([child()])),
            database_step(database()),
            source_step(changed),
        ]);
        assert_eq!(
            publisher.provision(PARENT, true).unwrap_err().code,
            "notion_setup_error"
        );
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
    }
    #[test]
    fn ambiguous_creation_is_not_retried_and_later_check_only_recovers() {
        for status in [408, 500, 503, 529] {
            let (publisher, worker) = script(vec![
                parent_step(),
                children_step(json!([])),
                create_step(status),
                parent_step(),
                children_step(json!([child()])),
                database_step(database()),
                source_step(source()),
                view_step(),
                detail_step(initial_view()),
            ]);
            assert_eq!(
                publisher.provision(PARENT, true).unwrap_err().code,
                "notion_setup_uncertain"
            );
            assert_eq!(publisher.provision(PARENT, false).unwrap()["reused"], true);
            assert_eq!(
                worker
                    .join()
                    .unwrap()
                    .iter()
                    .filter(|r| r["method"] == "POST")
                    .count(),
                1
            );
        }
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([])),
            create_step(200),
            ("GET", format!("/databases/{DATABASE}"), 404, json!({})),
        ]);
        let uncertain = publisher.provision(PARENT, true).unwrap_err();
        assert_eq!(uncertain.code, "notion_setup_uncertain");
        assert!(uncertain
            .message
            .contains(&format!("https://app.notion.com/p/{DATABASE}")));
        assert_eq!(
            worker
                .join()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "POST")
                .count(),
            1
        );
    }
    #[test]
    fn layout_failure_keeps_valid_destination_and_warns() {
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([])),
            create_step(200),
            database_step(database()),
            source_step(source()),
            (
                "GET",
                format!("/views?database_id={DATABASE}&page_size=100"),
                403,
                json!({}),
            ),
        ]);
        let result = publisher.provision(PARENT, true).unwrap();
        assert_eq!(result["id"], SOURCE);
        assert!(result["setupWarning"]
            .as_str()
            .unwrap()
            .contains("columns are ready"));
        assert_eq!(
            worker
                .join()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "POST")
                .count(),
            1
        );
    }
    #[test]
    fn explicit_finish_retrieves_reference_and_initializes_default_view_once_without_database_creation(
    ) {
        let mut steps = vec![
            parent_step(),
            children_step(json!([child()])),
            database_step(database()),
            source_step(source()),
            view_step(),
            detail_step(initial_view()),
            ("PATCH", format!("/views/{VIEW}"), 200, json!({"id":VIEW})),
        ];
        steps.extend([
            parent_step(),
            children_step(json!([child()])),
            database_step(database()),
            source_step(source()),
            view_step(),
            detail_step(configured_view()),
        ]);
        let (publisher, worker) = script(steps);
        for _ in 0..2 {
            let ready = publisher.template_finish_provision(PARENT).unwrap();
            assert_eq!(ready["reused"], true);
            assert_eq!(ready["layoutReady"], true);
            assert!(ready.get("setupWarning").is_none());
        }
        let seen = worker.join().unwrap();
        assert_eq!(seen.iter().filter(|r| r["method"] == "PATCH").count(), 1);
        assert!(seen.iter().all(|r| r["method"] != "POST"));
    }
    #[test]
    fn explicit_finish_preserves_modified_or_unverifiable_initial_views() {
        for (field, value) in [
            ("name", json!("My sales meetings")),
            ("last_edited_time", json!("2026-10-23T12:01:00.000Z")),
            ("created_time", Value::Null),
            (
                "filter",
                json!({"property":"Folder","select":{"equals":"Sales"}}),
            ),
            (
                "sorts",
                json!([{"property":"Folder","direction":"ascending"}]),
            ),
        ] {
            let mut view = initial_view();
            view[field] = value;
            let (publisher, worker) = script(vec![
                parent_step(),
                children_step(json!([child()])),
                database_step(database()),
                source_step(source()),
                view_step(),
                detail_step(view),
            ]);
            let ready = publisher.template_finish_provision(PARENT).unwrap();
            assert_eq!(ready["id"], SOURCE);
            assert_eq!(ready["layoutCustomized"], true);
            assert!(ready.get("setupWarning").is_none());
            assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
        }
    }
    #[test]
    fn rejects_unavailable_parent_and_excessive_or_duplicate_databases_before_writing() {
        let (publisher, worker) = script(vec![(
            "GET",
            format!("/pages/{PARENT}"),
            200,
            json!({"object":"page","id":PARENT,"archived":true}),
        )]);
        assert!(publisher.provision(PARENT, true).is_err());
        assert_eq!(worker.join().unwrap().len(), 1);
        let (publisher, worker) =
            script(vec![parent_step(), children_step(json!(vec![child(); 21]))]);
        assert!(publisher.provision(PARENT, true).is_err());
        assert_eq!(worker.join().unwrap().len(), 2);
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([child(), child()])),
            database_step(database()),
            source_step(source()),
            database_step(database()),
        ]);
        assert!(publisher.provision(PARENT, true).is_err());
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
    }
}
