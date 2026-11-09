//! Where in a file a setting is: key paths such as `plans[1].schedule.cron`,
//! the line and column of a path or of a byte offset, and the words of a
//! parser's error with every value left out of them.

use std::ops::Range;

use app::ports::Position;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// A parsed file, with the place of every key and value in it.
pub(crate) type Doc<'i> = Spanned<DeTable<'i>>;

/// One step down from a table: into one of its keys, or one of an array's
/// items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Key(String),
    Index(usize),
}

/// The way down to a setting, from the table of the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Path(Vec<Step>);

impl Path {
    pub(crate) fn key(&self, key: &str) -> Self {
        self.with(Step::Key(key.to_owned()))
    }

    pub(crate) fn index(&self, index: usize) -> Self {
        self.with(Step::Index(index))
    }

    fn with(&self, step: Step) -> Self {
        let mut steps = self.0.clone();
        steps.push(step);
        Self(steps)
    }

    /// `plans[1].schedule.cron`, or `None` for the file itself.
    pub(crate) fn text(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let mut text = String::new();
        for step in &self.0 {
            match step {
                Step::Key(key) => {
                    if !text.is_empty() {
                        text.push('.');
                    }
                    text.push_str(key);
                }
                Step::Index(index) => text.push_str(&format!("[{index}]")),
            }
        }
        Some(text)
    }
}

/// The line and column, both from 1, of the byte at `offset` in `text`. A
/// column counts characters, not bytes.
pub(crate) fn position(text: &str, offset: usize) -> Position {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |start| start.chars().count())
        + 1;
    Position { line, column }
}

/// A key of a table, with its place.
type Key<'i> = Spanned<std::borrow::Cow<'i, str>>;

/// A value, with its place.
type Node<'i> = Spanned<DeValue<'i>>;

fn entry<'a, 'i>(table: &'a DeTable<'i>, key: &str) -> Option<(&'a Key<'i>, &'a Node<'i>)> {
    table
        .iter()
        .find(|(name, _)| name.get_ref().as_ref() == key)
}

/// The key and the value the path leads to. The key is the last one of the
/// path, or `None` for an item of an array.
fn walk<'a, 'i>(doc: &'a Doc<'i>, path: &Path) -> Option<(Option<&'a Key<'i>>, &'a Node<'i>)> {
    let mut steps = path.0.iter();
    let Some(Step::Key(first)) = steps.next() else {
        return None;
    };
    let (key, value) = entry(doc.get_ref(), first)?;
    let mut found = (Some(key), value);
    for step in steps {
        let next = match (found.1.get_ref(), step) {
            (DeValue::Table(table), Step::Key(key)) => {
                let (key, value) = entry(table, key)?;
                (Some(key), value)
            }
            (DeValue::Array(items), Step::Index(index)) => (None, items.get(*index)?),
            _ => return None,
        };
        found = next;
    }
    Some(found)
}

/// Where the value at `path` is written.
pub(crate) fn value_span(doc: &Doc<'_>, path: &Path) -> Option<Range<usize>> {
    walk(doc, path).map(|(_, value)| value.span())
}

/// Where the key at the end of `path` is written, or its value for an item of
/// an array, which has no key.
pub(crate) fn key_span(doc: &Doc<'_>, path: &Path) -> Option<Range<usize>> {
    walk(doc, path).map(|(key, value)| key.map_or_else(|| value.span(), Spanned::span))
}

/// The path of the key or value that `span` is the place of, and whether it is
/// the key's.
pub(crate) fn path_of(doc: &Doc<'_>, span: &Range<usize>) -> Option<(Path, bool)> {
    in_table(doc.get_ref(), span, &Path::default())
}

fn in_table(table: &DeTable<'_>, span: &Range<usize>, at: &Path) -> Option<(Path, bool)> {
    for (key, value) in table.iter() {
        let here = at.key(key.get_ref());
        if key.span() == *span {
            return Some((here, true));
        }
        if value.span() == *span {
            return Some((here, false));
        }
        if let Some(found) = in_value(value.get_ref(), span, &here) {
            return Some(found);
        }
    }
    None
}

fn in_value(value: &DeValue<'_>, span: &Range<usize>, at: &Path) -> Option<(Path, bool)> {
    match value {
        DeValue::Table(table) => in_table(table, span, at),
        DeValue::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = at.index(index);
                if item.span() == *span {
                    return Some((here, false));
                }
                if let Some(found) = in_value(item.get_ref(), span, &here) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// What a parser said, without any text of the file in it: what is between
/// backticks or double quotes is a value that was in the file, and the file
/// may hold a secret. A double-quoted text is written as Rust's `{:?}` writes
/// a string, so a backslash escapes the character after it: `\"` is part of
/// the text, not its end.
pub(crate) fn scrub(message: &str) -> String {
    let mut kept = String::with_capacity(message.len());
    let mut inside: Option<char> = None;
    let mut escaped = false;
    for c in message.chars() {
        match inside {
            Some(_) if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(quote) if c == quote => inside = None,
            Some(_) => {}
            None if c == '`' || c == '"' => inside = Some(c),
            None => kept.push(c),
        }
    }
    kept.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .trim_end_matches([',', ':', ' '])
        .to_owned()
}
