//! What the catalog lane knows about `.ftl` sources: which messages and
//! attributes they define, which variables each text reads, and the rules the
//! catalogs and the code tables keep (technical design §8.4, "Catalog rules").
//! It reads the sources with `fluent-syntax`, so it sees them as a translator
//! wrote them, not as a bundle resolves them.

use std::collections::{BTreeMap, BTreeSet};

use api::{ArgKind, ArgSpec, MsgArg};
use fluent_syntax::ast;
use fluent_syntax::parser;
use l10n::code::{ERRORS, WARNINGS};

/// What one text, a message's value or one of its attributes, reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    /// Every variable the text names.
    pub variables: BTreeSet<String>,
    /// The variables handed to `NUMBER`, which only a count survives.
    pub numbers: BTreeSet<String>,
    /// The variables handed to `DATETIME`, which only a timestamp survives.
    pub datetimes: BTreeSet<String>,
    /// The select expressions, in order of appearance.
    pub selects: Vec<Select>,
    /// The variables read anywhere but inside a select that has a `[none]`
    /// arm, and outside that arm: where an optional argument may not be read.
    pub unguarded: BTreeSet<String>,
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

/// What the renderer binds an optional argument to when the sender left it out.
pub const NONE: &str = "none";

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
    walk_pattern(pattern, &[], &mut text);
    text
}

/// `guarded` holds the variables that a select with a `[none]` arm chooses
/// by: what it reads of them, it reads only when they are there.
fn walk_pattern(pattern: &ast::Pattern<&str>, guarded: &[String], text: &mut Text) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            walk_expression(expression, guarded, text);
        }
    }
}

fn walk_expression(expression: &ast::Expression<&str>, guarded: &[String], text: &mut Text) {
    match expression {
        ast::Expression::Inline(inline) => walk_inline(inline, guarded, text),
        ast::Expression::Select { selector, variants } => {
            let variable = selector_variable(selector);
            let keys: Vec<String> = variants
                .iter()
                .map(|variant| variant_key(&variant.key))
                .collect();
            let guards = variable
                .clone()
                .filter(|_| keys.iter().any(|key| key == NONE));
            let inside: Vec<String> = guarded.iter().cloned().chain(guards).collect();
            walk_inline(selector, &inside, text);
            if let Some(variable) = variable {
                text.selects.push(Select { variable, keys });
            }
            for variant in variants {
                // The `[none]` arm runs when the variable is absent, so reading
                // it there would show the stand-in.
                let absent = variant_key(&variant.key) == NONE;
                walk_pattern(&variant.value, if absent { guarded } else { &inside }, text);
            }
        }
    }
}

fn variant_key(key: &ast::VariantKey<&str>) -> String {
    match key {
        ast::VariantKey::Identifier { name } => (*name).to_owned(),
        ast::VariantKey::NumberLiteral { value } => (*value).to_owned(),
    }
}

fn walk_inline(inline: &ast::InlineExpression<&str>, guarded: &[String], text: &mut Text) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            let name = id.name.to_owned();
            if !guarded.contains(&name) {
                text.unguarded.insert(name.clone());
            }
            text.variables.insert(name);
        }
        ast::InlineExpression::FunctionReference { id, arguments } => {
            for positional in &arguments.positional {
                if id.name == "NUMBER"
                    && let ast::InlineExpression::VariableReference { id } = positional
                {
                    text.numbers.insert(id.name.to_owned());
                }
                if id.name == "DATETIME"
                    && let ast::InlineExpression::VariableReference { id } = positional
                {
                    text.datetimes.insert(id.name.to_owned());
                }
                walk_inline(positional, guarded, text);
            }
            for named in &arguments.named {
                walk_inline(&named.value, guarded, text);
            }
        }
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for positional in &arguments.positional {
                walk_inline(positional, guarded, text);
            }
            for named in &arguments.named {
                walk_inline(&named.value, guarded, text);
            }
        }
        ast::InlineExpression::Placeable { expression } => {
            walk_expression(expression, guarded, text);
        }
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
    let mut problems = Vec::new();
    for (id, message) in &source.messages {
        if !is_kebab_case(id) {
            problems.push(format!(
                "`{id}` is not a kebab-case ID (lowercase words joined by `-`)"
            ));
        }
        let reads_variables = message
            .value
            .iter()
            .chain(message.attributes.values())
            .any(|text| !text.variables.is_empty());
        if reads_variables && !message.commented {
            problems.push(format!(
                "`{id}` reads variables but has no translator comment above it"
            ));
        }
        for name in message.attributes.keys() {
            if !ATTRIBUTES.contains(&name.as_str()) {
                problems.push(format!(
                    "`{id}` has an attribute `.{name}` that no renderer reads"
                ));
            }
        }
    }
    problems
}

