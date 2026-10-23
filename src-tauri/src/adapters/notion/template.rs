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

    /// Update only the initial view of a newly created, verified database. No extra
    /// view creation is needed, so even an ambiguous PATCH cannot duplicate views.
    fn initial_template_view(&self, token: &str, database: &Value) -> Result<()> {
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
        let views = views["results"]
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| {
                setup_error(
                    "The database is ready. Its initial table view could not be configured.",
                )
            })?;
        let view = &views[0];
        if view["type"] != "table" || id(&view["parent"]["database_id"])? != database_id {
            return Err(setup_error(
                "The database is ready. Its initial table view could not be configured.",
            ));
        }
        let view_id = id(&view["id"])?;
        let columns = COLUMNS.iter().map(|(name,_)| {
            let property_id = database["source"]["properties"][name]["id"].as_str()
                .filter(|s| !s.is_empty() && s.len() <= 100)
                .ok_or_else(|| setup_error("The database is ready. Notion did not return its column IDs."))?;
            Ok(json!({"property_id":property_id,"visible":*name != "Tuck meeting ID" && *name != "Added to Notion","width":if *name == "Name"{320}else{180},"wrap":*name == "Name"}))
        }).collect::<Result<Vec<_>>>()?;
        self.send(token, Method::PATCH, &format!("/views/{view_id}"), Some(&json!({
            "name":"Recent meetings","sorts":[{"property":"Meeting date","direction":"descending"}],
            "configuration":{"type":"table","properties":columns,"wrap_cells":false}
        })), true)?;
        Ok(())
    }

    pub(super) fn template_provision(&self, parent: &str, allow_create: bool) -> Result<Value> {
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
        if self.initial_template_view(&token, &result).is_err() {
            result["setupWarning"] = json!("Your database and columns are ready. Notion could not apply the Recent meetings table layout; you can arrange its view in Notion.");
        }
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
            json!({"results":[{"id":VIEW,"type":"table","parent":{"database_id":DATABASE}}],"has_more":false}),
        )
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
        let view = &seen[6]["body"];
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
        let (publisher, worker) = script(vec![
            parent_step(),
            children_step(json!([child()])),
            database_step(database()),
            source_step(source()),
        ]);
        let result = publisher.provision(PARENT, false).unwrap();
        assert_eq!(result["reused"], true);
        assert!(worker.join().unwrap().iter().all(|r| r["method"] == "GET"));
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
