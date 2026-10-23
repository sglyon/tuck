//! Synthetic local acceptance: real native AI adapter, core and on-disk SQLite.
//! No hardware capture, real model, personal library, hosted account or publication.
use crate::adapters::{archive::LocalArchive, chatgpt::ChatGptAuth, intelligence::Intelligence};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, thread, time::Duration};
use tuck_core::{
    ports::{OnSegment, PublicationPort, RecordingPort, Result, SecretPort, Store},
    store::sqlite::SqliteStore,
    Adapters, MeetingService,
};

const QUIET: &str = "00000000-0000-4000-8000-000000000001";
const SALES: &str = "00000000-0000-4000-8000-000000000002";
const WORDS: &str = "We need to understand the budget before agreeing to a pilot and the customer asked for an example report.";
const QUOTE: &str = "the customer asked for an example report";

struct NoSecrets;
impl SecretPort for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<()> {
        panic!("This acceptance flow must never write credentials")
    }
    fn delete(&self, _: &str) -> Result<()> {
        panic!("This acceptance flow must never delete credentials")
    }
}
struct NoPublication;
impl PublicationPort for NoPublication {
    fn publish(&self, _: &str, _: &Value) -> Result<String> {
        panic!("Coach suggestions must never publish meeting notes automatically")
    }
}
struct SyntheticCapture;
impl RecordingPort for SyntheticCapture {
    fn status(&self) -> Value {
        json!({"captureAvailable":true,"modelInstalled":true,"platform":"synthetic acceptance"})
    }
    fn start(
        &self,
        id: &str,
        on: OnSegment,
        system: bool,
        microphone: bool,
        _: u32,
        _: bool,
    ) -> Result<()> {
        assert!(!system && microphone);
        on(
            json!({"id":"source-old","meetingId":id,"start":0.,"end":5.,"text":"STALE_EVIDENCE_EXCLUDED This old passage must not be sent with the recent call context.","speakerId":"you","source":"microphone"}),
        )?;
        on(
            json!({"id":"source-recent","meetingId":id,"start":1000.,"end":1005.,"text":WORDS,"speakerId":"you","source":"microphone"}),
        )
    }
    fn pause(&self) -> Result<()> {
        Ok(())
    }
    fn resume(&self) -> Result<()> {
        Ok(())
    }
    fn stop(&self) -> Result<f64> {
        Ok(1005.)
    }
    fn shutdown(&self) -> Result<()> {
        Ok(())
    }
    fn install_model(&self, _: &str) -> Result<Value> {
        panic!("No model download is permitted in this fixture")
    }
    fn unload(&self) -> Result<()> {
        Ok(())
    }
    fn import_file(&self, _: &str, _: &str, _: OnSegment, _: u32) -> Result<f64> {
        panic!("No audio import is permitted in this fixture")
    }
}

fn service(data: &Path) -> (Arc<MeetingService>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(data.join("acceptance.sqlite3")).unwrap());
    let secrets: Arc<dyn SecretPort> = Arc::new(NoSecrets);
    let accounts = Arc::new(ChatGptAuth::new(data, secrets.clone()).unwrap());
    let ai = Arc::new(Intelligence::new(accounts.clone(), secrets.clone()).unwrap());
    let app = MeetingService::new(
        store.clone(),
        Adapters {
            capture: Arc::new(SyntheticCapture),
            intelligence: ai,
            publisher: Arc::new(NoPublication),
            secrets,
            accounts,
            archive: Arc::new(LocalArchive::new(data)),
        },
        Arc::new(|_, _| {}),
    )
    .unwrap();
    (app, store)
}
fn call(app: &Arc<MeetingService>, method: &str, params: Value) -> Value {
    app.handle(method, params).unwrap()
}
fn no_http(server: &tiny_http::Server) {
    assert!(
        server
            .recv_timeout(Duration::from_millis(10))
            .unwrap()
            .is_none(),
        "This step must not make an AI request"
    );
}

