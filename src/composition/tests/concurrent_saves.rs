use std::sync::PoisonError;

use api::ErrorCode;
use app::fakes::InMemoryConfigStore;
use domain::RepositoryId;

use super::service_support::*;
use super::*;

#[tokio::test]
async fn two_connections_made_at_the_same_moment_are_both_kept() {
    let engine = engine();
    let mut other = settings_dto();
    other.repository = "/srv/other/repo".to_owned();
    engine
        .ids
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            other.repository.clone(),
            RepositoryId::parse(&"b".repeat(64)).expect("a repository ID"),
        );
    let config = Arc::new(Yielding(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    let (first, second) = tokio::join!(
        service.connect(connect("first", settings_dto())),
        service.connect(connect("second", other)),
    );
    first.expect("the first connection");
    second.expect("the second connection");
    let config = saved(&*config).await;
    assert!(config.repository(&alias("first")).is_some());
    assert!(config.repository(&alias("second")).is_some());
}

/// Both connections find the configuration without the repository; the
/// one that writes second finds the other's entry, and is refused.
#[tokio::test]
async fn one_repository_connected_under_two_aliases_at_the_same_moment_is_kept_once() {
    let engine = engine();
    let config = Arc::new(Yielding(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    let (first, second) = tokio::join!(
        service.connect(connect("first", settings_dto())),
        service.connect(connect("second", settings_dto())),
    );
    let (kept, refused) = match (first.map(|_| ()), second.map(|_| ())) {
        (Ok(()), Err(refused)) => ("first", refused),
        (Err(refused), Ok(())) => ("second", refused),
        other => panic!("one connection is kept and the other refused: {other:?}"),
    };
    assert_eq!(
        refused,
        RepositoriesError::AlreadyConfigured {
            alias: kept.to_owned()
        }
    );
    let config = saved(&*config).await;
    assert_eq!(
        config.repositories().keys().collect::<Vec<_>>(),
        [&alias(kept)]
    );
}

/// Both Creates find the entry recording no ID; the one that takes the
/// lock second finds the ID the first recorded, and is refused as a
/// repository already there.
#[tokio::test]
async fn one_saved_repository_created_twice_at_the_same_moment_is_created_once() {
    let engine = engine();
    let config = Arc::new(Yielding(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the entry is saved");
    imported_and_found_none(&engine, &service, &*config).await;
    let (first, second) = tokio::join!(
        service.create_saved("demo".to_owned()),
        service.create_saved("demo".to_owned()),
    );
    match (first, second) {
        (Ok(_), Err(RepositoriesError::Exists { .. }))
        | (Err(RepositoriesError::Exists { .. }), Ok(_)) => {}
        other => panic!("one Create runs and the other is refused: {other:?}"),
    }
    assert_eq!(
        jobs(&engine),
        3,
        "the connection's test, and one Create's look and initialisation"
    );
}

#[tokio::test]
async fn a_repository_that_never_answers_is_refused_and_the_lock_is_released() {
    // A job that is waited on for ever is a failure of the test, not a
    // stuck run.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let timed_out = || {
            Err(RepositoriesError::Unavailable {
                code: ErrorCode::RepositoryTimedOut,
                args: vec![],
                detail: None,
            })
        };
        let engine = engine();
        *engine.hangs.lock().unwrap_or_else(PoisonError::into_inner) = 3;
        let config = Arc::new(InMemoryConfigStore::new());
        let service = service(&engine, config.clone());
        assert_eq!(
            service
                .test(api::TestRepositoryDto {
                    settings: settings_dto()
                })
                .await
                .map(|_| ()),
            timed_out()
        );
        assert_eq!(
            service
                .connect(connect("first", settings_dto()))
                .await
                .map(|_| ()),
            timed_out()
        );
        assert_eq!(
            service
                .init(init(Some("second"), settings_dto()))
                .await
                .map(|_| ()),
            timed_out()
        );
        assert!(saved(&*config).await.repositories().is_empty());
        // The lock was let go each time: the next connection takes it.
        service
            .connect(connect("third", settings_dto()))
            .await
            .expect("a connection after the timeouts");
        assert!(saved(&*config).await.repository(&alias("third")).is_some());
    })
    .await
    .expect("the service waited for ever");
}

#[tokio::test]
async fn a_connection_waiting_for_the_lock_goes_on_when_the_one_holding_it_times_out() {
    // A job that is waited on for ever is a failure of the test, not a
    // stuck run.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let engine = engine();
        // Only the first job to reach the engine hangs.
        *engine.hangs.lock().unwrap_or_else(PoisonError::into_inner) = 1;
        let config = Arc::new(Yielding(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        let (first, second) = tokio::join!(
            service.connect(connect("first", settings_dto())),
            service.connect(connect("second", settings_dto())),
        );
        assert_eq!(
            first.map(|_| ()),
            Err(RepositoriesError::Unavailable {
                code: ErrorCode::RepositoryTimedOut,
                args: vec![],
                detail: None,
            })
        );
        second.expect("the second connection, once the first let go");
        let config = saved(&*config).await;
        assert!(config.repository(&alias("first")).is_none());
        assert!(config.repository(&alias("second")).is_some());
    })
    .await
    .expect("the service waited for ever");
}
