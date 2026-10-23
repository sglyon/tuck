use super::*;

const PARENT: &str = "00000000-0000-4000-8000-000000000001";
const DATABASE: &str = "00000000-0000-4000-8000-000000000002";
const NEW_DATABASE: &str = "00000000-0000-4000-8000-000000000003";

fn connect(r: &Rig) {
    call(
        &r.service,
        "save_secret",
        json!({"kind":"notion","value":"test-notion-token"}),
    );
}

#[test]
fn setup_selects_consistent_mapping_without_enabling_uploads_and_survives_reopen() {
    for store in stores() {
        let r = rig(store.clone());
        assert!(r.service.create_notion_database(PARENT).is_err());
        assert!(r.publisher.provisioned.lock().unwrap().is_empty());
        connect(&r);
        assert!(r.service.create_notion_database("not a page id").is_err());
        let result = call(
            &r.service,
            "notion_create_database",
            json!({"parent":PARENT}),
        );
        let settings = &result["settings"];
        assert_eq!(settings["notionTarget"], DATABASE);
        assert_eq!(
            settings["notionPropertyMapping"],
            result["destination"]["mapping"]
        );
        assert_eq!(settings["notionAutoSync"], false);
        assert_eq!(settings["notionSetup"]["status"], "ready");
        assert_eq!(settings["notionSetup"]["destination"]["id"], DATABASE);
        assert!(settings.get("_notionSetup").is_none());
        assert!(settings["notionSetup"].get("attempt").is_none());
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true)]
        );
        assert_eq!(*r.publisher.calls.lock().unwrap(), 0);

        let reopened = rig(store);
        connect(&reopened);
        let saved = reopened.service.get_settings().unwrap();
        assert_eq!(saved["notionTarget"], settings["notionTarget"]);
        assert_eq!(
            saved["notionPropertyMapping"],
            settings["notionPropertyMapping"]
        );
        reopened.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *reopened.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), false)]
        );
    }
}

#[test]
fn setup_journal_is_read_only_and_cannot_be_forged_or_cleared_by_preferences() {
    for store in stores() {
        let r = rig(store.clone());
        connect(&r);
        r.service.create_notion_database(PARENT).unwrap();
        let before = tx(&*store, |repository| repository.settings()).unwrap();
        for changes in [
            json!({"_notionSetup":null}),
            json!({"_notionSetup":{"parent":PARENT,"status":"ready","attempt":"forged"}}),
            json!({"notionSetup":null}),
            json!({"notionSetup":{"status":"creating"}}),
        ] {
            assert!(r
                .service
                .handle("update_settings", json!({"changes":changes}))
                .is_err());
        }
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"threads":3,"notionContent":{"recap":false,"notes":true,"tasks":false}}}),
        );
        let after = tx(&*store, |repository| repository.settings()).unwrap();
        assert_eq!(after["_notionSetup"], before["_notionSetup"]);
        let public = r.service.get_settings().unwrap();
        assert!(public.get("_notionSetup").is_none());
        assert!(public["notionSetup"].get("attempt").is_none());
        assert!(!public.to_string().contains("test-notion-token"));
    }
}

#[test]
fn lost_creation_response_only_reconciles_after_settings_edits_and_reopen() {
    for store in stores() {
        let r = rig(store.clone());
        connect(&r);
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_setup_uncertain",
                "The response was lost after sending creation.",
            )));
        assert_eq!(
            r.service.create_notion_database(PARENT).unwrap_err().code,
            "notion_setup_uncertain"
        );
        assert_eq!(
            r.service.get_settings().unwrap()["notionSetup"]["status"],
            "uncertain"
        );
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"threads":4,"notionTarget":"","notionPropertyMapping":{}}}),
        );

        let reopened = rig(store);
        connect(&reopened);
        assert!(reopened
            .service
            .create_notion_database(NEW_DATABASE)
            .is_err());
        assert!(reopened.publisher.provisioned.lock().unwrap().is_empty());
        reopened
            .publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_setup_not_found",
                "No existing database is visible yet.",
            )));
        assert_eq!(
            reopened
                .service
                .create_notion_database(PARENT)
                .unwrap_err()
                .code,
            "notion_setup_not_found"
        );
        assert_eq!(
            reopened.service.get_settings().unwrap()["notionSetup"]["status"],
            "uncertain"
        );
        reopened.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true)]
        );
        assert_eq!(
            *reopened.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), false), (PARENT.into(), false)]
        );
    }
}

