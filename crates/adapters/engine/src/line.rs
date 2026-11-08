//! Reading one line from a process we don't trust to end it.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use crate::job::{MAX_MESSAGE_BYTES, MAX_MESSAGE_READ};

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line of at most [`MAX_MESSAGE_BYTES`] is in the buffer, without its
    /// `\n`. A last line the stream ends without a `\n` counts too.
    Read,
    /// The line is longer than [`MAX_MESSAGE_BYTES`]. The buffer holds its
    /// start, and the rest is still in the reader.
    TooLong,
    /// The stream ended before another line began.
    End,
}

/// Reads the next line into `buf`, replacing what it held, and never reads
/// more than `MAX_MESSAGE_BYTES + 1` bytes of it: the buffer's size is
/// bounded whatever the other side sends, even a line that never ends.
pub(crate) async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    buf: &mut Vec<u8>,
) -> io::Result<Line> {
    buf.clear();
    let read = (&mut *reader)
        .take(MAX_MESSAGE_READ)
        .read_until(b'\n', buf)
        .await?;
    if read == 0 {
        return Ok(Line::End);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    Ok(if buf.len() > MAX_MESSAGE_BYTES {
        Line::TooLong
    } else {
        Line::Read
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read(input: &[u8]) -> (Line, Vec<u8>) {
        let mut reader = input;
        let mut buf = Vec::new();
        let line = read_line(&mut reader, &mut buf).await.unwrap();
        (line, buf)
    }

    #[tokio::test]
    async fn lines_come_one_at_a_time_without_their_newline() {
        let mut reader: &[u8] = b"one\ntwo\n";
        let mut buf = b"stale".to_vec();
        assert_eq!(read_line(&mut reader, &mut buf).await.unwrap(), Line::Read);
        assert_eq!(buf, b"one");
        assert_eq!(read_line(&mut reader, &mut buf).await.unwrap(), Line::Read);
        assert_eq!(buf, b"two");
        assert_eq!(read_line(&mut reader, &mut buf).await.unwrap(), Line::End);
    }

    #[tokio::test]
    async fn a_last_line_without_a_newline_is_still_a_line() {
        assert_eq!(read(b"tail").await, (Line::Read, b"tail".to_vec()));
    }

    #[tokio::test]
    async fn an_empty_line_is_a_line() {
        assert_eq!(read(b"\nnext\n").await, (Line::Read, Vec::new()));
    }

    #[tokio::test]
    async fn a_line_of_exactly_the_cap_is_read_whole() {
        let mut input = vec![b'x'; MAX_MESSAGE_BYTES];
        input.push(b'\n');
        let (line, buf) = read(&input).await;
        assert_eq!(line, Line::Read);
        assert_eq!(buf.len(), MAX_MESSAGE_BYTES);
    }

    #[tokio::test]
    async fn a_last_line_of_exactly_the_cap_is_read_whole() {
        let (line, buf) = read(&vec![b'x'; MAX_MESSAGE_BYTES]).await;
        assert_eq!(line, Line::Read);
        assert_eq!(buf.len(), MAX_MESSAGE_BYTES);
    }

    #[tokio::test]
    async fn a_line_one_byte_past_the_cap_is_too_long() {
        let mut input = vec![b'x'; MAX_MESSAGE_BYTES + 1];
        input.push(b'\n');
        assert_eq!(read(&input).await.0, Line::TooLong);
        assert_eq!(
            read(&vec![b'x'; MAX_MESSAGE_BYTES + 1]).await.0,
            Line::TooLong
        );
    }

    #[tokio::test]
    async fn a_too_long_line_is_not_buffered_past_the_cap() {
        let (line, buf) = read(&vec![b'x'; 4 * MAX_MESSAGE_BYTES]).await;
        assert_eq!(line, Line::TooLong);
        assert_eq!(buf.len(), MAX_MESSAGE_BYTES + 1);
    }
}