#[test]
fn native_local_coach_batches_grounded_agents_privately_and_persists_reviewed_follow_up() {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let endpoint = format!("http://{}", server.server_addr().to_ip().unwrap());
    let data = tempfile::tempdir().unwrap();
    let (app, store) = service(data.path());
    call(
        &app,
        "update_settings",
        json!({"changes":{"provider":"local","localUrl":endpoint,"model":"synthetic-local-model"}}),
    );
    let meeting = call(
        &app,
        "create_meeting",
        json!({"title":"PRIVATE_TITLE_EXCLUDED"}),
    );
    let id = meeting["id"].as_str().unwrap();
    call(
        &app,
        "update_meeting",
        json!({"id":id,"changes":{"notes":"PRIVATE_MY_NOTES_EXCLUDED","people":["PRIVATE_PERSON_EXCLUDED"]}}),
    );
    store.transaction(&mut |repo| {
        repo.add_message(id,&json!({"id":"private-chat","role":"user","content":"PRIVATE_CHAT_EXCLUDED","createdAt":"2026-10-23T12:00:00Z"}))?;
        Ok(Value::Null)
    }).unwrap();
    call(
        &app,
        "start_recording",
        json!({"meetingId":id,"microphone":true,"systemAudio":false}),
    );
    assert_eq!(
        call(&app, "get_live_coach", json!({"meetingId":id}))["enabled"],
        false
    );
    app.coach_tick_at(0).unwrap();
    app.coach_tick_at(300_000).unwrap();
    no_http(&server);

    call(
        &app,
        "set_live_coach",
        json!({"meetingId":id,"enabled":true,"agentIds":[QUIET,SALES]}),
    );
    app.coach_tick_at(0).unwrap();
    app.coach_tick_at(29_999).unwrap();
    no_http(&server);
    let served = server.clone();
    let provider = thread::spawn(move || {
        let mut request = served
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("Expected one combined coach request");
        assert_eq!(request.url(), "/api/chat");
        assert_eq!(request.method(), &tiny_http::Method::Post);
        assert!(!request
            .headers()
            .iter()
            .any(|h| h.field.equiv("Authorization")));
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).unwrap();
        let payload: Value = serde_json::from_str(&body).unwrap();
        let answer=json!([
            {"agentId":QUIET,"kind":"question","text":"Who will review the example report?","segmentId":"source-recent","quote":QUOTE},
            {"agentId":SALES,"kind":"follow_up","text":"Consider sending an example report after the call.","segmentId":"source-recent","quote":QUOTE}
        ]).to_string();
        request
            .respond(tiny_http::Response::from_string(format!(
                "{}\n{}\n",
                json!({"message":{"content":answer},"done":false}),
                json!({"done":true,"done_reason":"stop"})
            )))
            .unwrap();
        (payload, body)
    });
    app.coach_tick_at(30_000).unwrap();
    let (payload, body) = provider.join().unwrap();
    assert_eq!(payload["options"]["num_thread"], 2);
    assert_eq!(payload["options"]["num_predict"], 1200);
    assert_eq!(payload["think"], false);
    assert!(payload.get("tools").is_none());
    let prompt: Value =
        serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(prompt["eligibleAgents"].as_array().unwrap().len(), 2);
    assert_eq!(prompt["eligibleAgents"][0]["id"], QUIET);
    assert_eq!(prompt["eligibleAgents"][1]["id"], SALES);
    assert!(prompt["eligibleAgents"][0]["instructions"]
        .as_str()
        .unwrap()
        .contains("unanswered questions"));
    assert!(prompt["eligibleAgents"][1]["instructions"]
        .as_str()
        .unwrap()
        .contains("objections"));
    assert_eq!(prompt["recentEvidence"].as_array().unwrap().len(), 1);
    assert_eq!(prompt["recentEvidence"][0]["text"], WORDS);
    for excluded in [
        "PRIVATE_TITLE_EXCLUDED",
        "PRIVATE_MY_NOTES_EXCLUDED",
        "PRIVATE_PERSON_EXCLUDED",
        "PRIVATE_CHAT_EXCLUDED",
        "STALE_EVIDENCE_EXCLUDED",
    ] {
        assert!(
            !body.contains(excluded),
            "Unexpected automatic context: {excluded}"
        );
    }
    let coached = call(&app, "get_live_coach", json!({"meetingId":id}));
    let hints = coached["suggestions"].as_array().unwrap();
    assert_eq!(hints.len(), 2);
    assert!(hints
        .iter()
        .all(|s| s["segmentId"] == "source-recent" && s["quote"] == QUOTE && s["start"] == 1000.));
    assert!(app.meeting(id).unwrap()["tasks"]
        .as_array()
        .unwrap()
        .is_empty());
    let follow_up = hints.iter().find(|s| s["kind"] == "follow_up").unwrap();
    let save = json!({"meetingId":id,"id":follow_up["id"]});
    call(&app, "save_coach_follow_up", save.clone());
    let saved = call(&app, "save_coach_follow_up", save);
    assert_eq!(saved["meeting"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(saved["meeting"]["tasks"][0]["owner"], "");
    assert_eq!(saved["meeting"]["tasks"][0]["dueDate"], "");
    app.coach_tick_at(120_000).unwrap();
    no_http(&server);
    call(&app, "stop_recording", json!({"meetingId":id}));
    assert_eq!(
        call(&app, "get_live_coach", json!({"meetingId":id}))["enabled"],
        false
    );
    app.coach_tick_at(180_000).unwrap();
    call(&app, "shutdown", json!({}));
    let meeting_id = id.to_owned();
    drop(app);
    drop(store);

    let (restarted, _store) = service(data.path());
    let restored = call(
        &restarted,
        "get_live_coach",
        json!({"meetingId":meeting_id}),
    );
    assert_eq!(restored["enabled"], false);
    assert_eq!(restored["status"], "stopped");
    assert_eq!(restored["suggestions"].as_array().unwrap().len(), 2);
    assert!(restored["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["status"] == "saved" && s["taskId"].is_string()));
    assert_eq!(
        restarted.meeting(&meeting_id).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    restarted.coach_tick_at(999_999).unwrap();
    no_http(&server);
    call(&restarted, "shutdown", json!({}));
}
