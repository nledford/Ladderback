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
/// typed is not shown.
pub struct SecretInput<R> {
    reader: R,
    tty: Option<Tty>,
}

impl SecretInput<io::StdinLock<'static>> {
    /// Standard input, at a terminal or a pipe.
    pub fn stdin() -> io::Result<Self> {
        let tty = if stdin_is_terminal() {
            Some(stdin_tty()?)
        } else {
            None
        };
        Ok(Self {
            reader: io::stdin().lock(),
            tty,
        })
    }
}

impl<R: BufRead> SecretInput<R> {
    /// `reader`, the input of the terminal `tty`.
    #[cfg(all(test, unix))]
    fn over(tty: Tty, reader: R) -> io::Result<Self> {
        Ok(Self {
            reader,
            tty: Some(tty),
        })
    }

    /// One line of at most `max` bytes, without its line ending: an
    /// `InvalidData` error when it is longer, and when it is not UTF-8.
    pub fn read_line(&mut self, max: usize) -> io::Result<String> {
        let _quiet = self.tty.as_ref().map(EchoOff::start).transpose()?;
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
/// person types is still shown, so the next line starts on its own.
struct EchoOff {
    #[cfg(unix)]
    tty: Tty,
    #[cfg(unix)]
    saved: nix::sys::termios::Termios,
}

impl EchoOff {
    #[cfg(unix)]
    fn start(tty: &Tty) -> io::Result<Self> {
        use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
        let saved = tcgetattr(tty).map_err(io::Error::from)?;
        let mut quiet = saved.clone();
        quiet.local_flags.remove(LocalFlags::ECHO);
        quiet.local_flags.insert(LocalFlags::ECHONL);
        tcsetattr(tty, SetArg::TCSAFLUSH, &quiet).map_err(io::Error::from)?;
        Ok(Self {
            tty: tty.try_clone()?,
            saved,
        })
    }

    #[cfg(not(unix))]
    fn start(_tty: &Tty) -> io::Result<Self> {
        Err(cannot_hide())
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        use nix::sys::termios::{SetArg, tcsetattr};
        // Nothing more can be done for a terminal that won't take its own
        // settings back.
        let _ = tcsetattr(&self.tty, SetArg::TCSANOW, &self.saved);
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
}
