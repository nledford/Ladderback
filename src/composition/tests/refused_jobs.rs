use std::sync::PoisonError;

use api::{ErrorCode, MsgArg};
use app::fakes::InMemoryConfigStore;

use super::service_support::*;
use super::*;

#[tokio::test]
async fn a_failed_job_is_refused_by_the_kind_of_its_failure() {
    use FailureReason as F;
    let text = |value: &str| MsgArg::Text(value.to_owned());
    let unavailable = |code, args: Vec<(&'static str, MsgArg)>, detail: Option<&str>| {
        Err(RepositoriesError::Unavailable {
            code,
            args,
            detail: detail.map(str::to_owned),
        })
    };
    for (reason, expected) in [
        (
            F::EngineFailed {
                detail: "wrong password".into(),
            },
            Err(RepositoriesError::OperationFailed {
                detail: "wrong password".into(),
            }),
        ),
        (
            F::EngineUnavailable {
                detail: "no worker".into(),
            },
            unavailable(ErrorCode::EngineUnavailable, vec![], Some("no worker")),
        ),
        (
            F::JobRejected {
                detail: "refused".into(),
            },
            unavailable(ErrorCode::EngineUnavailable, vec![], Some("refused")),
        ),
        (
            F::EngineExited { exit_code: Some(3) },
            unavailable(
                ErrorCode::EngineExited,
                vec![("exit_code", text("3"))],
                None,
            ),
        ),
        (
            F::EngineExited { exit_code: None },
            unavailable(ErrorCode::EngineExited, vec![], None),
        ),
        (
            F::DeadlinePassed,
            unavailable(ErrorCode::DeadlinePassed, vec![], None),
        ),
        (
            F::EngineStartTimedOut,
            unavailable(ErrorCode::EngineStartTimedOut, vec![], None),
        ),
        (
            F::EngineExitTimedOut,
            unavailable(ErrorCode::EngineExitTimedOut, vec![], None),
        ),
        (
            F::DaemonStopped,
            unavailable(ErrorCode::DaemonStopped, vec![], None),
        ),
        (
            F::DaemonShutDown,
            unavailable(ErrorCode::DaemonShutDown, vec![], None),
        ),
        (
            F::SecretsTooLargeToMask {
                field: Some("HTTPS_PROXY".to_owned()),
                limit: 262_144,
            },
            Err(RepositoriesError::SecretRefused(SecretRefusal {
                code: ErrorCode::SecretsTooLargeToMask,
                args: vec![
                    ("limit", MsgArg::Bytes(262_144)),
                    ("field", text("HTTPS_PROXY")),
                ],
                detail: None,
            })),
        ),
        (
            F::RepositoryReplaced,
            unavailable(ErrorCode::RepositoryReplaced, vec![], None),
        ),
        (
            F::CredentialsRefused { half: None },
            unavailable(ErrorCode::CredentialsRefused, vec![], None),
        ),
        (
            F::CredentialsRefused {
                half: Some(domain::execution::RepositoryHalf::Hot),
            },
            unavailable(
                ErrorCode::CredentialsRefused,
                vec![("half", text("hot"))],
                None,
            ),
        ),
        (
            F::RepositoryConfigNotFound {
                detail: "No repository config file found for `/srv/demo/repo`.".into(),
            },
            Err(RepositoriesError::ConfigNotFound {
                detail: "No repository config file found for `/srv/demo/repo`.".into(),
            }),
        ),
        (
            F::RepositoryExists,
            Err(RepositoriesError::Exists {
                location: "/srv/demo/repo".into(),
            }),
        ),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        *engine
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Err(reason.clone());
        let service = service(&engine, config.clone());
        let tested = service
            .test(api::TestRepositoryDto {
                settings: settings_dto(),
            })
            .await;
        assert_eq!(
            tested.map(|_| ()),
            expected.clone().map(|_: ()| ()),
            "{reason:?}"
        );
        assert_eq!(
            service
                .connect(connect("demo", settings_dto()))
                .await
                .map(|_| ()),
            expected.map(|_: ()| ()),
            "{reason:?}"
        );
        assert!(
            saved(&*config).await.repositories().is_empty(),
            "{reason:?} saved something"
        );
    }
}

