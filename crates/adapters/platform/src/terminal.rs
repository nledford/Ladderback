//! Reading a secret from standard input (technical design §10.1): at a
//! terminal without showing what is typed, or one line from a pipe.

use std::io::{self, BufRead, IsTerminal, Read};

/// The terminal a person types at.
#[cfg(unix)]
type Tty = std::os::fd::OwnedFd;
#[cfg(not(unix))]
type Tty = std::convert::Infallible;

/// Whether standard input is a terminal a person types at.
pub fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// Reads one line of standard input, of at most `max` bytes, without its
/// line ending. At a terminal, what is typed is not shown. A longer line is
/// an `InvalidData` error and is never kept whole, and so is a line that is
/// not UTF-8.
pub fn read_secret_line(max: usize) -> io::Result<String> {
    SecretInput::stdin()?.read_line(max)
}

/// Standard input held for secrets, a line at a time. At a terminal, what is
/// typed is not shown from the moment this is made until it is dropped, and
/// the terminal's settings are changed once: changing them discards the
/// input the terminal has queued, so a person who pastes several lines at
/// once would lose all but the first if each read changed them.
pub struct SecretInput<R> {
    reader: R,
    _quiet: Option<EchoOff>,
}

impl SecretInput<io::StdinLock<'static>> {
    /// Standard input, at a terminal or a pipe.
    pub fn stdin() -> io::Result<Self> {
        let quiet = if stdin_is_terminal() {
            Some(EchoOff::start(&stdin_tty()?)?)
        } else {
            None
        };
        Ok(Self {
            reader: io::stdin().lock(),
            _quiet: quiet,
        })
    }
}

impl<R: BufRead> SecretInput<R> {
    /// `reader`, the input of the terminal `tty`.
    #[cfg(all(test, unix))]
    fn over(tty: Tty, reader: R) -> io::Result<Self> {
        Ok(Self {
            reader,
            _quiet: Some(EchoOff::start(&tty)?),
        })
    }

    /// One line of at most `max` bytes, without its line ending: an
    /// `InvalidData` error when it is longer, and when it is not UTF-8.
    pub fn read_line(&mut self, max: usize) -> io::Result<String> {
        read_line_from(&mut self.reader, max)
    }
}

#[cfg(unix)]
fn stdin_tty() -> io::Result<Tty> {
    use std::os::fd::AsFd as _;
    io::stdin().as_fd().try_clone_to_owned()
}

#[cfg(not(unix))]
fn stdin_tty() -> io::Result<Tty> {
    Err(cannot_hide())
}

fn read_line_from(input: &mut impl BufRead, max: usize) -> io::Result<String> {
    let mut line = Vec::new();
    // Room for the line ending, and one byte more to tell a line that is too
    // long from one that fits.
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(3);
    input.take(limit).read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    if line.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the line is longer than a password",
        ));
    }
    String::from_utf8(line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the line is not UTF-8"))
}

/// The terminal's echo turned off until this is dropped; the line ending a
/// person types is still shown, so the next line starts on its own. A drop
/// never runs when a signal ends the process, so Ctrl-C and `SIGTERM` put
/// the terminal's settings back first (#399, [`ending`]). One prompt is
/// open at a time, as the process has one standard input.
struct EchoOff {
    #[cfg(unix)]
    tty: Tty,
    #[cfg(unix)]
    saved: nix::sys::termios::Termios,
}

impl EchoOff {
    #[cfg(unix)]
    fn start(tty: &Tty) -> io::Result<Self> {
        use nix::sys::termios::{LocalFlags, tcgetattr};
        let saved = tcgetattr(tty).map_err(io::Error::from)?;
        let mut quiet = saved.clone();
        quiet.local_flags.remove(LocalFlags::ECHO);
        quiet.local_flags.insert(LocalFlags::ECHONL);
        // Everything that can fail is done before echo goes off, which only
        // a drop of this, or a signal, then turns back on.
        let echo_off = Self {
            tty: tty.try_clone()?,
            saved: saved.clone(),
        };
        ending::quiet(tty, saved, &quiet)?;
        Ok(echo_off)
    }

    #[cfg(not(unix))]
    fn start(_tty: &Tty) -> io::Result<Self> {
        Err(cannot_hide())
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        ending::put_back_now(&self.tty, &self.saved);
    }
}

