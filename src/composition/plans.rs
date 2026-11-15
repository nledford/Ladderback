//! The plan routes' side of the wiring (technical design §4.2, decision 96):
//! a plan's table from the Plans page, read by the config adapter as the
//! file is read when it loads, then created, changed or deleted by the use
//! case of `app`, and each refusal said as the routes answer it: at the
//! plan's setting, never with a value.

use std::sync::Arc;

use api::PlanProblem;
use app::plans::EditPlans;
use app::ports::ConfigStore;
use async_trait::async_trait;
use http_adapter::{Plans, PlansError};

/// The plans of the configuration `config`, edited by `edits`.
pub(super) struct PlanService {
    config: Arc<dyn ConfigStore>,
    edits: EditPlans,
}

impl PlanService {
    pub(super) fn new(config: Arc<dyn ConfigStore>, edits: EditPlans) -> Self {
        Self { config, edits }
    }
}

#[async_trait]
impl Plans for PlanService {
    async fn plan(&self, id: String) -> Result<api::PlanDto, PlansError> {
        let _ = (&self.config, &self.edits, id, PlanProblem::Invalid);
        todo!()
    }

    async fn create(&self, plan: api::PlanDto) -> Result<api::PlanDto, PlansError> {
        let _ = plan;
        todo!()
    }

    async fn change(&self, id: String, plan: api::PlanDto) -> Result<api::PlanDto, PlansError> {
        let _ = (id, plan);
        todo!()
    }

    async fn delete(&self, id: String) -> Result<(), PlansError> {
        let _ = id;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use app::repositories::ConfigWrites;
    use app::schedule::config_changes;
    use config_adapter::FileConfigStore;
    use platform::SystemClock;
    use serde_json::{Value, json};

    use super::*;

    const CONFIG: &str = r#"schema = 1

[repositories.demo]
repository = "/srv/demo-repo"
password-env = "DEMO_PASSWORD"

[[plans]]
id = "home"
repository = "demo"
sources = ["/home/demo"]

[notifications.channels.desk]
kind = "ntfy"
url-env = "DEMO_NTFY_URL"

[notifications.rules.failures]
events = ["backup.failed"]
channel = "desk"
plans = ["home"]
"#;

    /// The service over a file holding `CONFIG`, and the file.
    fn service() -> (PlanService, tempfile::TempDir, std::path::PathBuf) {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let file = scratch.path().join("config.toml");
        std::fs::write(&file, CONFIG).expect("the config file");
        let config: Arc<dyn ConfigStore> =
            Arc::new(FileConfigStore::new(file.clone(), Arc::new(SystemClock)));
        let edits = EditPlans::new(
            Arc::clone(&config),
            Arc::new(ConfigWrites::default()),
            config_changes().0,
        );
        (PlanService::new(config, edits), scratch, file)
    }

    fn table(value: Value) -> api::PlanDto {
        let Value::Object(table) = value else {
            panic!("a JSON object")
        };
        api::PlanDto(table)
    }

    fn invalid(field: &str, problem: PlanProblem) -> PlansError {
        PlansError::Invalid {
            field: field.to_owned(),
            problem,
        }
    }

    #[tokio::test]
    async fn a_plan_created_is_answered_as_the_file_writes_it() {
        let (plans, _scratch, file) = service();
        let sent = json!({"id": "photos", "repository": "demo", "sources": ["/home/demo/Pictures"], "schedule": {"cron": "0 3 * * *"}});
        let saved = plans.create(table(sent.clone())).await.expect("created");
        assert_eq!(Value::Object(saved.0), sent);
        let written = std::fs::read_to_string(file).expect("the file");
        assert!(written.contains("id = \"photos\""), "{written}");
    }

    #[tokio::test]
    async fn a_plan_is_refused_at_its_setting_as_the_loader_refuses_it() {
        let (plans, _scratch, file) = service();
        let base = json!({"id": "photos", "repository": "demo", "sources": ["/p"]});
        let with = |key: &str, value: Value| {
            let mut sent = base.clone();
            sent[key] = value;
            table(sent)
        };
        for (sent, expected) in [
            (
                with("schedule", json!({"cron": "61 * * * *"})),
                invalid("schedule.cron", PlanProblem::Invalid),
            ),
            (
                with("colour", json!("red")),
                invalid("colour", PlanProblem::UnknownKey),
            ),
            (
                with("sources", json!("/p")),
                invalid("sources", PlanProblem::WrongType),
            ),
            (
                table(json!({"id": "photos", "sources": ["/p"]})),
                invalid("repository", PlanProblem::Missing),
            ),
            (with("id", json!("home")), invalid("id", PlanProblem::Taken)),
            (
                with("repository", json!("nas")),
                invalid("repository", PlanProblem::UnknownRepository),
            ),
            (
                with("sources", json!([])),
                invalid("sources", PlanProblem::NoSources),
            ),
            (with("label", Value::Null), PlansError::InvalidBody),
        ] {
            assert_eq!(plans.create(sent).await, Err(expected));
        }
        assert_eq!(std::fs::read_to_string(file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn a_change_takes_the_paths_id_and_refuses_another() {
        let (plans, _scratch, _file) = service();
        let sent = json!({"repository": "demo", "sources": ["/home/demo/Documents"]});
        let saved = plans
            .change("home".to_owned(), table(sent))
            .await
            .expect("changed");
        assert_eq!(saved.0["id"], "home");
        assert_eq!(saved.0["sources"], json!(["/home/demo/Documents"]));
        let renamed = json!({"id": "away", "repository": "demo", "sources": ["/p"]});
        assert_eq!(
            plans.change("home".to_owned(), table(renamed)).await,
            Err(invalid("id", PlanProblem::Invalid))
        );
    }

    #[tokio::test]
    async fn a_plan_the_configuration_does_not_hold_is_unknown() {
        let (plans, _scratch, _file) = service();
        let sent = || table(json!({"repository": "demo", "sources": ["/p"]}));
        for id in ["gone", "Not An ID"] {
            let unknown = Err(PlansError::Unknown {
                plan: id.to_owned(),
            });
            assert_eq!(plans.plan(id.to_owned()).await, unknown.clone());
            assert_eq!(plans.change(id.to_owned(), sent()).await, unknown.clone());
            assert_eq!(
                plans.delete(id.to_owned()).await,
                unknown.map(|_: api::PlanDto| ())
            );
        }
    }

    #[tokio::test]
    async fn a_plan_a_rule_names_is_not_deleted() {
        let (plans, _scratch, file) = service();
        assert_eq!(
            plans.delete("home".to_owned()).await,
            Err(PlansError::InUse {
                plan: "home".to_owned(),
                rule: "failures".to_owned(),
            })
        );
        assert_eq!(std::fs::read_to_string(file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn a_plan_is_read_as_the_file_writes_it() {
        let (plans, _scratch, _file) = service();
        let shown = plans.plan("home".to_owned()).await.expect("the plan");
        assert_eq!(
            Value::Object(shown.0),
            json!({"id": "home", "repository": "demo", "sources": ["/home/demo"]})
        );
    }
}
