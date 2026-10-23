use super::*;

const JOURNAL: &str = "_notionSetup";
const CHECK_MESSAGE: &str = "Your database may have been created. Check for it before creating another; Tuck will only look for an existing database.";

impl MeetingService {
    /// Credential changes invalidate any delayed setup response without forgetting an
    /// uncertain remote creation. The journal never contains credentials or notes.
    pub fn reset_notion_destination(&self) -> Result<()> {
        self.transaction(|r| {
            let next = domain::updated_settings(
                &r.settings()?,
                &json!({
                    "notionTarget":"", "notionPropertyMapping":{}, "notionAutoSync":false
                }),
            )?;
            r.save_settings(&next)?;
            Ok(Value::Null)
        })?;
        Ok(())
    }

    pub fn create_notion_database(&self, parent: &str) -> Result<Value> {
        let parent = uuid::Uuid::parse_str(parent)
            .map_err(|_| AppError::invalid("Choose a shared Notion page for your database."))?
            .to_string();
        if !self.credential_flag("notion") {
            return Err(AppError::invalid(
                "Connect Notion before creating your meeting database.",
            ));
        }
        let _operation = self.operation("notion-setup", "database setup")?;
        let attempt = domain::id();
        let mut allow_create = false;
        self.transaction(|r| {
            let mut settings = r.settings()?;
            let prior = &settings[JOURNAL];
            let pending = matches!(prior["status"].as_str(), Some("creating" | "uncertain"));
            if pending && prior["parent"] != parent {
                return Err(AppError::invalid("Check the database setup already in progress before choosing a different page. You can also use an existing database."));
            }
            allow_create = !pending && !(prior["status"] == "ready" && prior["parent"] == parent);
            settings[JOURNAL] = json!({
                "parent":parent, "status":if allow_create {"creating"} else {"uncertain"},
                "attempt":attempt, "message":CHECK_MESSAGE
            });
            r.save_settings(&settings)?;
            Ok(Value::Null)
        })?;

        let destination = match self.adapters.publisher.provision(&parent, allow_create) {
            Ok(value) => value,
            Err(error) => {
                self.transaction(|r| {
                    let mut settings = r.settings()?;
                    if settings[JOURNAL]["attempt"] == attempt {
                        if allow_create && error.code != "notion_setup_uncertain" {
                            settings[JOURNAL] = Value::Null;
                        } else {
                            settings[JOURNAL]["status"] = json!("uncertain");
                            settings[JOURNAL]["message"] = json!(CHECK_MESSAGE);
                        }
                        r.save_settings(&settings)?;
                    }
                    Ok(Value::Null)
                })?;
                return Err(error);
            }
        };
        // A successful remote write can outlive a credential/destination change.
        // Never select its old destination in the new workspace.
        let saved = self.transaction(|r| {
            let current = r.settings()?;
            if current[JOURNAL]["attempt"] != attempt {
                return Err(AppError::new("notion_setup_changed", "Your Notion connection or destination changed during setup. Check for the database again before selecting it."));
            }
            let target = domain::text(&destination["id"], "Created database", 128, false)?;
            uuid::Uuid::parse_str(target).map_err(|_| AppError::new("notion_setup_uncertain", CHECK_MESSAGE))?;
            let mut next = domain::updated_settings(&current, &json!({
                "notionTarget":target, "notionPropertyMapping":destination["mapping"]
            }))?;
            next[JOURNAL] = json!({
                "parent":parent, "status":"ready", "attempt":attempt,
                "destination":{"id":target,"title":destination["title"],"url":destination["url"]},
                "message":destination["setupWarning"]
            });
            r.save_settings(&next)?;
            Ok(Value::Null)
        });
        saved?;
        Ok(json!({"destination":destination,"settings":self.get_settings()?}))
    }
}
