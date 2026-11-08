//! What the catalog lane knows about `.ftl` sources: which messages and
//! attributes they define, which variables each text reads, and the rules the
//! catalogs and the code tables keep (technical design §8.4, "Catalog rules").
//! It reads the sources with `fluent-syntax`, so it sees them as a translator
//! wrote them, not as a bundle resolves them.

use std::collections::{BTreeMap, BTreeSet};

use api::{ArgSpec, MsgArg};
use fluent_syntax::ast;
use fluent_syntax::parser;

/// What one text, a message's value or one of its attributes, reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    /// Every variable the text names.
    pub variables: BTreeSet<String>,
    /// The variables handed to `NUMBER`, which only a count survives.
    pub numbers: BTreeSet<String>,
    /// The select expressions, in order of appearance.
    pub selects: Vec<Select>,
}

/// A select expression over a variable, and the keys of its variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select {
    pub variable: String,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    /// Whether a comment sits right above the message.
    pub commented: bool,
    pub value: Option<Text>,
    pub attributes: BTreeMap<String, Text>,
}

/// The messages of one locale, from all its files.
#[derive(Clone, Debug, Default)]
pub struct Locale {
    pub messages: BTreeMap<String, Message>,
}

/// The attributes a renderer reads. A message carrying another is a typo.
pub const ATTRIBUTES: &[&str] = &["with-detail"];

/// Reads one locale's files, given as `(name, text)`. What a bundle would
/// refuse comes back as problems, each naming its file: a syntax error, or an
/// ID defined twice (in one file or across files).
pub fn parse(files: &[(&str, &str)]) -> Result<Locale, Vec<String>> {
    let mut problems = Vec::new();
    let mut messages = BTreeMap::new();
    for (name, text) in files {
        let resource = match parser::parse(*text) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                problems.extend(errors.iter().map(|error| format!("{name}: {error}")));
                resource
            }
        };
        for entry in &resource.body {
            let ast::Entry::Message(message) = entry else {
                continue;
            };
            let id = message.id.name.to_owned();
            let read = Message {
                commented: message.comment.is_some(),
                value: message.value.as_ref().map(read_pattern),
                attributes: message
                    .attributes
                    .iter()
                    .map(|attribute| (attribute.id.name.to_owned(), read_pattern(&attribute.value)))
                    .collect(),
            };
            if messages.insert(id.clone(), read).is_some() {
                problems.push(format!("{name}: `{id}` is defined twice"));
            }
        }
    }
    if problems.is_empty() {
        Ok(Locale { messages })
    } else {
        Err(problems)
    }
}

fn read_pattern(pattern: &ast::Pattern<&str>) -> Text {
    let mut text = Text::default();
    walk_pattern(pattern, &mut text);
    text
}

fn walk_pattern(pattern: &ast::Pattern<&str>, text: &mut Text) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            walk_expression(expression, text);
        }
    }
}

fn walk_expression(expression: &ast::Expression<&str>, text: &mut Text) {
    match expression {
        ast::Expression::Inline(inline) => walk_inline(inline, text),
        ast::Expression::Select { selector, variants } => {
            walk_inline(selector, text);
            if let Some(variable) = selector_variable(selector) {
                let keys = variants
                    .iter()
                    .map(|variant| match &variant.key {
                        ast::VariantKey::Identifier { name } => (*name).to_owned(),
                        ast::VariantKey::NumberLiteral { value } => (*value).to_owned(),
                    })
                    .collect();
                text.selects.push(Select { variable, keys });
            }
            for variant in variants {
                walk_pattern(&variant.value, text);
            }
        }
    }
}

fn walk_inline(inline: &ast::InlineExpression<&str>, text: &mut Text) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            text.variables.insert(id.name.to_owned());
        }
        ast::InlineExpression::FunctionReference { id, arguments } => {
            for positional in &arguments.positional {
                if id.name == "NUMBER"
                    && let ast::InlineExpression::VariableReference { id } = positional
                {
                    text.numbers.insert(id.name.to_owned());
                }
                walk_inline(positional, text);
            }
            for named in &arguments.named {
                walk_inline(&named.value, text);
            }
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for positional in &arguments.positional {
                walk_inline(positional, text);
            }
            for named in &arguments.named {
                walk_inline(&named.value, text);
            }
        }
        ast::InlineExpression::Placeable { expression } => walk_expression(expression, text),
        ast::InlineExpression::TermReference { .. }
        | ast::InlineExpression::StringLiteral { .. }
        | ast::InlineExpression::NumberLiteral { .. }
        | ast::InlineExpression::MessageReference { .. } => {}
    }
}

/// The variable a select expression chooses by: `$unit`, or the variable
/// inside `NUMBER($count)`.
fn selector_variable(selector: &ast::InlineExpression<&str>) -> Option<String> {
    match selector {
        ast::InlineExpression::VariableReference { id } => Some(id.name.to_owned()),
        ast::InlineExpression::FunctionReference { id, arguments } if id.name == "NUMBER" => {
            match arguments.positional.first() {
                Some(ast::InlineExpression::VariableReference { id }) => Some(id.name.to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// What the source catalog must keep beyond parsing: IDs are kebab-case, a
/// message with variables has a translator comment, and an attribute is one
/// a renderer reads.
pub fn source_problems(source: &Locale) -> Vec<String> {
    let _ = source;
    todo!()
}

/// What a translation must keep, against the source: no message the source
/// lacks, exactly the attributes the source's message has, and no variable
/// the source's message doesn't read. Messages it leaves out are fine: they
/// fall back.
pub fn translation_problems(source: &Locale, translation: &Locale) -> Vec<String> {
    let _ = (source, translation);
    todo!()
}

/// The messages named like a code's (`error-…`, `warning-…`) that none of the
/// `mapped` IDs names: a message nothing renders.
pub fn messages_without_code(locale: &Locale, mapped: &[&str]) -> Vec<String> {
    let _ = (locale, mapped);
    todo!()
}

/// The `mapped` IDs the locale has no message with a value for: a code that
/// renders as its ID.
pub fn codes_without_message(locale: &Locale, mapped: &[&str]) -> Vec<String> {
    let _ = (locale, mapped);
    todo!()
}

/// What is wrong between the arguments a code declares and a message: a
/// variable nobody sends renders as its name, an argument nobody reads is
/// dead weight, `NUMBER` over anything but a count renders nothing, and the
/// value must not read the detail, which only `.with-detail` shows.
pub fn argument_mismatches(id: &str, declared: &[ArgSpec], message: &Message) -> Vec<String> {
    let _ = (id, declared, message);
    todo!()
}

/// Argument sets that, between them, take every branch of every select
/// expression in `text`: a count for each plural category, a word for each
/// other key, and a representative value for every other variable.
pub fn sample_args(text: &Text) -> Vec<Vec<(String, MsgArg)>> {
    let _ = text;
    todo!()
}
