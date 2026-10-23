use super::*;

fn seed(store: &dyn Store, count: usize) {
    tx(store, |r| {
        for index in 0..count {
            let mut m = Meeting::create(&format!("Meeting {index:03}"))?;
            m.id = format!("meeting-{index:03}");
            // Deliberate ties: the ID tiebreaker must work across every page.
            m.created_at = format!("2026-10-{:02}T12:00:00Z", 1 + index / 20);
            m.folder = format!("Folder {index:03}");
            m.tags = vec![if index % 2 == 0 { "even" } else { "odd" }.into(), "shared".into()];
            m.people = vec![if index % 3 == 0 { "Alex" } else { "Sam" }.into()];
            m.starred = index % 4 == 0;
            m.notes = "Private long notes. ".repeat(3000);
            m.summary = "Private recap. ".repeat(3000);
            let mut task = domain::ActionItem::create(&json!(format!("Task {index:03}")), &json!(""), &json!(if index % 2 == 0 { "2026-12-01" } else { "" }))?;
            task.id = "same-task-id".into(); // Identity is meeting+task, not task alone.
            task.created_at = "2026-10-01T00:00:00Z".into();
            task.done = index % 5 == 0;
            m.tasks.push(task);
            r.save(&mut m)?;
            r.append(&TranscriptSegment::from_dto(json!({"id":"same-segment","meetingId":m.id,"start":0,"end":2,"speakerId":"you","source":"import","text":if index % 2 == 0 {"original phrase"} else {"A needle in this conversation"}}))?)?;
        }
        Ok(Value::Null)
    }).unwrap();
}

