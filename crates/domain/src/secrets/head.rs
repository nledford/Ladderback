//! The start of a text, for the redactor to look for the end of a secret
//! in (#861, technical design §10.2).
//!
//! A text masked a line at a time (a worker's or rclone's stderr, a log
//! record) splits a TOML multi-line string where a backslash ends a line:
//! the line that ends partway through a secret is masked as a cut text,
//! and the rest of the secret begins the next one, after the white space
//! TOML drops. A text that begins, after its white space, with the last
//! [`MIN_TAIL_BYTES`] bytes or more of a needle is masked from there to
//! where the needle ends. That masks more than a secret where the start of
//! a text only looks like the end of one, never less (I4).

use std::ops::Range;

use super::{Needle, advance, fill_borders};

/// The fewest bytes of the end of a needle that mask the start of a text:
/// fewer are too short to tell from other text, and stay.
pub(super) const MIN_TAIL_BYTES: usize = 4;

/// The start of a text after its white space, at most as long as the
/// longest needle, searched for the way a needle is: the most of it that
/// is the end of a needle is found in one pass over that needle's last
/// bytes, as many as the start has, each compared at most twice.
pub(super) struct Head<'a> {
    /// Where it starts in the text.
    at: usize,
    bytes: &'a [u8],
    borders: Vec<u32>,
}

impl<'a> Head<'a> {
    /// `text` from `at` on, where its white space ends, up to `longest`
    /// bytes; `None` where fewer than [`MIN_TAIL_BYTES`] are left.
    pub(super) fn of(text: &'a str, at: usize, longest: usize) -> Option<Self> {
        let rest = text.as_bytes().get(at..)?;
        let bytes = rest.get(..longest).unwrap_or(rest);
        if bytes.len() < MIN_TAIL_BYTES {
            return None;
        }
        let mut borders = Vec::new();
        fill_borders(bytes, &mut borders)?;
        Some(Self { at, bytes, borders })
    }

    /// Where in the text the start of it is the end of `needle`, when that
    /// is [`MIN_TAIL_BYTES`] or more. A needle and a valid text are UTF-8,
    /// and what matches starts at a character of the text and ends where
    /// the needle does, so it starts and ends between characters.
    pub(super) fn tail(&self, needle: &Needle) -> Option<Range<usize>> {
        let whole = needle.text.as_bytes();
        let end = whole
            .get(whole.len().saturating_sub(self.bytes.len())..)
            .unwrap_or_default();
        let matched = end.iter().fold(0, |matched, &byte| {
            advance(self.bytes, &self.borders, matched, byte)
        }) as usize;
        (matched >= MIN_TAIL_BYTES).then(|| self.at..self.at.saturating_add(matched))
    }
}
