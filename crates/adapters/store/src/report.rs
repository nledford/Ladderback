//! Keeps what an error says about stored values short, however long the
//! stored values are: a row is input, and a megabyte of text in one must not
//! become a megabyte in a log line.

/// The longest a `Corrupt` or `Unavailable` detail gets, in bytes.
const MAX_DETAIL_BYTES: usize = 400;

/// The most characters of a stored value a report quotes.
const MAX_QUOTED_CHARS: usize = 40;

/// `text`, cut to [`MAX_DETAIL_BYTES`] at a character boundary, with `…` where
/// it was cut.
pub(crate) fn cap(mut text: String) -> String {
    if text.len() > MAX_DETAIL_BYTES {
        let mut end = MAX_DETAIL_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

/// The start of a stored value, quoted for a report.
pub(crate) fn shown(value: &str) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(MAX_QUOTED_CHARS).collect();
    if chars.next().is_some() {
        format!("{head:?}…")
    } else {
        format!("{head:?}")
    }
}