#[test]
fn known_prewrite_failure_allows_an_explicit_creation_retry() {
    for store in stores() {
        let r = rig(store);
        connect(&r);
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_parent_unavailable",
                "The parent is not shared; no write was attempted.",
            )));
        assert_eq!(
            r.service.create_notion_database(PARENT).unwrap_err().code,
            "notion_parent_unavailable"
        );
        assert!(r.service.get_settings().unwrap()["notionSetup"].is_null());
        r.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true), (PARENT.into(), true)]
        );
    }
}

#[test]
fn delayed_setup_never_overwrites_a_new_destination_or_connection() {
    for (connection_changed, selection_changed) in [(false, true), (true, false), (true, true)] {
        for store in stores() {
            let r = rig(store);
            connect(&r);
            let service = Arc::downgrade(&r.service);
            *r.publisher.provision_hook.lock().unwrap() = Some(Box::new(move || {
                let service = service.upgrade().unwrap();
                if connection_changed {
                    service.reset_notion_destination().unwrap();
                }
                if selection_changed {
                    call(
                        &service,
                        "update_settings",
                        json!({"changes":{"notionTarget":NEW_DATABASE,"notionPropertyMapping":{"date":"New meeting date"}}}),
                    );
                }
            }));
            assert_eq!(
                r.service.create_notion_database(PARENT).unwrap_err().code,
                "notion_setup_changed"
            );
            let settings = r.service.get_settings().unwrap();
            assert_eq!(
                settings["notionTarget"],
                if selection_changed { NEW_DATABASE } else { "" }
            );
            assert_eq!(
                settings["notionPropertyMapping"],
                if selection_changed {
                    json!({"date":"New meeting date"})
                } else {
                    json!({})
                }
            );
            assert_eq!(settings["notionAutoSync"], false);
            assert_eq!(settings["notionSetup"]["status"], "creating");
            // A late successful response is still an uncertain remote creation. A retry
            // reconciles it instead of issuing another create request.
            *r.publisher.provision_hook.lock().unwrap() = None;
            r.service.create_notion_database(PARENT).unwrap();
            assert_eq!(
                r.publisher.provisioned.lock().unwrap().last(),
                Some(&(PARENT.into(), false))
            );
        }
    }
}

#[test]
fn portable_backup_excludes_the_device_creation_journal() {
    for store in stores() {
        let r = rig(store);
        connect(&r);
        r.service.create_notion_database(PARENT).unwrap();
        call(
            &r.service,
            "backup_library",
            json!({"path":"/fixture/setup-backup.zip","includeAudio":false}),
        );
        let manifest = r.archive.backup.lock().unwrap().clone().unwrap();
        let settings = &manifest["settings"];
        assert!(settings.get("_notionSetup").is_none());
        assert!(settings.get("notionSetup").is_none());
        assert_eq!(settings["notionTarget"], "");
        assert_eq!(settings["notionPropertyMapping"], json!({}));
        assert_eq!(settings["notionAutoSync"], false);
        assert!(!manifest.to_string().contains("test-notion-token"));
    }
}

