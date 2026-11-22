//! What the daemon and every command write when they panic (#1384): one
//! fixed line with the place and the thread, never the panic's message.
//!
//! Rust's default hook writes the message to stderr as it is, and the
//! daemon's stderr is its service log. A message can quote anything a value
//! held, a resolved secret included (invariant I4), and nothing here knows
//! the secrets to mask it with. `ladderback worker` replaces this hook with
//! its own, which masks the message with the job's secrets (§4.1).

/// Replaces the panic hook with one that reports where a panic happened and
/// on which thread, and leaves its message out.
pub(crate) fn install() {
    todo!()
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    /// Set in the copy of this test binary that panics.
    const COPY: &str = "LADDERBACK_TEST_PANIC_REPORT";

    /// What the panic's message carries, and the report never may.
    const CANARY: &str = "canary-1384-do-not-print";

    /// The name of the thread that panics.
    const THREAD: &str = "panicking-worker-pool";

    /// The line of the `panic!` in [`panic_with_the_canary`].
    const PANIC_LINE: u32 = line!() + 3;

    fn panic_with_the_canary() {
        panic!("the repository password is {CANARY}");
    }

    /// The hook as `main` installs it, in a copy of this test binary: its
    /// stderr names the file, the line and the thread of the panic, and
    /// never the message. Asked with `RUST_BACKTRACE`, it adds the
    /// backtrace, which names functions and places, never values.
    #[test]
    fn a_panic_is_reported_by_its_place_and_thread_never_its_message() {
        if std::env::var_os(COPY).is_some() {
            install();
            let ended = std::thread::Builder::new()
                .name(THREAD.to_owned())
                .spawn(panic_with_the_canary)
                .unwrap()
                .join();
            assert!(ended.is_err(), "the thread didn't panic");
            std::process::exit(0);
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "panic_report::tests::a_panic_is_reported_by_its_place_and_thread_never_its_message",
            ])
            .env(COPY, "1")
            .env("RUST_BACKTRACE", "1")
            .env_remove("RUST_LIB_BACKTRACE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "the copy ended {:?}: {said}",
            output.status
        );
        assert!(!said.contains(CANARY), "{said}");
        assert!(
            said.contains(&format!("{}:{PANIC_LINE}:", file!())),
            "{said}"
        );
        assert!(said.contains(&format!("'{THREAD}'")), "{said}");
        assert!(said.contains("panic_with_the_canary"), "{said}");
    }
}