fn is_kebab_case(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What a translation must keep, against the source: no message the source
/// lacks, exactly the attributes the source's message has, no variable the
/// source's message doesn't read, and, in the message of a code, no read of an
/// optional argument that would show `none`. Messages it leaves out are fine:
/// they fall back.
pub fn translation_problems(source: &Locale, translation: &Locale) -> Vec<String> {
    let mut problems = Vec::new();
    for (id, message) in &translation.messages {
        let Some(original) = source.messages.get(id) else {
            problems.push(format!("`{id}` is not a message of the source locale"));
            continue;
        };
        for name in original.attributes.keys() {
            if !message.attributes.contains_key(name) {
                problems.push(format!(
                    "`{id}` lacks the attribute `.{name}` of the source's message"
                ));
            }
        }
        for name in message.attributes.keys() {
            if !original.attributes.contains_key(name) {
                problems.push(format!(
                    "`{id}` has an attribute `.{name}` the source's message lacks"
                ));
            }
        }
        let value = message.value.iter().zip(original.value.iter());
        let attributes = message.attributes.iter().filter_map(|(name, text)| {
            original
                .attributes
                .get(name)
                .map(|original_text| (text, original_text))
        });
        for (text, original_text) in value.chain(attributes) {
            for variable in text.variables.difference(&original_text.variables) {
                problems.push(format!(
                    "`{id}` reads `${variable}`, which the source's message doesn't"
                ));
            }
        }
        if let Some(declared) = declared_arguments(id) {
            problems.extend(optional_problems(id, declared, message));
        }
    }
    problems
}

/// The arguments declared by the code that the message `id` belongs to.
fn declared_arguments(id: &str) -> Option<&'static [ArgSpec]> {
    let error = ERRORS
        .iter()
        .find(|(_, mapped)| mapped.as_str() == id)
        .map(|(code, _)| code.args());
    error.or_else(|| {
        WARNINGS
            .iter()
            .find(|(_, mapped)| mapped.as_str() == id)
            .map(|(code, _)| code.args())
    })
}

/// The renderer binds an optional argument the sender left out to the text
/// `none`. So a text may read one only inside a select that has a `[none]`
/// arm, and not in that arm: anywhere else it would show the word. Checks the
/// value and every attribute.
fn optional_problems(id: &str, declared: &[ArgSpec], message: &Message) -> Vec<String> {
    let value = message.value.iter().map(|text| (format!("`{id}`"), text));
    let attributes = message
        .attributes
        .iter()
        .map(|(name, text)| (format!("`.{name}` of `{id}`"), text));
    let mut problems = Vec::new();
    for (place, text) in value.chain(attributes) {
        for arg in declared
            .iter()
            .filter(|arg| arg.optional && text.unguarded.contains(arg.name))
        {
            problems.push(format!(
                "{place} reads the optional `${}` outside a select with a `[none]` arm, so it can show \"none\"",
                arg.name
            ));
        }
    }
    problems
}

/// The messages named like a code's (`error-…`, `warning-…`) that none of the
/// `mapped` IDs names: a message nothing renders.
pub fn messages_without_code(locale: &Locale, mapped: &[&str]) -> Vec<String> {
    locale
        .messages
        .keys()
        .filter(|id| id.starts_with("error-") || id.starts_with("warning-"))
        .filter(|id| !mapped.contains(&id.as_str()))
        .map(|id| {
            format!("`{id}` is named like the message of a code, but no code is mapped to it")
        })
        .collect()
}

/// The `mapped` IDs the locale has no message with a value for: a code that
/// renders as its ID.
pub fn codes_without_message(locale: &Locale, mapped: &[&str]) -> Vec<String> {
    mapped
        .iter()
        .filter(|id| {
            locale
                .messages
                .get(**id)
                .is_none_or(|message| message.value.is_none())
        })
        .map(|id| {
            format!(
                "`{id}` is mapped to a code, but the catalog has no message with a value for it"
            )
        })
        .collect()
}