#[test]
fn restoring_safe_preferences_retains_an_uncertain_device_creation_journal() {
    for store in stores() {
        let r = rig(store);
        connect(&r);
        call(
            &r.service,
            "backup_library",
            json!({"path":"/fixture/before-setup.zip","includeAudio":false}),
        );
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"threads":4}}),
        );
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_setup_uncertain",
                "The response was lost after sending creation.",
            )));
        assert!(r.service.create_notion_database(PARENT).is_err());
        let preview = call(
            &r.service,
            "preview_restore_library",
            json!({"path":"/fixture/before-setup.zip"}),
        );
        assert_eq!(preview["settingsMode"], "safe-preferences");
        // Preview must neither drop the journal nor permit another remote creation.
        assert_eq!(
            r.service.get_settings().unwrap()["notionSetup"]["status"],
            "uncertain"
        );
        call(
            &r.service,
            "restore_library",
            json!({"path":"/fixture/before-setup.zip","expectedFingerprint":preview["fingerprint"]}),
        );
        let settings = r.service.get_settings().unwrap();
        assert_eq!(settings["threads"], 2);
        assert_eq!(settings["notionTarget"], "");
        assert_eq!(settings["notionAutoSync"], false);
        assert_eq!(settings["notionSetup"]["status"], "uncertain");
        assert_eq!(settings["notionSetup"]["parent"], PARENT);
        r.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true), (PARENT.into(), false)]
        );
    }
}

#[test]
fn restoring_safe_preferences_invalidates_a_delayed_creation_response() {
    for store in stores() {
        let r = rig(store);
        connect(&r);
        call(
            &r.service,
            "backup_library",
            json!({"path":"/fixture/before-setup.zip","includeAudio":false}),
        );
        let service = Arc::downgrade(&r.service);
        *r.publisher.provision_hook.lock().unwrap() = Some(Box::new(move || {
            call(
                &service.upgrade().unwrap(),
                "restore_library",
                json!({"path":"/fixture/before-setup.zip","expectedFingerprint":"a".repeat(64)}),
            );
        }));
        assert_eq!(
            r.service.create_notion_database(PARENT).unwrap_err().code,
            "notion_setup_changed"
        );
        let settings = r.service.get_settings().unwrap();
        assert_eq!(settings["notionTarget"], "");
        assert_eq!(settings["notionPropertyMapping"], json!({}));
        assert_eq!(settings["notionSetup"]["parent"], PARENT);
        assert!(matches!(
            settings["notionSetup"]["status"].as_str(),
            Some("creating" | "uncertain")
        ));
        *r.publisher.provision_hook.lock().unwrap() = None;
        r.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true), (PARENT.into(), false)]
        );
    }
}

#[test]
fn finishing_layout_requires_the_verified_parent_and_current_destination() {
    for store in stores() {
        let r = rig(store);
        assert!(r.service.finish_notion_database(PARENT).is_err());
        connect(&r);
        assert!(r.service.finish_notion_database(PARENT).is_err());
        assert!(r.publisher.finished.lock().unwrap().is_empty());
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_setup_uncertain",
                "Creation response was lost.",
            )));
        assert!(r.service.create_notion_database(PARENT).is_err());
        assert!(r.service.finish_notion_database(PARENT).is_err());
        r.service.create_notion_database(PARENT).unwrap();
        assert!(r.service.finish_notion_database(NEW_DATABASE).is_err());
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionTarget":NEW_DATABASE,"notionPropertyMapping":{}}}),
        );
        assert!(r.service.finish_notion_database(PARENT).is_err());
        assert!(r.publisher.finished.lock().unwrap().is_empty());
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true), (PARENT.into(), false)]
        );
    }
}