/// Ctrl-C (`SIGINT`) and `SIGTERM` at the prompt (#399): their default
/// action ends the process at once, with no drop run, which would leave the
/// terminal silent until `reset` or `stty echo`. A thread of their own takes
/// them instead, puts the prompt's terminal settings back, and then ends the
/// process by the signal's own default action, so the shell sees the same
/// end as before.
///
/// The thread stays for the rest of the process once a prompt has started
/// it: no safe code can give a signal back to its default action, and the
/// thread ends the process as that action would, prompt or not. So a
/// signal the parent set to be ignored ends the process too from then on,
/// and if the thread can't be started, the prompt fails and these signals
/// do nothing until the process, which then ends, has.
#[cfg(unix)]
mod ending {
    use std::io;
    use std::os::fd::OwnedFd;
    use std::sync::{Mutex, PoisonError};

    use nix::sys::termios::{FlushArg, SetArg, Termios, tcdrain, tcflush, tcsetattr};
    use signal_hook::consts::{SIGINT, SIGTERM};

    /// The terminal a prompt has changed, and its settings from before.
    static CHANGED: Mutex<Option<(OwnedFd, Termios)>> = Mutex::new(None);

    /// Whether the thread that takes the signals runs.
    static WATCHING: Mutex<bool> = Mutex::new(false);

    /// Sets `quiet` on `tty`, whose settings were `saved`, and has Ctrl-C or
    /// `SIGTERM` put `saved` back before it ends the process, until
    /// [`put_back_now`]. Both happen under the lock the signal's thread
    /// takes, so it finds either echo still on or `saved` to put back.
    pub(super) fn quiet(tty: &OwnedFd, saved: Termios, quiet: &Termios) -> io::Result<()> {
        watch()?;
        let kept = tty.try_clone()?;
        // The prompt's own output is waited for outside the lock: a terminal
        // that holds it back (after Ctrl-S) would hold a signal back too.
        tcdrain(tty).map_err(io::Error::from)?;
        let mut changed = CHANGED.lock().unwrap_or_else(PoisonError::into_inner);
        set_quiet(tty, quiet).map_err(io::Error::from)?;
        *changed = Some((kept, saved));
        Ok(())
    }

    /// Sets `quiet` on `tty` and drops what was typed before it, without
    /// waiting for the terminal's output: [`quiet`] does this under the lock
    /// the signal's thread takes, and another writer's output that the
    /// terminal holds back (after Ctrl-S) mustn't hold a signal back.
    pub(super) fn set_quiet(tty: &OwnedFd, quiet: &Termios) -> nix::Result<()> {
        // Settings first, then the flush: a key pressed between them is
        // dropped rather than shown.
        tcsetattr(tty, SetArg::TCSANOW, quiet)?;
        tcflush(tty, FlushArg::TCIFLUSH)
    }

    /// Puts `saved` back on `tty` now, and nothing on a signal after.
    pub(super) fn put_back_now(tty: &OwnedFd, saved: &Termios) {
        let mut changed = CHANGED.lock().unwrap_or_else(PoisonError::into_inner);
        // Nothing more can be done for a terminal that won't take its own
        // settings back.
        let _ = tcsetattr(tty, SetArg::TCSANOW, saved);
        *changed = None;
    }

    fn watch() -> io::Result<()> {
        let mut watching = WATCHING.lock().unwrap_or_else(PoisonError::into_inner);
        if *watching {
            return Ok(());
        }
        let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM])?;
        std::thread::Builder::new()
            .name("prompt-signals".to_owned())
            .spawn(move || {
                for signal in signals.forever() {
                    // Held until the process ends, so a prompt's own drop
                    // can't change the terminal meanwhile.
                    let mut changed = CHANGED.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some((tty, saved)) = changed.take() {
                        let _ = tcsetattr(&tty, SetArg::TCSANOW, &saved);
                    }
                    // Ends the process as the signal would have; it returns
                    // only for a signal whose default is to go on.
                    let _ = signal_hook::low_level::emulate_default_handler(signal);
                    drop(changed);
                }
            })?;
        *watching = true;
        Ok(())
    }
}

