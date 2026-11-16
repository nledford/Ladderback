//! A text as TOML reads the escapes of a basic string in it, for the
//! redactor to look for a secret in (#839, technical design §10.2).
//!
//! A secret in a TOML file (`config.toml`, a rustic profile) is written with
//! TOML's escapes: `\b`, `\t`, `\n`, `\f`, `\r`, `\e`, `\"` and `\\`, any
//! character as `\xHH`, `\uHHHH` or `\UHHHHHHHH` in either case, and in a
//! multi-line string broken by a backslash that ends a line. A person may
//! escape any character of it, so its written forms are too many to list as
//! needles: the text is read as TOML reads it instead, and what a secret is
//! found in is masked in the text, from the first byte it was read from to
//! the last.
//!
//! Every backslash of the text is read so, wherever it is: in a string or
//! not, an escape TOML 1.0 lacks (`\e`, `\x`) or not. That masks more than a
//! secret, never less (I4).

use super::{Needle, advance, cover, head::Head, place};

/// What a backslash and what follows it stand for, as TOML reads them.
enum Escape {
    /// A character, written in this many bytes.
    Char(char, usize),
    /// A backslash that ends a line, and the line breaks and white space
    /// after it, in this many bytes: TOML drops them.
    Nothing(usize),
    /// The text ends partway through an escape (`\u00`).
    Open,
    /// Not an escape: the backslash is text.
    No,
}

/// What `rest`, a text from one of its characters on, starts with.
fn escape(rest: &str) -> Escape {
    let Some(after) = rest.strip_prefix('\\') else {
        return Escape::No;
    };
    let short = |c| Escape::Char(c, 2);
    match after.chars().next() {
        None => Escape::Open,
        Some('b') => short('\u{8}'),
        Some('t') => short('\t'),
        Some('n') => short('\n'),
        Some('f') => short('\u{c}'),
        Some('r') => short('\r'),
        Some('e') => short('\u{1b}'),
        Some('"') => short('"'),
        Some('\\') => short('\\'),
        Some('x') => code(after, 2),
        Some('u') => code(after, 4),
        Some('U') => code(after, 8),
        Some(' ' | '\t' | '\r' | '\n') => line_end(after),
        Some(_) => Escape::No,
    }
}

/// `\x`, `\u` or `\U` and the `digits` hex digits of a character's code;
/// `after` is what follows the backslash.
fn code(after: &str, digits: usize) -> Escape {
    let hex = after.get(1..).unwrap_or_default();
    let given = hex
        .bytes()
        .take(digits)
        .take_while(u8::is_ascii_hexdigit)
        .count();
    if given < digits {
        return if given == hex.len() {
            Escape::Open
        } else {
            Escape::No
        };
    }
    hex.get(..digits)
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .and_then(char::from_u32)
        .map_or(Escape::No, |c| Escape::Char(c, digits.saturating_add(2)))
}

/// A backslash that ends a line: white space, a line break, and every line
/// break and white space after it; `after` is what follows the backslash.
fn line_end(after: &str) -> Escape {
    let rest = after.trim_start_matches([' ', '\t']);
    let Some(next) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        // The text ends before the line does, or partway through its break.
        return if rest.is_empty() || rest == "\r" {
            Escape::Open
        } else {
            Escape::No
        };
    };
    let mut left = next;
    loop {
        let trimmed = left.trim_start_matches([' ', '\t', '\n']);
        match trimmed.strip_prefix("\r\n") {
            Some(further) => left = further,
            None => {
                // A text that ends between the CR and the LF of a later
                // break ends in the line end too.
                left = if trimmed == "\r" { "" } else { trimmed };
                break;
            }
        }
    }
    Escape::Nothing(after.len().saturating_sub(left.len()).saturating_add(1))
}

/// Whether `open`, an escape a cut went through, may be the start of how
/// TOML writes `c`: a backslash alone may be any escape, and `\u00` may be
/// `\u0063`, `c`. A line end begins no character: what follows it does.
fn may_begin(open: &str, c: char) -> bool {
    let Some(after) = open.strip_prefix('\\') else {
        return false;
    };
    let code = u32::from(c);
    let (digits, fits) = match after.chars().next() {
        None => return true,
        Some('x') => (2, code <= 0xff),
        Some('u') => (4, code <= 0xffff),
        Some('U') => (8, true),
        Some(_) => return false,
    };
    let given = after.get(1..).unwrap_or_default().to_ascii_lowercase();
    fits && format!("{code:0digits$x}").starts_with(&given)
}

/// Where a text was cut, as [`TomlRead`] reads it, or where a whole text
/// ends in a backslash that ends a line: one line of a file, masked on its
/// own, that a secret may go on after.
struct Cut {
    /// How many bytes of what was read come before the cut.
    read: usize,
    /// Where the cut is in the text.
    at: usize,
    /// Where an escape the cut went through starts in the text.
    open: Option<usize>,
}

/// A text as TOML reads the escapes in it: `read`, and for each byte of it
/// where in the text what it was read from starts (`from`) and ends (`to`).
/// A text masking reads so holds no more than `u32::MAX` bytes.
pub(super) struct TomlRead<'a> {
    text: &'a str,
    read: String,
    from: Vec<u32>,
    to: Vec<u32>,
    cut: Option<Cut>,
}

