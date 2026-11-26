use api::{MsgArg, RestartDto, RestartWhenDto};
use l10n::msg;

use super::*;

fn told(offered: bool, pending: bool, running: u32) -> RestartDto {
    RestartDto {
        offered,
        pending,
        running,
    }
}

#[test]
fn a_read_shows_whether_a_restart_is_offered_or_waits() {
    assert_eq!(Stage::of(None), Stage::Unreachable);
    assert_eq!(Stage::of(Some(&told(false, false, 2))), Stage::Unavailable);
    assert_eq!(Stage::of(Some(&told(true, true, 2))), Stage::Pending);
    assert_eq!(
        Stage::of(Some(&told(true, false, 2))),
        Stage::Offered { running: 2 }
    );
}

#[test]
fn a_pending_restart_with_no_answer_is_the_daemon_going_down() {
    assert_eq!(
        Stage::while_pending(None),
        Stage::Restarting { seen_down: true }
    );
    assert_eq!(
        Stage::while_pending(Some(&told(true, true, 1))),
        Stage::Pending
    );
    // Canceled elsewhere: the section offers a restart again.
    assert_eq!(
        Stage::while_pending(Some(&told(true, false, 1))),
        Stage::Offered { running: 1 }
    );
}

#[test]
fn a_restart_now_or_with_nothing_running_waits_for_the_daemon_and_one_that_waits_is_pending() {
    let waits = Stage::Restarting { seen_down: false };
    assert_eq!(
        Stage::after_ask(RestartWhenDto::Now, &told(true, false, 2)),
        waits
    );
    assert_eq!(
        Stage::after_ask(RestartWhenDto::Finished, &told(true, true, 0)),
        waits
    );
    assert_eq!(
        Stage::after_ask(RestartWhenDto::Finished, &told(true, true, 2)),
        Stage::Pending
    );
}

#[test]
fn the_request_says_when() {
    assert_eq!(request(RestartWhenDto::Now).when, RestartWhenDto::Now);
    assert_eq!(
        request(RestartWhenDto::Finished).when,
        RestartWhenDto::Finished
    );
}

#[test]
fn with_nothing_running_the_dialog_offers_a_plain_restart_and_no_restart_now() {
    assert_eq!(
        running_said(0),
        (msg::UI_RESTART_RUNNING, vec![("running", MsgArg::Count(0))])
    );
    assert_eq!(confirm_said(0), (msg::UI_RESTART_CONFIRM, Vec::new()));
    assert!(!offers_now(0));
}

#[test]
fn with_operations_running_the_dialog_offers_a_restart_once_they_finish_or_now() {
    assert_eq!(
        running_said(3),
        (msg::UI_RESTART_RUNNING, vec![("running", MsgArg::Count(3))])
    );
    assert_eq!(
        confirm_said(1),
        (
            msg::UI_RESTART_WHEN_FINISHED,
            vec![("running", MsgArg::Count(1))]
        )
    );
    assert!(offers_now(1));
}

#[test]
fn an_answer_before_the_daemon_went_down_never_reloads() {
    let mut wait = Wait::new(0, false);
    assert_eq!(wait.after(true, 250), Waited::Again { not_back: false });
    assert_eq!(wait.after(true, 500), Waited::Again { not_back: false });
    assert_eq!(wait.after(false, 750), Waited::Again { not_back: false });
    assert_eq!(wait.after(true, 1_000), Waited::Reload);
}

#[test]
fn a_wait_that_has_seen_the_daemon_down_reloads_on_its_first_answer() {
    let mut wait = Wait::new(0, true);
    assert_eq!(wait.after(true, 250), Waited::Reload);
}

#[test]
fn a_minute_without_the_daemon_says_it_isnt_back_and_goes_on_asking() {
    let start = 1_000_000;
    let mut wait = Wait::new(start, false);
    assert_eq!(
        wait.after(false, start + NOT_BACK_AFTER_MS - 1),
        Waited::Again { not_back: false }
    );
    assert_eq!(
        wait.after(false, start + NOT_BACK_AFTER_MS),
        Waited::Again { not_back: true }
    );
    assert_eq!(
        wait.after(true, start + NOT_BACK_AFTER_MS + 250),
        Waited::Reload
    );
}

#[test]
fn the_status_says_a_restart_waits_or_the_daemon_restarts() {
    assert_eq!(
        status_of(Stage::Pending, false),
        Some(msg::UI_RESTART_PENDING)
    );
    let restarting = Stage::Restarting { seen_down: true };
    assert_eq!(status_of(restarting, false), Some(msg::UI_RESTARTING));
    assert_eq!(status_of(restarting, true), Some(msg::UI_RESTART_NOT_BACK));
    for quiet in [
        Stage::Loading,
        Stage::Unreachable,
        Stage::Unavailable,
        Stage::Offered { running: 0 },
    ] {
        assert_eq!(status_of(quiet, true), None, "{quiet:?}");
    }
}

// While the page waits, a proxy before the stopped daemon answers in its
// place; only Ladderback's own answer, a session or one of its refusals,
// is the daemon back.
#[test]
fn only_ladderbacks_own_answer_counts_as_the_daemon_back() {
    assert!(is_ladderback(200, r#"{"sign-in":"signed-out"}"#));
    assert!(is_ladderback(503, r#"{"code":"sign-in-unavailable"}"#));
    assert!(!is_ladderback(502, "<html><body>Bad Gateway</body></html>"));
    assert!(!is_ladderback(504, ""));
    assert!(!is_ladderback(200, "<html><body>Maintenance</body></html>"));
    assert!(!is_ladderback(503, r#"{"error":"upstream"}"#));
    // A session's JSON under a gateway status is the proxy's, not ours.
    assert!(!is_ladderback(502, r#"{"sign-in":"signed-out"}"#));
}