#[cfg(not(unix))]
fn cannot_hide() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform cannot hide what is typed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_read_without_its_ending() {
        for input in [
            "demo-admin-pass\n",
            "demo-admin-pass\r\n",
            "demo-admin-pass",
        ] {
            let read = read_line_from(&mut input.as_bytes(), 15).unwrap();
            assert_eq!(read, "demo-admin-pass", "{input:?}");
        }
    }

    #[test]
    fn only_the_first_line_is_read() {
        let read = read_line_from(&mut "first-line\nsecond-line\n".as_bytes(), 64).unwrap();
        assert_eq!(read, "first-line");
    }

    #[test]
    fn a_line_longer_than_the_most_is_refused() {
        let error = read_line_from(&mut "123456\n".as_bytes(), 5).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            read_line_from(&mut "12345\n".as_bytes(), 5).unwrap(),
            "12345"
        );
    }

    #[test]
    fn a_line_that_is_not_utf8_is_refused() {
        let error = read_line_from(&mut &b"\xff\xfe\n"[..], 64).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

#[cfg(all(test, unix))]
mod tty_tests {
    use std::fs::File;
    use std::io::{BufReader, Write};
    use std::time::Duration;

    use nix::pty::openpty;
    use nix::sys::termios::{LocalFlags, tcgetattr};

    use super::*;

    /// A person pastes two lines at once while the prompt waits: the first
    /// read takes one, and the second line, already queued by the terminal,
    /// is read next, with echo still off between them.
    #[test]
    fn lines_pasted_together_are_all_read_with_echo_off_between_them() {
        let pty = openpty(None, None).unwrap();
        let tty = pty.slave;
        let reader = BufReader::new(File::from(tty.try_clone().unwrap()));
        let mut input = SecretInput::over(tty.try_clone().unwrap(), reader).unwrap();
        let mut master = File::from(pty.master);
        // The terminal program takes what the terminal shows: changing the
        // terminal's settings waits for its output to be taken.
        let mut screen = master.try_clone().unwrap();
        std::thread::spawn(move || {
            let mut taken = [0; 256];
            while matches!(screen.read(&mut taken), Ok(read) if read > 0) {}
        });
        let paste = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            master.write_all(b"first\nsecond\n").unwrap();
            // A read that lost the second line gets this one instead of
            // waiting for ever.
            std::thread::sleep(Duration::from_millis(500));
            master.write_all(b"after\n").unwrap();
            master
        });
        assert_eq!(input.read_line(64).unwrap(), "first");
        assert_eq!(input.read_line(64).unwrap(), "second");
        let echo = tcgetattr(&tty).unwrap().local_flags;
        assert!(
            !echo.contains(LocalFlags::ECHO),
            "echo came back between the lines"
        );
        drop(input);
        let _master = paste.join().unwrap();
    }

    /// Output another writer has queued that the terminal won't take (after
    /// Ctrl-S) doesn't hold up turning echo off, which happens under the
    /// lock the signal's thread needs before it can end the process.
    #[test]
    fn echo_goes_off_without_waiting_for_held_output() {
        let pty = openpty(None, None).unwrap();
        let tty = pty.slave;
        // Nobody takes the terminal's output, so this writer fills its queue
        // and then waits.
        let mut writer = File::from(tty.try_clone().unwrap());
        std::thread::spawn(move || {
            let line = [b'x'; 256];
            while writer.write_all(&line).is_ok() {}
        });
        std::thread::sleep(Duration::from_millis(500));
        let mut quiet = tcgetattr(&tty).unwrap();
        quiet.local_flags.remove(LocalFlags::ECHO);
        let slave = tty.try_clone().unwrap();
        let (done, set) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(ending::set_quiet(&slave, &quiet));
        });
        let result = set.recv_timeout(Duration::from_secs(5));
        assert!(matches!(result, Ok(Ok(()))), "{result:?}");
        let echo = tcgetattr(&tty).unwrap().local_flags;
        assert!(!echo.contains(LocalFlags::ECHO));
        drop(pty.master);
    }

    /// Set in the copy of this test binary that
    /// [`a_signal_after_the_prompt_still_ends_the_process`] runs.
    const CHILD: &str = "LADDERBACK_TEST_SIGNAL_AFTER_PROMPT";

    /// Once a prompt has closed, the thread that took its signals stays, and
    /// a signal still ends the process as its default action would: Ctrl-C
    /// while `set-password` hashes and writes the password still stops it.
    #[test]
    fn a_signal_after_the_prompt_still_ends_the_process() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Command, Stdio};

        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        if std::env::var_os(CHILD).is_some() {
            let pty = openpty(None, None).unwrap();
            let tty = pty.slave;
            let reader = BufReader::new(File::from(tty.try_clone().unwrap()));
            drop(SecretInput::over(tty, reader).unwrap());
            kill(Pid::this(), Signal::SIGTERM).unwrap();
            // Reached only while the signal does nothing: the copy then ends
            // well, which the test below refuses.
            std::thread::sleep(Duration::from_secs(5));
            drop(pty.master);
            return;
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal::tty_tests::a_signal_after_the_prompt_still_ends_the_process",
            ])
            .env(CHILD, "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(
            status.signal().map(Signal::try_from),
            Some(Ok(Signal::SIGTERM)),
            "{status:?}"
        );
    }
}