impl<'a> TomlRead<'a> {
    /// `text`, cut after its first `cut` bytes when that is given, as TOML
    /// reads it; `None` when no escape in it stands for anything else, which
    /// leaves nothing to look for that the text itself doesn't hold. A cut
    /// text with a backslash in it is read all the same: the cut may have
    /// gone through an escape. An escape is read on either side of the cut,
    /// never across it, and one the cut went through is said where it
    /// starts. A whole text that ends in a backslash that ends a line is
    /// read as cut before it, and the backslash and what follows it as text.
    pub(super) fn of(text: &'a str, cut: Option<usize>) -> Option<Self> {
        let escapes = (cut.is_some() && text.contains('\\'))
            || text.trim_end_matches([' ', '\t', '\r']).ends_with('\\')
            || text.match_indices('\\').any(|(at, _)| {
                let rest = text.get(at..).unwrap_or_default();
                matches!(escape(rest), Escape::Char(..) | Escape::Nothing(_))
            });
        if !escapes {
            return None;
        }
        let mut read = Self {
            text,
            read: String::with_capacity(text.len()),
            from: Vec::with_capacity(text.len()),
            to: Vec::with_capacity(text.len()),
            cut: None,
        };
        match cut {
            Some(at) => {
                let open = read.push(0, at, true);
                read.cut = Some(Cut {
                    read: read.read.len(),
                    at,
                    open,
                });
                read.push(at, text.len(), false);
            }
            None => {
                if let Some(open) = read.push(0, text.len(), true) {
                    // Only a line end can go on past the end of a whole
                    // text: one masked a line at a time (a worker's stderr,
                    // a log record) can't end partway through `\u0007`.
                    let after = text.get(open.saturating_add(1)..).unwrap_or_default();
                    if !after.starts_with(['x', 'u', 'U']) {
                        read.cut = Some(Cut {
                            read: read.read.len(),
                            at: text.len(),
                            open: None,
                        });
                    }
                    read.push(open, text.len(), false);
                }
            }
        }
        Some(read)
    }

    /// Reads the text from `start` to `end`, and says where an escape `end`
    /// went through starts, when `cut` says that is where it was cut; an
    /// escape the end of the text went through is text.
    fn push(&mut self, start: usize, end: usize, cut: bool) -> Option<usize> {
        let mut at = start;
        while let Some(rest) = self.text.get(at..end).filter(|rest| !rest.is_empty()) {
            let (c, len) = match escape(rest) {
                Escape::Char(c, len) => (Some(c), len),
                Escape::Nothing(len) => (None, len),
                Escape::Open if cut => return Some(at),
                Escape::Open | Escape::No => {
                    let c = rest.chars().next()?;
                    (Some(c), c.len_utf8())
                }
            };
            let next = at.saturating_add(len);
            if let Some(c) = c {
                self.read.push(c);
                for _ in 0..c.len_utf8() {
                    self.from.push(place(at));
                    self.to.push(place(next));
                }
            }
            at = next;
        }
        None
    }

    /// Marks in `spans` (the longest span of a needle that ends at each byte
    /// of the text, see `Redactor::mask_with`) every place `needle` is read
    /// in, as the bytes of the text it was read from; and, where the text was
    /// cut, the end of what was kept from where it is read as the start of
    /// `needle`, or an escape the cut went through that may be its first
    /// character.
    pub(super) fn mask(&self, needle: &Needle, spans: &mut [u32]) {
        let whole = needle.text.as_bytes();
        let cut_at = self.cut.as_ref().map(|cut| cut.read);
        let mut matched = 0;
        let mut at_cut = 0;
        for (at, &byte) in self.read.as_bytes().iter().enumerate() {
            matched = advance(whole, &needle.borders, matched, byte);
            let end = at.saturating_add(1);
            if matched as usize == whole.len() {
                self.cover_read(spans, end.saturating_sub(matched as usize), end);
            }
            if cut_at == Some(end) {
                at_cut = matched as usize;
            }
        }
        let Some(cut) = &self.cut else {
            return;
        };
        let start = if at_cut > 0 {
            cut.read
                .checked_sub(at_cut)
                .and_then(|first| self.from.get(first))
                .map(|&from| from as usize)
        } else {
            let first = needle.text.chars().next();
            cut.open.filter(|&open| {
                let escape = self.text.get(open..cut.at).unwrap_or_default();
                first.is_some_and(|first| may_begin(escape, first))
            })
        };
        if let Some(start) = start {
            cover(spans, start, cut.at);
        }
    }

    /// What was read from `at` on, where the white space the text begins
    /// with ends: white space as written is read as itself, so it ends at
    /// the same place in what was read, and white space written as an
    /// escape (`\x20`) is part of what follows it. The rest of a secret a
    /// line-ending backslash broke may be written with escapes ([`Head`]).
    pub(super) fn head(&self, at: usize, longest: usize) -> Option<Head<'_>> {
        Head::of(&self.read, at, longest)
    }

    /// Marks in `spans` the start of the text, from where `head`, the start
    /// of what was read, is the end of `needle`, as the bytes of the text it
    /// was read from.
    pub(super) fn mask_tail(&self, head: &Head<'_>, needle: &Needle, spans: &mut [u32]) {
        if let Some(tail) = head.tail(needle) {
            self.cover_read(spans, tail.start, tail.end);
        }
    }

    /// Marks the bytes of the text that bytes `start` to `end` of what was
    /// read were read from.
    fn cover_read(&self, spans: &mut [u32], start: usize, end: usize) {
        let from = self.from.get(start);
        let to = end.checked_sub(1).and_then(|last| self.to.get(last));
        if let (Some(&from), Some(&to)) = (from, to) {
            cover(spans, from as usize, to as usize);
        }
    }
}
