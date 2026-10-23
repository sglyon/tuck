use super::*;

#[test]
fn publication_choices_are_durable_and_only_selected_text_reaches_the_port() {
    for store in stores() {
        let r = rig(store.clone());
        let id = create(&r);
        call(
            &r.service,
            "update_meeting",
            json!({"id":id,"changes":{
                "notes":"Private personal notes", "summary":"Publish this recap"
            }}),
        );
        call(
            &r.service,
            "add_task",
            json!({"meetingId":id,"title":"Private action"}),
        );
        call(
            &r.service,
            "update_settings",
            json!({"changes":{
                "notionTarget":"database-id", "notionContent":{"recap":true,"notes":false,"tasks":false}
            }}),
        );
        r.service.publish_notion(&id).unwrap();
        let sent = r.publisher.published.lock().unwrap()[0].clone();
        assert_eq!(sent["summary"], "Publish this recap");
        assert_eq!(sent["notes"], "");
        assert_eq!(sent["tasks"], json!([]));
        assert!(sent.get("segments").is_none());
        assert!(sent.get("messages").is_none());
        assert!(!sent.to_string().contains("Private"));
        assert_eq!(
            r.service.meeting(&id).unwrap()["notes"],
            "Private personal notes"
        );
        let reopened = rig(store);
        assert_eq!(
            reopened.service.get_settings().unwrap()["notionContent"],
            json!({"recap":true,"notes":false,"tasks":false})
        );
    }
}

#[test]
fn empty_or_malformed_publication_choices_do_not_start_an_upload() {
    for store in stores() {
        let r = rig(store);
        let id = create(&r);
        for content in [
            json!(null),
            json!({}),
            json!({"recap":true,"notes":true}),
            json!({"recap":true,"notes":true,"tasks":"yes"}),
            json!({"recap":true,"notes":true,"tasks":true,"audio":true}),
        ] {
            assert!(r
                .service
                .handle(
                    "update_settings",
                    json!({"changes":{"notionContent":content}})
                )
                .is_err());
        }
        let none = json!({"recap":false,"notes":false,"tasks":false});
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":"database-id","notionContent":none}}),
        );
        assert!(r
            .service
            .handle(
                "update_settings",
                json!({"changes":{"notionAutoSync":true}})
            )
            .is_err());
        assert!(r.service.publish_notion(&id).is_err());
        assert_eq!(*r.publisher.calls.lock().unwrap(), 0);
        assert_eq!(r.service.meeting(&id).unwrap()["notionStatus"], "idle");
        let defaults = domain::publication_snapshot(&json!({"id":"id","summary":"Recap","notes":"Notes","tasks":[{"title":"Action"}],"segments":[{"text":"Raw speech"}],"audio":"bytes","messages":[{"text":"Chat"}]}), &json!({})).unwrap();
        assert_eq!(defaults["notes"], "Notes");
        assert_eq!(defaults["summary"], "Recap");
        assert_eq!(defaults["tasks"].as_array().unwrap().len(), 1);
        assert!(defaults.get("audio").is_none());
        assert!(defaults.get("segments").is_none());
        assert!(defaults.get("messages").is_none());
    }
}
