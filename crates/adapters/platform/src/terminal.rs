//! Reading a secret from standard input (technical design §10.1): at a
//! terminal without showing what is typed, or one line from a pipe.

use std::io::{self, BufRead, IsTerminal, Read};

/// Whether standard input is a terminal a person types at.
pub fn stdin_is_terminal() -> bool {
    io::stdin().is_terminal()
}

/// Reads one line of standard input, of at most `max` bytes, without its
/// line ending. At a terminal, what is typed is not shown. A longer line is
/// an `InvalidData` error and is never kept whole, and so is a line that is
/// not UTF-8.
pub fn read_secret_line(max: usize) -> io::Result<String> {
    let _quiet = if stdin_is_terminal() {
        Some(EchoOff::start()?)
    } else {
        None
    };
    read_line_from(&mut io::stdin().lock(), max)
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
    saved: nix::sys::termios::Termios,
}

impl EchoOff {
    #[cfg(unix)]
    fn start() -> io::Result<Self> {
        use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
        let saved = tcgetattr(io::stdin()).map_err(io::Error::from)?;
        let mut quiet = saved.clone();
        quiet.local_flags.remove(LocalFlags::ECHO);
        quiet.local_flags.insert(LocalFlags::ECHONL);
        tcsetattr(io::stdin(), SetArg::TCSAFLUSH, &quiet).map_err(io::Error::from)?;
        Ok(Self { saved })
    }

    #[cfg(not(unix))]
    fn start() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform cannot hide what is typed",
        ))
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        use nix::sys::termios::{SetArg, tcsetattr};
        // Nothing more can be done for a terminal that won't take its own
        // settings back.
        let _ = tcsetattr(io::stdin(), SetArg::TCSANOW, &self.saved);
    }
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
