use super::support::*;
use super::*;

/// The log names a run by what started it, in words (#505).
#[test]
fn a_run_is_named_in_the_log_by_what_started_it_in_words() {
    for (trigger, words) in [
        (Trigger::Schedule, "the scheduled run"),
        (Trigger::Manual, "the manual run"),
        (Trigger::CatchUp, "the catch-up run"),
        (Trigger::Retry, "the retry"),
        (Trigger::Hub, "the hub's run"),
        (Trigger::Mcp, "the MCP client's run"),
        (
            Trigger::ApiToken {
                id: domain::access::api_token::ApiTokenId::parse("abcdefghijkl").expect("an ID"),
                label: domain::access::api_token::ApiTokenLabel::new("nightly-mcp".to_owned())
                    .expect("a label"),
            },
            "the run API token \"nightly-mcp\" (abcdefghijkl) asked for",
        ),
    ] {
        assert_eq!(run_named(&trigger), words, "{trigger:?}");
    }
}

/// What a repository of one location resolves to is said of the
/// repository, never of "location 1" (#473).
#[test]
fn the_log_says_a_repository_of_one_location_exposes_its_credentials_naming_no_location() {
    captured_log::capture();
    log_location(LocationReport::ExposedCredentials {
        alias: RepositoryAlias::parse("solo-exposed").expect("an alias"),
        position: None,
        cause: domain::planning::ExposedCredentials::PlainHttp,
    });
    let said: Vec<_> = captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains("solo-exposed"))
        .collect();
    let [(level, line)] = said.as_slice() else {
        panic!("one line names the repository: {said:?}");
    };
    assert_eq!(*level, log::Level::Warn);
    assert!(
            line.contains(
                "repository solo-exposed: this repository sends its credentials where whoever answers at its address can read them (plain-http)"
            ),
            "{line}"
        );
    assert!(!line.contains("location"), "{line}");
}

