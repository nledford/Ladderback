//! The worker's stdout: one JSON line per event, written under one lock.
//!
//! Every thread that reports something (the job, rustic's progress callbacks,
//! the log forwarder, the deadline watchdog) shares one [`Events`]. It is the
//! only place that writes, so it is also where secrets are redacted (I4) and
//! where the stream is closed for good.

use api::MsgArg;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, TryLockError};
use std::time::{Duration, Instant};

use domain::secrets::Redactor;

use crate::job::{TRUNCATED, WorkerEvent};

/// What [`Events::end`] found when it closed the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ending {
    /// This call closed the stream, writing its last line if it had one.
    Closed,
    /// The stream already ended, after a result or a failure. Nothing changed.
    AlreadyClosed,
    /// Another thread held the stream for the whole wait, say because the
    /// daemon stopped reading. Nothing was written.
    Stuck,
}

/// A shared handle on the worker's event stream.
#[derive(Clone)]
pub(crate) struct Events(Arc<Shared>);

struct Shared {
    sink: Mutex<Sink>,
    redactor: RwLock<Redactor>,
    started: AtomicBool,
    /// What a caller with nobody to tell does when it finds the stream
    /// broken: see [`Events::report`].
    on_broken: Option<Box<dyn Fn() + Send + Sync>>,
}

struct Sink {
    out: Box<dyn Write + Send>,
    /// Set by the result or failure that ends the stream, and by
    /// [`Events::end`]: later events are dropped.
    closed: bool,
}

impl Events {
    /// A stream whose broken pipe is left for the job to find at its next
    /// event. The worker process itself uses [`Events::with_broken_stream`].
    pub(crate) fn new(out: impl Write + Send + 'static) -> Self {
        Self::build(out, None)
    }

    /// A stream that runs `on_broken` when [`Events::report`] can't write.
    pub(crate) fn with_broken_stream(
        out: impl Write + Send + 'static,
        on_broken: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::build(out, Some(Box::new(on_broken)))
    }

    fn build(
        out: impl Write + Send + 'static,
        on_broken: Option<Box<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self(Arc::new(Shared {
            sink: Mutex::new(Sink {
                out: Box::new(out),
                closed: false,
            }),
            redactor: RwLock::new(Redactor::default()),
            started: AtomicBool::new(false),
            on_broken,
        }))
    }

    /// From now on, every secret `redactor` knows is replaced in the text
    /// that crosses this stream.
    pub(crate) fn redact_with(&self, redactor: Redactor) {
        *self
            .0
            .redactor
            .write()
            .unwrap_or_else(PoisonError::into_inner) = redactor;
    }

    /// `text` with every secret the stream knows replaced, for what the
    /// worker writes elsewhere (stderr).
    pub(crate) fn redact_text(&self, text: &str) -> String {
        self.0
            .redactor
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .redact(text)
    }

    /// Whether [`WorkerEvent::Started`] was written. Progress that begins
    /// before it never reports: the daemon accepts progress only from a
    /// started job.
    pub(crate) fn has_started(&self) -> bool {
        self.0.started.load(Ordering::SeqCst)
    }

    /// Writes `event` as one line and flushes it, unless the stream ended.
    /// A result or a failure ends it.
    pub(crate) fn emit(&self, event: &WorkerEvent) -> io::Result<()> {
        let mut sink = self.0.sink.lock().unwrap_or_else(PoisonError::into_inner);
        if sink.closed {
            return Ok(());
        }
        self.write(&mut sink, event)
    }

    /// [`Events::emit`] for callers that have nobody to tell about a
    /// failure: rustic's progress and log callbacks. A stream that can't be
    /// written means the daemon is gone, so the worker gives up at once
    /// instead of running on to a result nobody can read.
    pub(crate) fn report(&self, event: &WorkerEvent) {
        if self.emit(event).is_err()
            && let Some(on_broken) = &self.0.on_broken
        {
            on_broken();
        }
    }

