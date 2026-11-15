//! One-shot calls (§4.6): `ladderback mcp call` runs tools without a client,
//! through the dispatch `tools/call` uses. A call is read from the command
//! line or a script's line into a [`Call`]; its [`Answer`] is printed as one
//! JSON line, its [`Outcome`].

use api::ErrorDto;
use rmcp::model::JsonObject;
use serde::Serialize;
use serde_json::Value;

use crate::ToolName;

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

impl Call {
    /// The name of the tool called.
    pub fn tool(&self) -> &'static str {
        todo!()
    }

    /// The arguments given.
    pub fn arguments(&self) -> &JsonObject {
        todo!()
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
        let _ = (tool, words);
        todo!()
    }

    /// The call of `tool` with `arguments`, one JSON object.
    ///
    /// # Errors
    ///
    /// No tool is named `tool`, or `arguments` isn't one JSON object.
    pub fn of_object(tool: &str, arguments: &str) -> Result<Self, CallRefused> {
        let _ = (tool, arguments);
        todo!()
    }

    /// The call a script's `line` holds: `{"tool", "arguments"}`.
    ///
    /// # Errors
    ///
    /// The line isn't such an object, or names no tool.
    pub fn of_line(line: &str) -> Result<Self, CallRefused> {
        let _ = line;
        todo!()
    }
}

/// `value` as the type `name` has in the input schema `schema`.
pub(crate) fn typed(schema: &JsonObject, name: &str, value: &str) -> Value {
    let _ = (schema, name, value);
    todo!()
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
        todo!()
    }

    /// The line a one-shot call prints for this answer of `tool`.
    pub fn outcome<'a>(&'a self, tool: &'a str) -> Outcome<'a> {
        let _ = tool;
        todo!()
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
