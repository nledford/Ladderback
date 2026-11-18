//! A tool call's arguments, read as the tool's argument type (§4.6). An
//! argument the tool can't take is the tool's `invalid-arguments` refusal,
//! naming the argument and what is wrong with it, never the value given: a
//! result the agent reads and can act on, where a protocol error is one
//! some clients show only to the person.

use api::{ArgumentProblem, ErrorCode, ErrorDto, MsgArg};
use rmcp::model::JsonObject;
use serde::de::value::MapDeserializer;
use serde::de::{DeserializeOwned, Deserializer, IntoDeserializer, Visitor};
use serde::forward_to_deserialize_any;
use serde_json::Value;

/// `given`, a call's arguments, read as `T`; none given reads as an empty
/// object.
///
/// # Errors
///
/// `invalid-arguments`: an argument `T` needs is missing, one it doesn't
/// take is given, or one is given a value it doesn't take.
pub fn read<T: DeserializeOwned>(given: Option<JsonObject>) -> Result<T, ErrorDto> {
    let given = given.unwrap_or_default();
    let fields = given
        .iter()
        .map(|(name, value)| (name.as_str(), Field { name, value }));
    T::deserialize(MapDeserializer::new(fields)).map_err(|wrong| wrong.refusal())
}

/// The refusal of the argument `field`, given a value the tool can't take,
/// as [`read`] refuses one: for a value only read further on.
pub fn invalid(field: &str) -> ErrorDto {
    Wrong::new(field, ArgumentProblem::Invalid).refusal()
}

/// The arguments of a tool that takes none.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoArguments {}

/// What is wrong with a call's arguments: the argument, and its problem.
#[derive(Debug, PartialEq, Eq)]
pub struct Wrong {
    field: String,
    problem: ArgumentProblem,
}

impl Wrong {
    fn new(field: &str, problem: ArgumentProblem) -> Self {
        Self {
            field: field.to_owned(),
            problem,
        }
    }

    /// The tool's refusal: `invalid-arguments`, naming the argument and its
    /// problem.
    fn refusal(self) -> ErrorDto {
        ErrorDto::new(
            ErrorCode::InvalidArguments,
            [
                ("field", MsgArg::Text(self.field)),
                ("problem", MsgArg::Text(self.problem.as_str().to_owned())),
            ],
            None,
        )
    }
}

impl std::fmt::Display for Wrong {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.field, self.problem.as_str())
    }
}

impl std::error::Error for Wrong {}

impl serde::de::Error for Wrong {
    /// Reached only for a fault of the arguments as a whole, which an
    /// argument type, a struct of named fields read from an object, never
    /// has: said as an invalid value of no argument in particular.
    fn custom<T: std::fmt::Display>(_message: T) -> Self {
        Self::new("", ArgumentProblem::Invalid)
    }

    fn missing_field(field: &'static str) -> Self {
        Self::new(field, ArgumentProblem::Missing)
    }

    fn unknown_field(field: &str, _expected: &'static [&'static str]) -> Self {
        Self::new(field, ArgumentProblem::UnknownKey)
    }
}

/// One argument's value, read under its name: a value the argument doesn't
/// take is [`ArgumentProblem::Invalid`] of that name.
struct Field<'de> {
    name: &'de str,
    value: &'de Value,
}

impl Field<'_> {
    fn invalid(&self) -> Wrong {
        Wrong::new(self.name, ArgumentProblem::Invalid)
    }
}

impl<'de> Deserializer<'de> for Field<'de> {
    type Error = Wrong;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Wrong> {
        self.value
            .deserialize_any(visitor)
            .map_err(|_| self.invalid())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Wrong> {
        self.value
            .deserialize_option(visitor)
            .map_err(|_| self.invalid())
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de> IntoDeserializer<'de, Wrong> for Field<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_wrong_says_the_argument_and_its_problem() {
        assert_eq!(
            Wrong::new("timeout", ArgumentProblem::Invalid).to_string(),
            "timeout: invalid"
        );
        assert_eq!(
            Wrong::new("plan", ArgumentProblem::Missing).to_string(),
            "plan: missing"
        );
    }
}