#[tokio::test]
async fn the_engine_not_starting_is_unavailable() {
    for (error, expected) in [
        (
            EngineError::Unavailable {
                detail: "no such program".into(),
            },
            RepositoriesError::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: vec![],
                detail: Some("no such program".into()),
            },
        ),
        (
            EngineError::StartTimedOut,
            RepositoriesError::Unavailable {
                code: ErrorCode::EngineStartTimedOut,
                args: vec![],
                detail: None,
            },
        ),
    ] {
        let engine = engine();
        *engine
            .refuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error);
        let answer = service(&engine, Arc::new(InMemoryConfigStore::new()))
            .test(api::TestRepositoryDto {
                settings: settings_dto(),
            })
            .await;
        assert_eq!(answer, Err(expected));
    }
}

/// An engine that refuses a job for more secrets than it masks is told
/// by the code of a secret refusal, with the limit and the setting where
/// the secrets passed it, or none when the job's own did (§10.2).
#[tokio::test]
async fn the_engine_refusing_a_job_with_too_much_to_mask_is_told_by_its_code() {
    for field in [Some("HTTPS_PROXY"), None] {
        let engine = engine();
        *engine
            .refuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::SecretsTooLargeToMask {
            field: field.map(str::to_owned),
            limit: 262_144,
        });
        let answer = service(&engine, Arc::new(InMemoryConfigStore::new()))
            .test(api::TestRepositoryDto {
                settings: settings_dto(),
            })
            .await;
        let mut args = vec![("limit", MsgArg::Bytes(262_144))];
        args.extend(field.map(|field| ("field", MsgArg::Text(field.to_owned()))));
        assert_eq!(
            answer,
            Err(RepositoriesError::SecretRefused(SecretRefusal {
                code: ErrorCode::SecretsTooLargeToMask,
                args,
                detail: None,
            })),
            "{field:?}"
        );
    }
}

#[tokio::test]
async fn an_existing_repository_is_refused_with_its_location_redacted() {
    let engine = engine();
    *engine
        .outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::RepositoryExists);
    let mut settings = settings_dto();
    settings.repository = "rest:https://backup:hunter2@nas.example/repo".to_owned();
    let Err(RepositoriesError::Exists { location }) =
        service(&engine, Arc::new(InMemoryConfigStore::new()))
            .init(init(None, settings))
            .await
    else {
        panic!("an existing repository is refused as one");
    };
    assert!(!location.contains("hunter2"), "{location}");
    assert!(location.contains("nas.example/repo"), "{location}");
}

/// A job that finds no repository at the location is refused as
/// `repository-not-found`, not as a failure in the engine's words, by a
/// test and a connection alike (decision 53).
#[tokio::test]
async fn a_location_with_no_repository_is_refused_as_one() {
    let engine = engine();
    *engine
        .outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::RepositoryNotFound);
    let service = service(&engine, Arc::new(InMemoryConfigStore::new()));
    assert_eq!(
        service
            .test(api::TestRepositoryDto {
                settings: settings_dto(),
            })
            .await,
        Err(RepositoriesError::NotFound)
    );
    assert_eq!(
        service.connect(connect("demo", settings_dto())).await,
        Err(RepositoriesError::NotFound)
    );
}

#[tokio::test]
async fn an_existing_rclone_repository_is_refused_without_the_credential_in_its_location() {
    let engine = engine();
    *engine
        .outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::RepositoryExists);
    let mut settings = settings_dto();
    settings.repository =
        "rclone::sftp,host=nas.example.com,pass=CANARY-rclone-exists:/backups".to_owned();
    let Err(RepositoriesError::Exists { location }) =
        service(&engine, Arc::new(InMemoryConfigStore::new()))
            .init(init(None, settings))
            .await
    else {
        panic!("an existing repository is refused as one");
    };
    assert_eq!(
        location,
        "rclone::sftp,host=nas.example.com,pass=***:/backups"
    );
}