#[test]
fn pages_have_stable_ties_bounds_complete_facets_and_global_tasks() {
    for store in stores() {
        seed(&*store, 205);
        let r = rig(store);
        let boot = call(&r.service, "bootstrap_library", json!({"limit":17}));
        assert_eq!(boot["meetingPage"]["total"], 205);
        assert_eq!(
            boot["meetingPage"]["meetings"].as_array().unwrap().len(),
            17
        );
        assert_eq!(boot["library"]["meetingCount"], 205);
        assert_eq!(boot["library"]["starredCount"], 52);
        assert_eq!(boot["library"]["openTaskCount"], 164);
        assert_eq!(boot["library"]["folders"].as_array().unwrap().len(), 200);
        assert_eq!(boot["library"]["facetsTruncated"]["folders"], true);
        assert_eq!(boot["library"]["facetsTruncated"]["tags"], false);
        assert!(
            boot.to_string().len() < 25_000,
            "lean payload must omit long bodies"
        );
        let header = &boot["meetingPage"]["meetings"][0];
        for key in [
            "notes",
            "summary",
            "tasks",
            "segments",
            "speakers",
            "messages",
            "bookmarks",
        ] {
            assert!(header.get(key).is_none(), "unexpected body: {key}");
        }
        assert_eq!(header["segmentCount"], 1);
        assert_eq!(header["speakerCount"], 1);
        assert_eq!(header["taskCount"], 1);
        assert_eq!(header["hasNotes"], true);
        let mut page = boot["meetingPage"].clone();
        let mut ids = Vec::new();
        loop {
            ids.extend(
                page["meetings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["id"].as_str().unwrap().to_owned()),
            );
            if page["nextCursor"].is_null() {
                break;
            }
            page = call(
                &r.service,
                "list_meetings",
                json!({"limit":17,"cursor":page["nextCursor"]}),
            );
        }
        let expected: Vec<_> = (0..205).rev().map(|n| format!("meeting-{n:03}")).collect();
        assert_eq!(ids, expected);
        let offpage = call(
            &r.service,
            "get_meeting",
            json!({"meetingId":"meeting-000"}),
        );
        assert_eq!(offpage["segments"].as_array().unwrap().len(), 1);
        assert!(offpage["notes"].as_str().unwrap().len() > 30_000);

        let mut taskpage = call(&r.service, "list_task_page", json!({"limit":13}));
        assert_eq!(taskpage["total"], 164);
        let mut tasks = Vec::new();
        loop {
            tasks.extend(taskpage["tasks"].as_array().unwrap().iter().cloned());
            if taskpage["nextCursor"].is_null() {
                break;
            }
            taskpage = call(
                &r.service,
                "list_task_page",
                json!({"limit":13,"cursor":taskpage["nextCursor"]}),
            );
        }
        assert_eq!(tasks.len(), 164);
        assert_eq!(
            tasks
                .iter()
                .map(|t| t["meetingId"].as_str().unwrap())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            164
        );
        assert!(tasks
            .windows(2)
            .all(|t| tuck_core::library::task_position(&t[0])
                < tuck_core::library::task_position(&t[1])));
        let scoped = call(
            &r.service,
            "list_task_page",
            json!({"includeDone":true,"scope":{"tags":["even"],"people":["Alex"]}}),
        );
        assert_eq!(scoped["total"], 35);
        assert_eq!(
            call(&r.service, "library_overview", json!({})),
            boot["library"]
        );
    }
}

#[test]
fn selection_searches_all_pages_effective_corrections_and_literal_scope() {
    for store in stores() {
        seed(&*store, 24);
        tx(&*store,|r| {
            r.save_correction("meeting-001","same-segment",Some(&json!({"segmentId":"same-segment","text":"Corrected promise","updatedAt":domain::now()})))?;
            let mut m=r.get("meeting-003")?;
            m.notes="Notes-only phrase Äpfel".into(); r.save(&mut m)?;
            Ok(Value::Null)
        }).unwrap();
        let r = rig(store);
        let p = call(
            &r.service,
            "list_meetings",
            json!({"query":"corrected PROMISE","scope":{"meetingIds":["meeting-001"]}}),
        );
        assert_eq!(p["total"], 1);
        let p = call(
            &r.service,
            "list_meetings",
            json!({"query":"needle","scope":{"meetingIds":["meeting-001"]}}),
        );
        assert_eq!(
            p["total"], 0,
            "original ASR must not override the correction"
        );
        assert_eq!(
            call(
                &r.service,
                "list_meetings",
                json!({"query":"NOTES-ONLY phrase Äpfel"})
            )["total"],
            1
        );
        let mut p = call(
            &r.service,
            "list_meetings",
            json!({"limit":2,"starred":true,"scope":{"tags":["shared","even"],"people":["Alex"]}}),
        );
        assert_eq!(p["total"], 2);
        assert_eq!(p["meetings"][0]["id"], "meeting-012");
        p = call(
            &r.service,
            "list_meetings",
            json!({"scope":{"folder":"Folder 000","tags":["odd"]}}),
        );
        assert_eq!(p["total"], 0);
        assert_eq!(
            call(&r.service, "list_meetings", json!({"query":"%' OR 1=1 --"}))["total"],
            0
        );
    }
}

#[test]
fn cursor_binding_rollback_and_relevant_mutations_are_explicit() {
    for store in stores() {
        seed(&*store, 4);
        let r = rig(store.clone());
        let page = call(&r.service, "list_meetings", json!({"limit":1}));
        let cursor = &page["nextCursor"];
        assert!(r
            .service
            .handle("list_meetings", json!({"cursor":cursor,"starred":true}))
            .is_err());
        tx(&*store, |repo| {
            let mut m = repo.get("meeting-000")?;
            m.starred = !m.starred;
            repo.save(&mut m)?;
            Err(AppError::invalid("rollback"))
        })
        .unwrap_err();
        assert!(r
            .service
            .handle("list_meetings", json!({"cursor":cursor}))
            .is_ok());
        tx(&*store,|repo| {repo.add_revision("meeting-000",&json!({"id":"revision","target":"notes","text":"history","createdAt":domain::now()}))?;Ok(Value::Null)}).unwrap();
        assert!(r
            .service
            .handle("list_meetings", json!({"cursor":cursor}))
            .is_ok());
        for mutation in [
            "title",
            "starred",
            "correction",
            "segment",
            "create",
            "delete",
            "message",
        ] {
            let page = call(&r.service, "list_meetings", json!({"limit":1}));
            let cursor = page["nextCursor"].clone();
            tx(&*store, |repo| {
                match mutation {
                    "title" | "starred" => {
                        let mut m = repo.get("meeting-000")?;
                        if mutation == "title" {
                            m.title = "Changed".into()
                        } else {
                            m.starred = !m.starred;
                        }
                        repo.save(&mut m)?;
                    }
                    "correction" => repo.save_correction(
                        "meeting-000",
                        "same-segment",
                        Some(&json!({"segmentId":"same-segment","text":"Edited"})),
                    )?,
                    "segment" => {
                        repo.append(&TranscriptSegment::from_dto(segment("meeting-000", "new"))?)?;
                    }
                    "create" => {
                        repo.save(&mut Meeting::create("New")?)?;
                    }
                    "delete" => {
                        let m = repo.get("meeting-003")?;
                        repo.delete(&m)?;
                    }
                    "message" => repo.add_message(
                        "meeting-000",
                        &json!({"id":"newmsg","content":"Private body","createdAt":domain::now()}),
                    )?,
                    _ => unreachable!(),
                }
                Ok(Value::Null)
            })
            .unwrap();
            assert_eq!(
                r.service
                    .handle("list_meetings", json!({"cursor":cursor}))
                    .unwrap_err()
                    .code,
                "cursor_stale",
                "{mutation}"
            );
        }
        for limit in [json!(0), json!(101), json!(-1), json!(1.5), json!("5")] {
            assert!(r
                .service
                .handle("list_meetings", json!({"limit":limit}))
                .is_err());
        }
        for cursor in [json!("garbage"), json!(123), json!("x".repeat(128_001))] {
            assert!(r
                .service
                .handle("list_meetings", json!({"cursor":cursor}))
                .is_err());
        }
        let escaped: Vec<_> = (0..100)
            .map(|i| format!("{i}{}", "\\\n".repeat(100)))
            .collect();
        assert!(r
            .service
            .handle(
                "list_meetings",
                json!({"scope":{"tags":escaped,"people":escaped,"meetingIds":escaped}})
            )
            .is_err());
        let p = call(
            &r.service,
            "list_meetings",
            json!({"limit":1,"scope":{"tags":["shared","shared"]}}),
        );
        assert!(
            r.service
                .handle(
                    "list_meetings",
                    json!({"cursor":p["nextCursor"],"scope":{"tags":["shared"]}})
                )
                .is_ok(),
            "normalized selection must replay"
        );
    }
}

#[test]
fn sqlite_lean_startup_never_decodes_unselected_evidence_or_inactive_meeting_bodies() {
    let path = std::env::temp_dir().join(format!("tuck-library-loading-{}.sqlite3", domain::id()));
    {
        let store = SqliteStore::open(&path).unwrap();
        seed(&store, 3);
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute(
            "UPDATE segments SET document='unreadable evidence' WHERE meeting_id='meeting-000'",
            [],
        )
        .unwrap();
        // Valid JSON, deliberately unreadable as a full Meeting: startup recovery
        // must not deserialize inactive note bodies or unrelated messages.
        db.execute(
            "UPDATE meetings SET document=json_set(document,'$.notes',42) WHERE id='meeting-000'",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO messages VALUES('meeting-000','broken','2026','unreadable message')",
            [],
        )
        .unwrap();
    }
    let r = rig(Arc::new(SqliteStore::open(&path).unwrap()));
    let boot = call(&r.service, "bootstrap_library", json!({"limit":1}));
    assert_eq!(boot["meetingPage"]["total"], 3);
    assert_eq!(boot["meetingPage"]["meetings"][0]["id"], "meeting-002");
    assert_eq!(
        call(
            &r.service,
            "get_meeting",
            json!({"meetingId":"meeting-001"})
        )["segments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(r
        .service
        .handle("get_meeting", json!({"meetingId":"meeting-000"}))
        .is_err());
    assert!(
        r.service.handle("bootstrap", json!({})).is_err(),
        "legacy full bootstrap still reads full DTOs"
    );
    drop(r);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn lean_headers_follow_recording_metadata_and_detail_loading() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{"title":"Planning","folder":"Work","tags":["team"],"people":["Alex"],"starred":true}}),
        );
        call(&r.service, "start_recording", json!({"meetingId":id}));
        r.service.segment(&id, segment(&id, "live")).unwrap();
        // Capture callback appended evidence, but lean payload still has only counts.
        let boot = call(&r.service, "bootstrap_library", json!({}));
        let header = &boot["meetingPage"]["meetings"][0];
        assert_eq!(header["status"], "recording");
        assert_eq!(header["folder"], "Work");
        assert_eq!(header["starred"], true);
        assert_eq!(header["segmentCount"], 1);
        assert_eq!(boot["library"]["activeMeetingId"], id);
        assert_eq!(boot["status"]["activeMeetingId"], id);
        assert_eq!(boot["library"]["tags"], json!(["team"]));
        let detail = call(&r.service, "get_meeting", json!({"meetingId":id}));
        assert_eq!(detail["status"], "recording");
        assert_eq!(detail["segments"].as_array().unwrap().len(), 1);
        call(&r.service, "stop_recording", json!({"meetingId":id}));
        assert_eq!(
            call(&r.service, "library_overview", json!({}))["activeMeetingId"],
            Value::Null
        );
        assert_eq!(
            call(&r.service, "list_meetings", json!({}))["meetings"][0]["status"],
            "complete"
        );
    }
}
