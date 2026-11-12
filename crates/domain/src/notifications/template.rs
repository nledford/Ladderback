//! A rule's template (§11): the user's own words, with placeholders for the
//! facts of the event. Not localized: it says what the user wrote.
//!
//! A placeholder is a name in braces, `{plan}`, from a closed list:
//! `{event}`, `{plan}`, `{repository}`, `{operation}`, `{code}` and `{time}`.
//! None of them can hold a secret (I4), and a name not on the list is
//! refused when the template is read, so a template can never ask for one.
//! `{{` and `}}` write a brace.

use super::Event;
use crate::planning::{InvalidValue, ValueKind};

/// The longest template, in bytes. A template is read from `config.toml`,
/// so its length is input; with each placeholder's value capped too (an ID
/// is at most 64 bytes, a time 30), a rendered message stays small.
pub const MAX_TEMPLATE_BYTES: usize = 2048;

/// One fact of an event a template can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placeholder {
    /// The event's name: `backup.failed`.
    Event,
    /// The backup's plan.
    Plan,
    /// The repository's alias.
    Repository,
    /// The operation's ID.
    Operation,
    /// The failure's stored code.
    Code,
    /// When it happened, in RFC 3339, UTC.
    Time,
}

/// A piece of a template: text as written, or a fact.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Text(String),
    Value(Placeholder),
}

/// A template that has been read, with the text it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    text: String,
    parts: Vec<Part>,
}

impl Template {
    /// The template `text` writes, or [`ValueKind::NotificationTemplate`]
    /// when it is empty, longer than [`MAX_TEMPLATE_BYTES`], names a
    /// placeholder that isn't on the list, or leaves a brace unclosed or
    /// unopened. The refusal keeps none of the text, which may be anything.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let _ = text;
        todo!()
    }

    /// The text the template was read from.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The message for `event`: the template's text with each placeholder's
    /// fact in its place, and nothing for a fact the event doesn't have.
    pub fn render(&self, event: &Event) -> String {
        let _ = event;
        todo!()
    }
}

#[allow(dead_code, reason = "the red test commit builds the parts in the next commit")]
fn refused() -> InvalidValue {
    InvalidValue::new(ValueKind::NotificationTemplate, "")
}

#[allow(dead_code, reason = "the red test commit builds the parts in the next commit")]
fn parts(template: &Template) -> &[Part] {
    &template.parts
}
