//! What the worker writes to stderr, logged a line at a time with the job's
//! secrets masked (invariant I4).

use domain::secrets::Redactor;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStderr;
use tokio::sync::watch;

use super::DRAIN_AFTER_EXIT;
use super::events::worker_exited;
use super::stop::{Exited, deadline_after};

/// The longest line of the worker's stderr that is logged.
const MAX_LOG_LINE: usize = crate::job::MAX_MESSAGE_BYTES;

/// Logs what the worker writes to stderr, a line at a time, as warnings: its
/// panic report, and whatever a library of its says there. It runs beside the
/// events, which a worker that writes a lot to stderr can't hold up, and ends
/// like they do, [`DRAIN_AFTER_EXIT`] after the worker.
pub(super) async fn forward_stderr(
    mut stderr: BufReader<ChildStderr>,
    mut exited: watch::Receiver<Exited>,
    redactor: Redactor,
) {
    let mut line = Vec::new();
    let mut drain_until = None;
    loop {
        line.clear();
        match next_log_line(&mut stderr, &mut line, &mut exited, &mut drain_until).await {
            Ok(Some(cut)) => {
                if !line.is_empty() || cut {
                    log::log!(
                        target: "ladderback::worker",
                        log::Level::Warn,
                        "{}{}",
                        redacted_log_line(&line, cut, &redactor),
                        if cut { "…" } else { "" }
                    );
                }
            }
            Ok(None) | Err(_) => return,
        }
    }
}

/// One line of stderr with the job's secrets masked. A line cut at the cap
/// may end in the start of a secret, which is masked as a whole one is
/// ([`Redactor::redact_cut`]). (The worker's own events are masked before
/// they are cut; see [`translate`] for what it can't mask.)
///
/// [`translate`]: super::translate::translate
fn redacted_log_line(line: &[u8], cut: bool, redactor: &Redactor) -> String {
    if cut {
        redactor.redact_cut(line)
    } else {
        redactor.redact(&String::from_utf8_lossy(line))
    }
}

/// The next line of the worker's stderr: see [`read_log_line`]. Once the
/// worker has exited, what it wrote is read, but not for longer than
/// [`DRAIN_AFTER_EXIT`] in all: another process may hold the pipe open.
async fn next_log_line(
    stderr: &mut BufReader<ChildStderr>,
    line: &mut Vec<u8>,
    exited: &mut watch::Receiver<Exited>,
    drain_until: &mut Option<tokio::time::Instant>,
) -> std::io::Result<Option<bool>> {
    let reading = read_log_line(stderr, line);
    tokio::pin!(reading);
    tokio::select! {
        biased;
        read = &mut reading => return read,
        () = worker_exited(exited) => {}
    }
    let until = *drain_until
        .get_or_insert_with(|| deadline_after(tokio::time::Instant::now(), DRAIN_AFTER_EXIT));
    match tokio::time::timeout_at(until, reading).await {
        Ok(read) => read,
        Err(_) => Ok(None),
    }
}

