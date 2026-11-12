//! Reading a secret from standard input (technical design §10.1): at a
//! terminal without showing what is typed, or one line from a pipe.

use std::io::{self, BufRead};

/// Whether standard input is a terminal a person types at.
pub fn stdin_is_terminal() -> bool {
    todo!()
}

/// Reads one line of standard input, of at most `max` bytes, without its
/// line ending.
pub fn read_secret_line(_max: usize) -> io::Result<String> {
    todo!()
}

fn read_line_from(_input: &mut impl BufRead, _max: usize) -> io::Result<String> {
    todo!()
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