    /// Ends the stream from outside the job: writes `last` if there is one,
    /// then drops everything that follows. Waits at most `patience` for a
    /// writer that is stuck on a full pipe, so ending never hangs.
    pub(crate) fn end(&self, last: Option<&WorkerEvent>, patience: Duration) -> Ending {
        let give_up_at = Instant::now().checked_add(patience);
        let mut sink = loop {
            match self.0.sink.try_lock() {
                Ok(sink) => break sink,
                // A writer that panicked left at worst a half-written line.
                Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    if give_up_at.is_some_and(|at| Instant::now() >= at) {
                        return Ending::Stuck;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        if sink.closed {
            return Ending::AlreadyClosed;
        }
        if let Some(last) = last {
            // The stream ends either way, so a failed write changes nothing.
            let _ = self.write(&mut sink, last);
        }
        sink.closed = true;
        Ending::Closed
    }

    /// Writes one event as a flushed line. The caller holds the sink.
    fn write(&self, sink: &mut Sink, event: &WorkerEvent) -> io::Result<()> {
        let mut line = serde_json::to_vec(&self.redacted(event))?;
        line.push(b'\n');
        sink.out.write_all(&line)?;
        sink.out.flush()?;
        if matches!(event, WorkerEvent::Started { .. }) {
            self.0.started.store(true, Ordering::SeqCst);
        }
        if event.is_final() {
            sink.closed = true;
        }
        Ok(())
    }

    /// The event with every secret removed from the text it carries, and
    /// each text then cut to [`MAX_TEXT_BYTES`]. Redacting first means a cut
    /// can never keep the start of a secret the worker knows; the daemon
    /// masks what it knows besides in a text marked as cut as a cut text.
    fn redacted(&self, event: &WorkerEvent) -> WorkerEvent {
        let redactor = self
            .0
            .redactor
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let clean = |text: &str| clip(redactor.redact(text));
        let mut event = event.clone();
        match &mut event {
            WorkerEvent::ProgressStart { prefix, .. } => *prefix = clean(prefix),
            WorkerEvent::ProgressTitle { title, .. } => *title = clean(title),
            WorkerEvent::Warning { item, detail, .. } => {
                if let Some(item) = item {
                    *item = clean(item);
                }
                *detail = clean(detail);
            }
            WorkerEvent::Log { message, .. } => *message = clean(message),
            WorkerEvent::Failed { args, detail, .. } => {
                for (_, arg) in args.iter_mut() {
                    if let MsgArg::Text(text) = arg {
                        *text = clean(text);
                    }
                }
                if let Some(detail) = detail {
                    *detail = clean(detail);
                }
            }
            _ => {}
        }
        event
    }
}

/// `text`, cut at a char boundary and marked when longer than
/// [`MAX_TEXT_BYTES`].
fn clip(mut text: String) -> String {
    if text.len() <= MAX_TEXT_BYTES {
        return text;
    }
    let room = MAX_TEXT_BYTES - TRUNCATED.len();
    let end = (0..=room)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    text.truncate(end);
    text.push_str(TRUNCATED);
    text
}

/// The longest text one event field carries. Several fit in one line well
/// under [`crate::job::MAX_MESSAGE_BYTES`], so no event the worker writes is
/// refused by the daemon.
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;

impl std::fmt::Debug for Events {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Events")
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! An in-memory event stream for tests.

    use std::io::{self, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};

    use super::Events;
    use crate::job::WorkerEvent;

    /// Everything written to an [`Events`], readable while it is still in use.
    #[derive(Clone, Default)]
    pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Capture {
        pub(crate) fn events(&self) -> (Events, Capture) {
            (Events::new(self.clone()), self.clone())
        }

        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }

        pub(crate) fn parsed(&self) -> Vec<WorkerEvent> {
            self.text()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A stream and a handle on what it received.
    pub(crate) fn capture() -> (Events, Capture) {
        Capture::default().events()
    }

    /// A writer that takes `left` more writes and then fails like a pipe
    /// whose reader has gone.
    struct BreaksAfter {
        left: usize,
    }

    impl Write for BreaksAfter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.left == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "nobody reads"));
            }
            self.left -= 1;
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A stream whose pipe breaks after `lines` lines, and how many times a
    /// caller gave the worker up because of it.
    pub(crate) fn breaking_after(lines: usize) -> (Events, Arc<AtomicUsize>) {
        let gave_up = Arc::new(AtomicUsize::new(0));
        let events = Events::with_broken_stream(BreaksAfter { left: lines }, {
            let gave_up = Arc::clone(&gave_up);
            move || {
                gave_up.fetch_add(1, Ordering::SeqCst);
            }
        });
        (events, gave_up)
    }

    /// A writer stuck on a full pipe: each write blocks until the test
    /// sends on the sender [`stuck`] returns, or drops it, and then goes
    /// to `written`.
    struct Stuck {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        written: Capture,
    }

    impl Write for Stuck {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let _ = self.entered.send(());
            let _ = self.release.recv();
            self.written.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A stream whose writer is stuck, the receiver that says it has entered
    /// its first write, and the sender that lets it go.
    pub(crate) fn stuck() -> (Events, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (events, _, entered, release) = stuck_then_captured();
        (events, entered, release)
    }

    /// [`stuck`], and a handle on what the stream received once let go.
    pub(crate) fn stuck_then_captured() -> (Events, Capture, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let written = Capture::default();
        let events = Events::new(Stuck {
            entered: entered_tx,
            release,
            written: written.clone(),
        });
        (events, written, entered, release_tx)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use api::{ErrorCode, MsgArg, WarningCode};
    use domain::OperationId;
    use domain::secrets::SecretValue;

    use super::testing::capture;
    use super::*;
    use crate::job::{Level, OpResult, ProgressKind};

    fn log(message: &str) -> WorkerEvent {
        WorkerEvent::Log {
            level: Level::Info,
            message: message.to_owned(),
        }
    }

    fn failed(detail: &str) -> WorkerEvent {
        WorkerEvent::Failed {
            code: ErrorCode::OperationFailed,
            args: Vec::new(),
            detail: Some(detail.to_owned()),
        }
    }

    fn started() -> WorkerEvent {
        WorkerEvent::Started {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(1)),
            pid: 7,
            index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
            repository: None,
        }
    }

    #[test]
    fn every_event_is_one_flushed_line_of_json() {
        let (events, capture) = capture();
        events.emit(&log("one")).unwrap();
        events.emit(&log("two")).unwrap();
        assert_eq!(capture.parsed(), [log("one"), log("two")]);
        assert_eq!(capture.text().lines().count(), 2);
        assert!(capture.text().ends_with('\n'));
    }

    #[test]
    fn a_secret_never_reaches_the_stream_in_any_text_the_worker_reports() {
        let (events, capture) = capture();
        events.redact_with(Redactor::new(&[SecretValue::new("hunter2")]));
        let leaky = "password hunter2 in the text";
        for event in [
            WorkerEvent::ProgressStart {
                key: 0,
                kind: ProgressKind::Bytes,
                prefix: leaky.to_owned(),
            },
            WorkerEvent::ProgressTitle {
                key: 0,
                title: leaky.to_owned(),
            },
            WorkerEvent::Warning {
                item: Some(leaky.to_owned()),
                during: "scan".to_owned(),
                code: WarningCode::SourceUnreadable,
                detail: leaky.to_owned(),
            },
            log(leaky),
            WorkerEvent::Failed {
                code: ErrorCode::OperationFailed,
                args: vec![("what".to_owned(), MsgArg::Text(leaky.to_owned()))],
                detail: Some(leaky.to_owned()),
            },
        ] {
            events.emit(&event).unwrap();
        }
        let text = capture.text();
        assert!(!text.contains("hunter2"), "a secret was written: {text}");
        assert_eq!(
            text.matches("password *** in the text").count(),
            7,
            "{text}"
        );
    }

    #[test]
    fn an_oversized_text_is_cut_so_its_line_stays_under_the_protocol_cap() {
        let (events, capture) = capture();
        let huge = "é".repeat(crate::job::MAX_MESSAGE_BYTES);
        events.emit(&log(&huge)).unwrap();
        events.emit(&failed(&huge)).unwrap();
        let text = capture.text();
        for line in text.lines() {
            assert!(
                line.len() <= crate::job::MAX_MESSAGE_BYTES,
                "{}",
                line.len()
            );
        }
        let parsed = capture.parsed();
        let [
            WorkerEvent::Log { message, .. },
            WorkerEvent::Failed {
                detail: Some(detail),
                ..
            },
        ] = parsed.as_slice()
        else {
            panic!("{text}");
        };
        for cut in [message, detail] {
            assert!(cut.ends_with(TRUNCATED), "the cut is marked");
            assert!(cut.starts_with("éé"), "the start is kept");
            assert!(cut.len() <= MAX_TEXT_BYTES);
        }
    }

    #[test]
    fn a_cut_never_leaves_part_of_a_secret_behind() {
        let (events, capture) = capture();
        events.redact_with(Redactor::new(&[SecretValue::new("hunter2")]));
        // The secret straddles the cut: cutting before redacting would keep
        // "hunt".
        let message = format!(
            "{}hunter2{}",
            "a".repeat(MAX_TEXT_BYTES - 10),
            "z".repeat(100)
        );
        events.emit(&log(&message)).unwrap();
        let text = capture.text();
        assert!(!text.contains("hunt"), "part of a secret was written");
        let parsed = capture.parsed();
        let [WorkerEvent::Log { message, .. }] = parsed.as_slice() else {
            panic!("{text}");
        };
        assert!(message.ends_with(TRUNCATED), "the text was cut");
        assert!(
            message.contains("a***z"),
            "the redacted secret survives the cut"
        );
    }

    #[test]
    fn nothing_follows_a_result_or_a_failure() {
        for last in [
            WorkerEvent::Result(OpResult::Fake { steps_completed: 1 }),
            failed("it broke"),
        ] {
            let (events, capture) = capture();
            events.emit(&log("before")).unwrap();
            events.emit(&last).unwrap();
            events.emit(&log("after")).unwrap();
            events.emit(&failed("again")).unwrap();
            assert_eq!(capture.parsed(), [log("before"), last]);
        }
    }

    #[test]
    fn ending_the_stream_writes_the_last_line_once_and_drops_what_follows() {
        let (events, capture) = capture();
        events.emit(&log("before")).unwrap();
        let last = failed("deadline");
        assert_eq!(
            events.end(Some(&last), Duration::from_secs(1)),
            Ending::Closed
        );
        events.emit(&log("after")).unwrap();
        assert_eq!(
            events.end(Some(&failed("second")), Duration::from_secs(1)),
            Ending::AlreadyClosed
        );
        assert_eq!(capture.parsed(), [log("before"), last]);
    }

    #[test]
    fn ending_after_a_result_writes_no_second_last_line() {
        let (events, capture) = capture();
        let result = WorkerEvent::Result(OpResult::Fake { steps_completed: 1 });
        events.emit(&result).unwrap();
        assert_eq!(
            events.end(Some(&failed("late")), Duration::from_secs(1)),
            Ending::AlreadyClosed
        );
        assert_eq!(capture.parsed(), [result]);
    }

    #[test]
    fn ending_without_a_last_line_only_closes_the_stream() {
        let (events, capture) = capture();
        assert_eq!(events.end(None, Duration::from_secs(1)), Ending::Closed);
        events.emit(&log("after")).unwrap();
        assert_eq!(capture.text(), "");
    }

    #[test]
    fn the_stream_has_started_once_started_was_written() {
        let (events, _capture) = capture();
        events.emit(&log("noise")).unwrap();
        assert!(!events.has_started());
        events.emit(&started()).unwrap();
        assert!(events.has_started());
    }

    #[test]
    fn a_writer_that_fails_reports_the_error() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "daemon went away",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let events = Events::new(Broken);
        let error = events.emit(&log("x")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn a_stream_that_cannot_be_written_gives_the_worker_up_through_the_callers_with_nobody_to_tell()
    {
        let (events, gave_up) = testing::breaking_after(1);
        events.report(&log("fine"));
        assert_eq!(gave_up.load(Ordering::SeqCst), 0);
        events.report(&log("nobody reads"));
        assert_eq!(gave_up.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stream_that_ended_on_purpose_is_not_a_broken_one() {
        let (events, gave_up) = testing::breaking_after(1);
        events.report(&WorkerEvent::Result(OpResult::Fake { steps_completed: 1 }));
        // The result closed the stream: later events are dropped, not written.
        events.report(&log("late"));
        assert_eq!(gave_up.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn text_for_stderr_is_redacted_like_the_stream() {
        let (events, _capture) = capture();
        events.redact_with(Redactor::new(&[SecretValue::new("hunter2")]));
        assert_eq!(
            events.redact_text("opening with hunter2 failed"),
            "opening with *** failed"
        );
    }

    #[test]
    fn ending_does_not_wait_for_a_stuck_writer_beyond_its_patience() {
        let (events, entered, release) = testing::stuck();
        let writer = {
            let events = events.clone();
            thread::spawn(move || events.emit(&log("blocked")))
        };
        entered.recv().unwrap();

        let began = Instant::now();
        let ending = events.end(Some(&failed("deadline")), Duration::from_millis(50));
        assert_eq!(ending, Ending::Stuck);
        assert!(began.elapsed() < Duration::from_secs(5));

        release.send(()).unwrap();
        writer.join().unwrap().unwrap();
    }

    #[test]
    fn a_thread_that_panicked_while_writing_does_not_silence_the_stream() {
        struct Panicking;
        impl Write for Panicking {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                panic!("the writer broke");
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let events = Events::new(Panicking);
        let poisoner = {
            let events = events.clone();
            thread::spawn(move || {
                let _ = events.emit(&log("x"));
            })
        };
        assert!(poisoner.join().is_err());
        // The lock is poisoned now, yet the worker can still end its stream.
        assert_eq!(
            events.end(None, Duration::from_secs(1)),
            Ending::Closed,
            "a poisoned stream must still close"
        );
    }
}
