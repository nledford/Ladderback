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

impl Placeholder {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "event" => Self::Event,
            "plan" => Self::Plan,
            "repository" => Self::Repository,
            "operation" => Self::Operation,
            "code" => Self::Code,
            "time" => Self::Time,
            _ => return None,
        })
    }
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
        if text.is_empty() || text.len() > MAX_TEMPLATE_BYTES {
            return Err(refused());
        }
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(at) = rest.find(['{', '}']) {
            let (before, from) = rest.split_at(at);
            literal.push_str(before);
            if let Some(after) = from.strip_prefix("{{") {
                literal.push('{');
                rest = after;
            } else if let Some(after) = from.strip_prefix("}}") {
                literal.push('}');
                rest = after;
            } else if let Some(open) = from.strip_prefix('{') {
                let end = open.find('}').ok_or_else(refused)?;
                let (name, after) = open.split_at(end);
                let placeholder = Placeholder::parse(name).ok_or_else(refused)?;
                if !literal.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut literal)));
                }
                parts.push(Part::Value(placeholder));
                rest = after.get(1..).unwrap_or_default();
            } else {
                // A `}` with no `{` before it and none after it.
                return Err(refused());
            }
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        Ok(Self {
            text: text.to_owned(),
            parts,
        })
    }

    /// The text the template was read from.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The message for `event`: the template's text with each placeholder's
    /// fact in its place, and nothing for a fact the event doesn't have.
    pub fn render(&self, event: &Event) -> String {
        let mut message = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => message.push_str(text),
                Part::Value(Placeholder::Event) => message.push_str(event.kind.name()),
                Part::Value(Placeholder::Plan) => {
                    if let Some(plan) = &event.plan {
                        message.push_str(plan.as_str());
                    }
                }
                Part::Value(Placeholder::Repository) => {
                    if let Some(repository) = &event.repository {
                        message.push_str(repository.as_str());
                    }
                }
                Part::Value(Placeholder::Operation) => {
                    message.push_str(&event.operation.as_uuid().to_string());
                }
                Part::Value(Placeholder::Code) => {
                    if let Some(code) = event.code {
                        message.push_str(code);
                    }
                }
                Part::Value(Placeholder::Time) => message.push_str(&event.at.to_string()),
            }
        }
        message
    }
}

fn refused() -> InvalidValue {
    InvalidValue::new(ValueKind::NotificationTemplate, "")
}
