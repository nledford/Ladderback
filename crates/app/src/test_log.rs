//! The log the use cases write, captured for the tests that check it.

use std::cell::RefCell;

/// One line logged: its target, its level and its text.
type Logged = (String, log::Level, String);

thread_local! {
    /// The lines the use cases logged on this thread.
    static LOGGED: RefCell<Vec<Logged>> = const { RefCell::new(Vec::new()) };
}

/// Keeps each line a use case logs on the thread that logged it: each test
/// runs its use cases on its own thread (`now_or_never`, or a
/// `#[tokio::test]`'s runtime on the test's thread), so tests running at
/// once in one process never see each other's lines.
struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata
            .target()
            .starts_with(module_path!().trim_end_matches("::test_log"))
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            LOGGED.with_borrow_mut(|lines| {
                lines.push((
                    record.target().to_owned(),
                    record.level(),
                    record.args().to_string(),
                ));
            });
        }
    }

    fn flush(&self) {}
}

/// Captures the log from here on, and forgets what this thread logged
/// before.
pub(crate) fn capture_the_log() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        log::set_logger(&CapturingLogger).expect("no other logger in app's tests");
        log::set_max_level(log::LevelFilter::Trace);
    });
    LOGGED.with_borrow_mut(Vec::clear);
}

/// What `module` and the modules under it logged on this thread since this
/// was last asked, with their levels; what the others logged is forgotten.
pub(crate) fn logged_by(module: &str) -> Vec<(log::Level, String)> {
    LOGGED
        .with_borrow_mut(std::mem::take)
        .into_iter()
        .filter(|(target, _, _)| target.starts_with(module))
        .map(|(_, level, line)| (level, line))
        .collect()
}