/// Reads one line of the worker's stderr into `line`: the first
/// [`MAX_LOG_LINE`] bytes of it. The rest of a longer line is read and
/// dropped, never held. `Some(cut)` is a line, `cut` whether it was longer
/// than that or ended without its newline, as a worker killed partway
/// through a write leaves it; `None` is the end of the stream.
async fn read_log_line(
    reader: &mut BufReader<ChildStderr>,
    line: &mut Vec<u8>,
) -> std::io::Result<Option<bool>> {
    let mut cut = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // A last line without its newline counts, as a cut one: the
            // stream may have ended partway through a write.
            return Ok((!line.is_empty() || cut).then_some(true));
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let content = newline.unwrap_or(available.len());
        let taken = newline.map_or(available.len(), |at| at.saturating_add(1));
        let room = MAX_LOG_LINE.saturating_sub(line.len());
        cut |= content > room;
        line.extend_from_slice(available.get(..content.min(room)).unwrap_or_default());
        reader.consume(taken);
        if newline.is_some() {
            return Ok(Some(cut));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::time::Duration;

    use api::WarningCode;
    use app::ports::{Engine, EngineEvent, RunningJob};
    use domain::{OperationId, Uuid};
    use futures::StreamExt;

    use super::*;
    use crate::client::WorkerEngine;
    use crate::client::test_support::*;
    use crate::job::{MAX_MESSAGE_BYTES, WorkerEvent};

    /// A stderr line that was not cut is all the worker said: an end that
    /// only begins like a secret stays. One that was cut is masked from
    /// where it begins a secret (I4).
    #[test]
    fn only_a_cut_stderr_line_is_masked_where_it_ends_like_a_secret_begins() {
        let redactor = redactor_of(&["hunter2-demo"]);
        assert_eq!(redacted_log_line(b"said h", false, &redactor), "said h");
        assert_eq!(redacted_log_line(b"said h", true, &redactor), "said ***");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn what_a_worker_writes_to_stderr_reaches_the_log_as_a_warning() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; echo 'stderr marker 1' >&2; echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let events = run(&engine).await.events.collect::<Vec<_>>().await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)], "{events:?}");
        assert!(
            logged_soon(log::Level::Warn, |text| text == "stderr marker 1")
                .await
                .is_some(),
            "the worker's stderr never reached the log"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stderr_line_over_the_cap_is_cut_and_the_lines_after_it_still_arrive() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; head -c {} /dev/zero | tr '\\0' x >&2; echo >&2; \
             echo 'stderr marker 2' >&2; echo '{}'",
            3 * MAX_MESSAGE_BYTES,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let _ = run(&engine).await.events.collect::<Vec<_>>().await;
        let long = logged_soon(log::Level::Warn, |text| text.starts_with("xxxx"))
            .await
            .expect("the long stderr line never reached the log");
        assert!(
            long.len() <= MAX_MESSAGE_BYTES + '…'.len_utf8() && long.ends_with('…'),
            "the long line was not cut at the cap: {} bytes",
            long.len()
        );
        assert!(
            logged_soon(log::Level::Warn, |text| text == "stderr marker 2")
                .await
                .is_some(),
            "the line after the long one was lost"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_jobs_password_and_secrets_are_masked_in_what_the_worker_writes_to_stderr() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; echo 'redaction one: hunter2-demo then CANARY-resolved' >&2; echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &["CANARY-resolved"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("redaction one:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "redaction one: *** then ***");
    }

    /// A line as rclone writes it, quoting what it can't serve as Go's `%q`
    /// does, a control or format character of a credential escaped (#331),
    /// is masked on the worker's stderr. rustic logs rclone's lines as the
    /// worker's log events, which the same redactor masks.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_secret_is_masked_where_rclone_quotes_it_in_what_the_worker_writes_to_stderr() {
        capture_the_worker_log();
        let script = format!(
            r#"cat >/dev/null; printf '%s\n' '2026/11/12 10:00:00 CRITICAL: Failed to create file system for ":s3,secret_access_key=CANARY-go\x01quoted\u200bkey:demo-bucket/repo": demo error' >&2; echo '{}'"#,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &["CANARY-go\u{1}quoted\u{200b}key"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let line = logged_soon(log::Level::Warn, |text| {
            text.starts_with("2026/11/12 10:00:00 CRITICAL")
        })
        .await
        .expect("the worker's stderr never reached the log");
        assert_eq!(
            line,
            "2026/11/12 10:00:00 CRITICAL: Failed to create file system for \
             \":s3,secret_access_key=***:demo-bucket/repo\": demo error"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_secret_straddling_the_cut_of_a_long_stderr_line_leaves_nothing_of_itself() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; head -c {} /dev/zero | tr '\\0' x >&2; \
             printf 'CANARY-straddle and more\\n' >&2; echo '{}'",
            MAX_MESSAGE_BYTES - 3,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &["CANARY-straddle"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let _ = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("xxxx"))
            .await
            .expect("the long stderr line never reached the log");
        let kept = line.strip_suffix('…').expect("the line was not cut");
        // The start of the secret is masked as the secret would be.
        let before = kept.strip_suffix("***").expect("the cut secret is masked");
        assert!(
            before.chars().all(|c| c == 'x'),
            "the start of the secret is in the log: {:?}",
            before.chars().rev().take(8).collect::<String>()
        );
    }

    /// A whole secret that ends just before the cut of a long stderr line is
    /// masked whole: nothing is cut away to leave its start behind (I4).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_whole_secret_just_before_the_cut_of_a_long_stderr_line_leaves_nothing_of_itself() {
        capture_the_worker_log();
        let secret = "CANARY-before-cut";
        let script = format!(
            "cat >/dev/null; head -c {} /dev/zero | tr '\\0' y >&2; \
             printf '{secret}yyyyyyyyyy and more\\n' >&2; echo '{}'",
            MAX_MESSAGE_BYTES - secret.len() - 1,
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let job = identify("hunter2-demo", &[secret]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let _ = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("yyyy"))
            .await
            .expect("the long stderr line never reached the log");
        assert!(line.ends_with('…'), "the line was not cut");
        assert!(
            !line.contains("CA"),
            "the start of the secret is in the log: {line:?}"
        );
    }

    /// A worker killed partway through a write leaves its last stderr line
    /// without its newline, cut by the kill rather than the cap: it can end
    /// partway through a secret, and it is masked and marked as a cut line
    /// (I4).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stderr_line_a_kill_cut_short_leaves_nothing_of_a_secret() {
        capture_the_worker_log();
        let script = "cat >/dev/null; printf 'killed mid-line: CANARY-kil' >&2; kill -KILL $$";
        let engine = WorkerEngine::new("sh", ["-c", script]);
        let job = identify("hunter2-demo", &["CANARY-killed"]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let _ = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        let line = logged_soon(log::Level::Warn, |text| {
            text.starts_with("killed mid-line:")
        })
        .await
        .expect("the cut stderr line never reached the log");
        assert_eq!(line, "killed mid-line: ***…");
    }

    /// More than a pipe holds, written between two events: a stderr that
    /// nobody reads would stop the worker there.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_floods_stderr_cannot_stall_its_events_or_the_end_of_its_job() {
        capture_the_worker_log();
        let warning = serde_json::to_string(&WorkerEvent::Warning {
            item: None,
            during: "backup".to_owned(),
            code: WarningCode::Unknown,
            detail: "slow disk".to_owned(),
        })
        .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{warning}'; i=0; \
             while [ $i -lt 4000 ]; do echo 'flood flood flood flood flood' >&2; i=$((i+1)); done; \
             echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let events = tokio::time::timeout(Duration::from_secs(4), events.collect::<Vec<_>>())
            .await
            .expect("the flood on stderr stalled the events");
        assert_matches!(
            events.as_slice(),
            [EngineEvent::Warning(_), EngineEvent::Succeeded(_)],
            "{events:?}"
        );
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("the flood on stderr held the end of the job back");
    }
}
