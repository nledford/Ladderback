//! One-shot calls (§4.6): `ladderback mcp call` runs tools without a client,
//! through the dispatch `tools/call` uses. A call is read from the command
//! line or a script's line into a [`Call`]; its [`Answer`] is printed as one
//! JSON line, its [`Outcome`].

use api::ErrorDto;
use rmcp::model::JsonObject;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Offer, ToolName, tools};

/// One call of a tool, read once where it enters: a tool that exists, and
/// arguments that are an object, for the tool to check as over MCP.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub(crate) tool: ToolName,
    pub(crate) arguments: JsonObject,
}

/// Why a call couldn't be read. None of them names a value given: a value
/// may be a profile's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallRefused {
    /// No tool has the name given.
    NoTool,
    /// The word at this position after the tool's name, from 1, isn't
    /// written as `name=value`.
    NotAPair(usize),
    /// The argument of this name is given more than once.
    Repeated(String),
    /// The arguments given as JSON aren't one object.
    NotAnObject,
    /// A script's line isn't a call: one object, the tool's name under
    /// `tool`, and its arguments, an object, under `arguments` when it takes
    /// any.
    NotACall,
}

/// A script's line as written: its arguments are read once the tool is
/// known to exist.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    tool: String,
    #[serde(default)]
    arguments: Option<Value>,
}

impl Call {
    /// The name of the tool called.
    pub fn tool(&self) -> &'static str {
        self.tool.name()
    }

    /// The arguments given.
    pub fn arguments(&self) -> &JsonObject {
        &self.arguments
    }

    /// The call of `tool` with `words`, each `name=value`: the value read as
    /// the type the tool's input schema gives the argument, an integer or a
    /// boolean when it is written as one, and as text otherwise.
    ///
    /// # Errors
    ///
    /// No tool is named `tool`; a word isn't `name=value`; or a name is
    /// given twice.
    pub fn of_words(tool: &str, words: &[String]) -> Result<Self, CallRefused> {
        let tool = ToolName::named(tool).ok_or(CallRefused::NoTool)?;
        let schema = tools()
            .into_iter()
            .find(|listed| listed.name == tool.name())
            .map(|listed| listed.input_schema)
            .unwrap_or_default();
        let mut arguments = JsonObject::new();
        for (position, word) in (1..).zip(words) {
            let Some((name, value)) = word.split_once('=').filter(|(name, _)| !name.is_empty())
            else {
                return Err(CallRefused::NotAPair(position));
            };
            if arguments.contains_key(name) {
                return Err(CallRefused::Repeated(name.to_owned()));
            }
            arguments.insert(name.to_owned(), typed(&schema, name, value));
        }
        Ok(Self { tool, arguments })
    }

    /// The call of `tool` with `arguments`, one JSON object.
    ///
    /// # Errors
    ///
    /// No tool is named `tool`, or `arguments` isn't one JSON object.
    pub fn of_object(tool: &str, arguments: &str) -> Result<Self, CallRefused> {
        let tool = ToolName::named(tool).ok_or(CallRefused::NoTool)?;
        match serde_json::from_str(arguments) {
            Ok(Value::Object(arguments)) => Ok(Self { tool, arguments }),
            _ => Err(CallRefused::NotAnObject),
        }
    }

    /// The call a script's `line` holds: `{"tool", "arguments"}`.
    ///
    /// # Errors
    ///
    /// The line isn't such an object, or names no tool.
    pub fn of_line(line: &str) -> Result<Self, CallRefused> {
        // A struct reads from an array too: a line is an object first.
        let Ok(Value::Object(line)) = serde_json::from_str(line) else {
            return Err(CallRefused::NotACall);
        };
        let Line { tool, arguments } =
            serde_json::from_value(Value::Object(line)).map_err(|_| CallRefused::NotACall)?;
        let tool = ToolName::named(&tool).ok_or(CallRefused::NoTool)?;
        match arguments {
            None => Ok(Self {
                tool,
                arguments: JsonObject::new(),
            }),
            Some(Value::Object(arguments)) => Ok(Self { tool, arguments }),
            Some(_) => Err(CallRefused::NotACall),
        }
    }

    /// Whether a server of `offer` offers the tool called: a proposal tool
    /// called where none is offered names no tool there, as over MCP.
    pub fn offered_in(&self, offer: Offer) -> bool {
        self.tool.offered_in(offer)
    }
}

/// `value` as the type `name` has in the input schema `schema`.
pub(crate) fn typed(schema: &JsonObject, name: &str, value: &str) -> Value {
    let kind = schema
        .get("properties")
        .and_then(|properties| properties.get(name))
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str);
    let read = match kind {
        Some("integer") => value
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| value.parse::<u64>().map(Value::from))
            .ok(),
        Some("boolean") => value.parse::<bool>().map(Value::from).ok(),
        _ => None,
    };
    read.unwrap_or_else(|| Value::from(value))
}

/// What a tool gave a one-shot call: what `tools/call` gives over MCP, as
/// its `structuredContent` or its refusal.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// What the tool read or did.
    Done(Value),
    /// The tool's refusal, `{code, args}`.
    Refused(ErrorDto),
    /// A result that couldn't be written as JSON, which no tool's result
    /// is: why.
    Unwritten(String),
}

impl Answer {
    /// Whether the call worked.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Done(_))
    }

    /// The line a one-shot call prints for this answer of `tool`.
    pub fn outcome<'a>(&'a self, tool: &'a str) -> Outcome<'a> {
        let (result, error) = match self {
            Self::Done(value) => (Some(value), None),
            Self::Refused(error) => (None, Some(Failure::Refused(error))),
            Self::Unwritten(why) => (None, Some(Failure::Unwritten(why))),
        };
        Outcome {
            tool,
            ok: self.is_ok(),
            result,
            error,
        }
    }
}

/// A one-shot call's line: `{"tool", "ok", "result"}`, or
/// `{"tool", "ok": false, "error"}`.
#[derive(Debug, Serialize)]
pub struct Outcome<'a> {
    tool: &'a str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure<'a>>,
}

/// A failed call's `error`: the tool's refusal, or why its result couldn't
/// be written.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Failure<'a> {
    Refused(&'a ErrorDto),
    Unwritten(&'a str),
}