/// What is wrong between the arguments a code declares and a message: a
/// variable nobody sends renders as its name, an argument nobody reads is
/// dead weight, `NUMBER` over anything but a count renders nothing, and the
/// value must not read the detail, which only `.with-detail` shows.
pub fn argument_mismatches(id: &str, declared: &[ArgSpec], message: &Message) -> Vec<String> {
    let mut problems = Vec::new();
    let names: BTreeSet<&str> = declared.iter().map(|arg| arg.name).collect();
    let counts: BTreeSet<&str> = declared
        .iter()
        .filter(|arg| arg.kind == ArgKind::Count)
        .map(|arg| arg.name)
        .collect();
    if names.contains("detail") {
        problems.push(format!(
            "the code of `{id}` declares an argument named `detail`, which is the detail's name"
        ));
    }
    let Some(value) = &message.value else {
        problems.push(format!("`{id}` has no value"));
        return problems;
    };
    // The plain text reads exactly the declared arguments.
    for name in value
        .variables
        .iter()
        .filter(|name| !names.contains(name.as_str()))
    {
        problems.push(format!(
            "`{id}` reads `${name}`, which its code doesn't declare (only `.with-detail` shows the detail)"
        ));
    }
    for name in names
        .iter()
        .filter(|name| !value.variables.contains(**name))
    {
        problems.push(format!(
            "`{id}` never reads `${name}`, which its code declares"
        ));
    }
    problems.extend(optional_problems(id, declared, message));
    // The form with a detail shows the detail and reads declared arguments.
    if let Some(with_detail) = message.attributes.get("with-detail") {
        if !with_detail.variables.contains("detail") {
            problems.push(format!("`.with-detail` of `{id}` never shows `$detail`"));
        }
        for name in with_detail
            .variables
            .iter()
            .filter(|name| name.as_str() != "detail" && !names.contains(name.as_str()))
        {
            problems.push(format!(
                "`.with-detail` of `{id}` reads `${name}`, which its code doesn't declare"
            ));
        }
    }
    // `NUMBER` keeps a count numeric and turns anything else into nothing.
    for text in message.value.iter().chain(message.attributes.values()) {
        for name in text
            .numbers
            .iter()
            .filter(|name| !counts.contains(name.as_str()))
        {
            problems.push(format!(
                "`{id}` passes `${name}` to NUMBER, but its code doesn't declare `{name}` as a count"
            ));
        }
    }
    problems
}

/// Argument sets that, between them, take every branch of every select
/// expression in `text`: a count for each plural category, a word for each
/// other key, and a representative value for every other variable.
pub fn sample_args(text: &Text) -> Vec<Vec<(String, MsgArg)>> {
    let rounds = text
        .selects
        .iter()
        .map(|select| select.keys.len())
        .max()
        .unwrap_or(1)
        .max(1);
    (0..rounds)
        .map(|round| {
            let mut args: BTreeMap<String, MsgArg> = text
                .variables
                .iter()
                .map(|name| {
                    let sample = if text.numbers.contains(name) {
                        MsgArg::Count(3)
                    } else if text.datetimes.contains(name) {
                        // `DATETIME` renders only a timestamp; any other
                        // sample is a function error, and its placeholder is
                        // the same in every language.
                        MsgArg::Timestamp {
                            utc_ms: 1_762_700_000_000,
                            zone: None,
                        }
                    } else {
                        MsgArg::Text("demo".to_owned())
                    };
                    (name.clone(), sample)
                })
                .collect();
            for select in &text.selects {
                if let Some(key) = select.keys.get(round % select.keys.len().max(1)) {
                    args.insert(select.variable.clone(), key_sample(key));
                }
            }
            args.into_iter().collect()
        })
        .collect()
}

/// A value a select expression takes the variant `key` for: a count for a
/// plural category or a number, the word itself otherwise.
fn key_sample(key: &str) -> MsgArg {
    match key {
        "zero" => MsgArg::Count(0),
        "one" => MsgArg::Count(1),
        "two" => MsgArg::Count(2),
        "few" => MsgArg::Count(3),
        "many" => MsgArg::Count(11),
        "other" => MsgArg::Count(7),
        word => word
            .parse()
            .map_or_else(|_| MsgArg::Text(word.to_owned()), MsgArg::Count),
    }
}