/// The lines the daemon logged about `operation` once its run had said
/// what the log refused it: one for each of the `refused` writes, so the
/// wait ends when there are that many. A run that never says gives up
/// after a few seconds, and the lines as they stand tell what is missing.
async fn logged_about_the_run(
    operation: &str,
    refused: &std::sync::atomic::AtomicUsize,
) -> Vec<(log::Level, String)> {
    let about = || {
        captured_log::lines()
            .into_iter()
            .filter(|(_, line)| line.contains(operation))
            .collect::<Vec<_>>()
    };
    let said = async {
        loop {
            let lines = about();
            let refused = refused.load(std::sync::atomic::Ordering::SeqCst);
            if refused > 0 && lines.len() >= refused {
                return lines;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), said)
        .await
        .unwrap_or_else(|_| about())
}

/// Each refused write is said once, at warning level, with the code of
/// the failure and none of its words (I4).
fn assert_each_refusal_is_said(lines: &[(log::Level, String)], refused: usize) {
    assert!(
        refused > 0,
        "the log refused nothing, so the test shows nothing"
    );
    assert_eq!(
        lines.len(),
        refused,
        "one line for each refused write: {lines:?}"
    );
    for (level, line) in lines {
        assert_eq!(*level, log::Level::Warn, "{line}");
        assert!(line.contains("store-unavailable"), "{line}");
    }
    let everything = captured_log::lines();
    assert!(
        everything.iter().all(|(_, line)| !line.contains("canary")),
        "a line repeats what the log said: {everything:?}"
    );
}

/// A backup goes on without a log that refuses its writes, and the
/// daemon says so in its own log: a log that has stopped taking writes
/// is otherwise invisible to the person running it (#135).
#[tokio::test]
async fn a_write_the_log_refuses_during_a_backup_is_logged_once_with_its_code_and_never_its_words()
{
    captured_log::capture();
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    runner.store = Arc::new(RefusingSaves {
        inner: Arc::clone(&runner.store),
        refused: Arc::clone(&refused),
    });
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("a backup starts without a log that takes its writes");
    // The engine parks the run, as a real one is parked while it works,
    // so a cancel is what ends it.
    let operation = operation_of(&id).expect("an operation ID");
    assert!(
        runner.registry.cancel(&operation),
        "the run has a switch to fire"
    );
    let lines = logged_about_the_run(&id, &refused).await;
    assert_each_refusal_is_said(&lines, refused.load(std::sync::atomic::Ordering::SeqCst));
}

/// The same for a maintenance run, whose last write is the one the log
/// refuses here: its start was taken, so the run goes on to the end.
#[tokio::test]
async fn a_write_the_log_refuses_during_maintenance_is_logged_once_with_its_code_and_never_its_words()
 {
    captured_log::capture();
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(Completes);
    let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    runner.store = Arc::new(RefusingSaves {
        inner: Arc::clone(&runner.store),
        refused: Arc::clone(&refused),
    });
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("a check starts");
    let lines = logged_about_the_run(&id, &refused).await;
    assert_each_refusal_is_said(&lines, refused.load(std::sync::atomic::Ordering::SeqCst));
}

/// The lines `log_ended` logs about a run that `n` names, which ended
/// after the log refused it the `refused` writes.
fn logged_when_a_run_ends(
    n: u128,
    refused: Vec<app::ports::StoreError>,
) -> Vec<(log::Level, String)> {
    captured_log::capture();
    let id = OperationId::from_uuid(Uuid::from_u128(n));
    log_ended(&Finished {
        operation: running_backup(id),
        result: None,
        recording_errors: refused,
        stats_failed: None,
    });
    let id = id.as_uuid().hyphenated().to_string();
    captured_log::lines()
        .into_iter()
        .filter(|(_, line)| line.contains(&id))
        .collect()
}

/// A stats read that failed after a check is one warning, named by the
/// code of its failure and never its words (I4).
#[test]
fn a_stats_read_that_failed_after_a_check_is_logged_by_its_code_and_never_its_words() {
    let reason = domain::execution::FailureReason::EngineFailed {
        detail: "canary-stats-detail".to_owned(),
    };
    let failures = [
        app::stats::StatsError::Failed(reason.clone()),
        app::stats::StatsError::Engine(EngineError::Unavailable {
            detail: "canary-stats-detail".to_owned(),
        }),
    ];
    for (n, failed) in (0x7101_u128..).zip(failures) {
        captured_log::capture();
        let id = OperationId::from_uuid(Uuid::from_u128(n));
        log_ended(&Finished {
            operation: running_backup(id),
            result: None,
            recording_errors: Vec::new(),
            stats_failed: Some(failed),
        });
        let id = id.as_uuid().hyphenated().to_string();
        let lines: Vec<_> = captured_log::lines()
            .into_iter()
            .filter(|(_, line)| line.contains(&id) && line.contains("stats"))
            .collect();
        let [(level, line)] = lines.as_slice() else {
            panic!("one line of the stats: {lines:?}");
        };
        assert_eq!(*level, log::Level::Warn, "{line}");
        assert!(!line.contains("canary"), "{line}");
    }
    let lines = captured_log::lines();
    assert!(
        lines
            .iter()
            .any(|(_, line)| line.contains(&format!("read failed: {}", reason.code()))),
        "the failure's code: {lines:?}"
    );
}

/// A log that took every write of a run has nothing to be warned of.
#[test]
fn a_run_the_log_took_in_full_logs_no_warning() {
    assert_eq!(logged_when_a_run_ends(0x7001, Vec::new()), []);
}

/// Each refused write is one warning, named by the code of its kind of
/// failure, and none of the failure's words reaches the log (I4).
#[test]
fn each_write_the_log_refused_is_one_warning_with_the_code_of_its_failure() {
    let words = "canary: the disk of /srv/secret-folder is full";
    let lines = logged_when_a_run_ends(
        0x7002,
        vec![
            app::ports::StoreError::Unavailable {
                detail: words.to_owned(),
            },
            app::ports::StoreError::Corrupt {
                detail: words.to_owned(),
            },
        ],
    );
    let [(first_level, first), (second_level, second)] = lines.as_slice() else {
        panic!("one line for each refused write: {lines:?}");
    };
    assert_eq!(*first_level, log::Level::Warn, "{first}");
    assert_eq!(*second_level, log::Level::Warn, "{second}");
    assert!(first.contains("store-unavailable"), "{first}");
    assert!(second.contains("store-corrupt"), "{second}");
    assert!(
        lines.iter().all(|(_, line)| !line.contains("canary")),
        "a line repeats what the log said: {lines:?}"
    );
}