#[test]
fn finishing_layout_has_an_explicit_port_and_preserves_ready_database_on_failure() {
    for store in stores() {
        let r = rig(store);
        connect(&r);
        let mut destination = r.publisher.provision_result().unwrap();
        destination["setupWarning"] =
            json!("The database is ready; its table layout is unfinished.");
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Ok(destination.clone()));
        r.service.create_notion_database(PARENT).unwrap();
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Err(AppError::new(
                "notion_layout_incomplete",
                "The layout could not be saved. Your database is still ready.",
            )));
        assert_eq!(
            r.service.finish_notion_database(PARENT).unwrap_err().code,
            "notion_layout_incomplete"
        );
        let settings = r.service.get_settings().unwrap();
        assert_eq!(settings["notionTarget"], DATABASE);
        assert_eq!(settings["notionSetup"]["status"], "ready");
        assert_eq!(
            settings["notionSetup"]["message"],
            "The layout could not be saved. Your database is still ready."
        );
        assert_eq!(settings["notionAutoSync"], false);
        assert_eq!(
            *r.publisher.finished.lock().unwrap(),
            vec![PARENT.to_owned()]
        );
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true)]
        );

        // Checking after a failed layout finish stays read-only. It does not call
        // the explicit finishing port or regain permission to create a database.
        r.publisher
            .provision_outcomes
            .lock()
            .unwrap()
            .push(Ok(destination));
        r.service.create_notion_database(PARENT).unwrap();
        assert_eq!(
            *r.publisher.finished.lock().unwrap(),
            vec![PARENT.to_owned()]
        );
        assert_eq!(
            *r.publisher.provisioned.lock().unwrap(),
            vec![(PARENT.into(), true), (PARENT.into(), false)]
        );
        let custom_mapping = json!({"date":"My meeting date","folder":"","tags":"My topics","people":"Attendees","meetingId":"My reference"});
        call(
            &r.service,
            "update_settings",
            json!({"changes":{"notionPropertyMapping":custom_mapping}}),
        );
        let finished = call(
            &r.service,
            "notion_finish_database",
            json!({"parent":PARENT}),
        );
        assert_eq!(finished["settings"]["notionSetup"]["status"], "ready");
        assert!(finished["settings"]["notionSetup"]["message"].is_null());
        assert_eq!(finished["settings"]["notionTarget"], DATABASE);
        assert_eq!(
            finished["settings"]["notionPropertyMapping"],
            custom_mapping
        );
        assert_eq!(
            *r.publisher.finished.lock().unwrap(),
            vec![PARENT.to_owned(), PARENT.to_owned()]
        );
        assert_eq!(r.publisher.provisioned.lock().unwrap().len(), 2);
    }
}

#[test]
fn finishing_layout_never_selects_a_different_or_delayed_destination() {
    for concurrent_selection in [false, true] {
        for store in stores() {
            let r = rig(store);
            connect(&r);
            r.service.create_notion_database(PARENT).unwrap();
            if concurrent_selection {
                let service = Arc::downgrade(&r.service);
                *r.publisher.finish_hook.lock().unwrap() = Some(Box::new(move || {
                    call(
                        &service.upgrade().unwrap(),
                        "update_settings",
                        json!({"changes":{
                            "notionTarget":NEW_DATABASE,"notionPropertyMapping":{"date":"New date"}
                        }}),
                    );
                }));
            } else {
                let mut wrong_destination = r.publisher.provision_result().unwrap();
                wrong_destination["id"] = json!(NEW_DATABASE);
                r.publisher
                    .provision_outcomes
                    .lock()
                    .unwrap()
                    .push(Ok(wrong_destination));
            }
            let result = r.service.finish_notion_database(PARENT);
            assert!(result.is_err());
            if concurrent_selection {
                assert_eq!(result.unwrap_err().code, "notion_setup_changed");
            }
            let settings = r.service.get_settings().unwrap();
            assert_eq!(
                settings["notionTarget"],
                if concurrent_selection {
                    NEW_DATABASE
                } else {
                    DATABASE
                }
            );
            if concurrent_selection {
                assert_eq!(
                    settings["notionPropertyMapping"],
                    json!({"date":"New date"})
                );
            }
            assert_eq!(settings["notionSetup"]["destination"]["id"], DATABASE);
            assert_eq!(
                *r.publisher.finished.lock().unwrap(),
                vec![PARENT.to_owned()]
            );
            assert_eq!(
                *r.publisher.provisioned.lock().unwrap(),
                vec![(PARENT.into(), true)]
            );
        }
    }
}
