//! The routes for rustic profiles (technical design §4.6, §6.2): what
//! importing a profile given as text would add, told without saving
//! anything or reading any file, and a plan as the profile the stock
//! `rustic` reads. Both are reads: neither changes the configuration.

use async_trait::async_trait;
#[cfg(test)]
use axum::http::StatusCode;

/// What the profile routes ask of the daemon, each from a request as it
/// came, before anything in it was read as a domain value.
#[async_trait]
pub trait Profiles: Send + Sync {
    /// What importing the profile whose TOML is the request's `content`
    /// under the alias `repository` would add, checked as an import checks
    /// it. Saves nothing and reads no file: the profiles it uses are
    /// notices.
    async fn preview(
        &self,
        request: api::ProfilePreviewDto,
    ) -> Result<api::ProfileImportDto, ProfilesError>;

    /// The plan whose ID is `plan` as a profile the stock `rustic` reads,
    /// and what was left out of it.
    async fn export(&self, plan: String) -> Result<api::ProfileExportDto, ProfilesError>;
}

/// Why a profile request was refused. None holds a secret: a refusal names
/// where in the profile it is, never a value (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfilesError {
    /// The body isn't a preview request.
    InvalidBody,
    /// The configuration has no plan with this ID.
    PlanUnknown { plan: String },
    /// The profile, or the plan, can't be carried over: `code` says why,
    /// with the arguments it declares and the reader's own words.
    Refused {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
    /// The configuration can't be read or used just now: its `config-*`
    /// code, arguments and the store's words.
    Unavailable {
        code: api::ErrorCode,
        args: Vec<(&'static str, api::MsgArg)>,
        detail: Option<String>,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use axum::body::Body;
    use axum::http::{Method, Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::request_checks::tests::{NoUi, test_state};
    use crate::{RequestChecks, router};

    /// Answers each route with its scripted answer, and remembers what each
    /// was asked.
    struct Scripted {
        preview: Result<api::ProfileImportDto, ProfilesError>,
        export: Result<api::ProfileExportDto, ProfilesError>,
        asked: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn answering(error: ProfilesError) -> Self {
            Self {
                preview: Err(error.clone()),
                export: Err(error),
                asked: Mutex::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<String> {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl Profiles for Scripted {
        async fn preview(
            &self,
            request: api::ProfilePreviewDto,
        ) -> Result<api::ProfileImportDto, ProfilesError> {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(format!("{}:{}", request.repository, request.content));
            self.preview.clone()
        }

        async fn export(&self, plan: String) -> Result<api::ProfileExportDto, ProfilesError> {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(plan);
            self.export.clone()
        }
    }

    async fn send(profiles: Arc<Scripted>, request: Request<Body>) -> (StatusCode, String) {
        let mut state = test_state();
        state.profiles = profiles;
        let response = router(Arc::new(NoUi), RequestChecks::default(), state)
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn preview(body: String) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/api/v1/profiles/preview")
            .header(header::HOST, "127.0.0.1:8080")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    fn export(plan: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/api/v1/plans/{plan}/profile"))
            .header(header::HOST, "127.0.0.1:8080")
            .body(Body::empty())
            .unwrap()
    }

    /// A profile at the 1 MiB cap whose every character JSON escapes still
    /// reaches the route whole, far over the API's own body limit.
    #[tokio::test]
    async fn a_preview_takes_a_profile_at_its_cap_written_as_json_and_answers_the_import() {
        let imported = api::ProfileImportDto {
            repository: "nas".to_owned(),
            plans: vec!["home".to_owned()],
            notices: Vec::new(),
        };
        let profiles = Arc::new(Scripted {
            preview: Ok(imported.clone()),
            ..Scripted::answering(ProfilesError::InvalidBody)
        });
        let content = "\"".repeat(1024 * 1024);
        let body = serde_json::json!({ "content": content, "repository": "nas" }).to_string();
        let (status, answer) = send(Arc::clone(&profiles), preview(body)).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(
            serde_json::from_str::<api::ProfileImportDto>(&answer).unwrap(),
            imported
        );
        assert_eq!(profiles.asked(), [format!("nas:{content}")]);
    }

    #[tokio::test]
    async fn a_preview_body_that_is_not_a_preview_request_is_refused_unread() {
        let profiles = Arc::new(Scripted::answering(ProfilesError::PlanUnknown {
            plan: "unused".to_owned(),
        }));
        for body in [
            r#"{"content":"","repository":"nas","file":"/etc/passwd"}"#,
            r#"{"content":""}"#,
            "not json",
        ] {
            let (status, answer) = send(Arc::clone(&profiles), preview(body.to_owned())).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(answer, r#"{"code":"invalid-body"}"#, "{body}");
        }
        assert_eq!(profiles.asked(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn an_export_answers_the_profile_of_the_plan_the_path_names() {
        let exported = api::ProfileExportDto {
            text: "[repository]\n".to_owned(),
            notices: Vec::new(),
        };
        let profiles = Arc::new(Scripted {
            export: Ok(exported.clone()),
            ..Scripted::answering(ProfilesError::InvalidBody)
        });
        let (status, answer) = send(Arc::clone(&profiles), export("home")).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        assert_eq!(
            serde_json::from_str::<api::ProfileExportDto>(&answer).unwrap(),
            exported
        );
        assert_eq!(profiles.asked(), ["home"]);
    }

    #[tokio::test]
    async fn each_refusal_is_answered_with_the_status_of_its_kind() {
        let config = ProfilesError::Unavailable {
            code: api::ErrorCode::ConfigUnavailable,
            args: vec![("file", api::MsgArg::Text("config.toml".to_owned()))],
            detail: Some("denied".to_owned()),
        };
        let secret = ProfilesError::Refused {
            code: api::ErrorCode::InlineSecret,
            args: Vec::new(),
            detail: None,
        };
        let unknown = ProfilesError::PlanUnknown {
            plan: "gone".to_owned(),
        };
        for (error, status, answer) in [
            (
                config,
                StatusCode::SERVICE_UNAVAILABLE,
                r#"{"code":"config-unavailable","args":{"file":{"type":"text","value":"config.toml"}},"detail":"denied"}"#,
            ),
            (
                secret,
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"code":"inline-secret"}"#,
            ),
            (
                unknown,
                StatusCode::NOT_FOUND,
                r#"{"code":"plan-unknown","args":{"plan":{"type":"text","value":"gone"}}}"#,
            ),
        ] {
            let profiles = Arc::new(Scripted::answering(error));
            assert_eq!(
                send(Arc::clone(&profiles), export("home")).await,
                (status, answer.to_owned())
            );
            let body = r#"{"content":"","repository":"nas"}"#.to_owned();
            assert_eq!(
                send(profiles, preview(body)).await,
                (status, answer.to_owned())
            );
        }
    }
}
