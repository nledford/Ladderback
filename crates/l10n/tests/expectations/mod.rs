//! The expectations the formatting hosts share (`formatting.json`), and the
//! driver that runs them through the public [`Localizer`], as a message is
//! written in the daemon, or in the browser.
//!
//! A lane includes this file with
//! `#[path = "../expectations/mod.rs"] mod expectations;` and checks the
//! [`Outcome`]s of one locale. It uses only the crate's public API, so the lane
//! of another host, such as the browser's `Intl` under wasm, includes it as it
//! is. Cargo doesn't build `tests/expectations/` as a test of its own: there is
//! no `main.rs` in it.
//!
//! A case is a JSON object with its `locale`, one input, and the text it
//! `expect`s:
//!
//! - `number`: an FTL number literal, written by `NUMBER(<literal>, <options>)`,
//!   whose `options` are named arguments, numbers or text.
//! - `count`, `size`, `duration`: a 64-bit number as text, passed to the message
//!   `{ $value }` as a `MsgArg::Count`, `Bytes` or `DurationMs`. The en-US
//!   catalog's own `size` and `duration` helpers write the last two, in the
//!   locale of the case.
//! - `instant`: `{ "utcMs": <number>, "zone": <IANA name>? }`, written by
//!   `DATETIME($value, <options>)` from a `MsgArg::Timestamp`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use api::{IanaZone, MsgArg};
use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};
use serde_json::{Map, Value};

const TABLE: &str = include_str!("formatting.json");

type Case = Map<String, Value>;

/// What the table expects of one case, and what the localizer wrote for it.
#[derive(Debug)]
pub struct Outcome {
    /// The case, as the table has it.
    pub case: String,
    pub expected: String,
    pub shown: String,
    /// What went wrong while the message was made: an unknown function, say.
    pub problems: Vec<String>,
}

impl Outcome {
    /// Whether the localizer wrote what the table expects, and nothing went
    /// wrong on the way.
    pub fn holds(&self) -> bool {
        self.shown == self.expected && self.problems.is_empty()
    }

    /// The case and how it went wrong, for the message of a failed assertion.
    /// The texts are quoted, so a no-break space can be told from a space.
    pub fn describe(&self) -> String {
        format!(
            "{}\n    expected {:?}\n    shown    {:?}\n    problems {:?}",
            self.case, self.expected, self.shown, self.problems
        )
    }
}

/// Writes every case of `locale` through a localizer that speaks it.
pub fn run(locale: &str) -> Vec<Outcome> {
    let at = LOCALES
        .iter()
        .position(|cases| cases.locale == locale)
        .unwrap_or_else(|| panic!("the table has no cases for {locale}"));
    let cases = &LOCALES[at];
    let catalog = Catalog {
        tag: &cases.locale,
        sources: &SOURCES[at],
    };
    let localizer =
        Localizer::from_catalogs(&[locale.parse().expect("a language tag")], &[catalog]);
    cases
        .cases
        .iter()
        .map(|case| {
            let written = localizer.message(MessageId::new(&case.id), &case.arguments);
            Outcome {
                case: case.json.clone(),
                expected: case.expected.clone(),
                problems: written.problems().to_vec(),
                shown: written.into_text(),
            }
        })
        .collect()
}

/// One case, ready to be written.
struct Prepared {
    /// The ID of its message in the locale's catalog.
    id: String,
    arguments: Vec<(&'static str, MsgArg)>,
    expected: String,
    json: String,
}

/// The cases of one locale, and the messages that write them.
struct Locale {
    locale: String,
    cases: Vec<Prepared>,
    /// The FTL of the cases' messages.
    ftl: String,
}

/// The cases of every locale, read once for the whole test run: a catalog
/// holds `'static` text.
static LOCALES: LazyLock<Vec<Locale>> = LazyLock::new(|| {
    let table: Value = serde_json::from_str(TABLE).expect("formatting.json is JSON");
    let mut by_locale: BTreeMap<String, Locale> = BTreeMap::new();
    for case in table["cases"]
        .as_array()
        .expect("formatting.json has a list of cases")
    {
        let case = case.as_object().expect("a case is an object");
        let name = text(case, "locale");
        let locale = by_locale.entry(name.to_owned()).or_insert_with(|| Locale {
            locale: name.to_owned(),
            cases: Vec::new(),
            ftl: String::new(),
        });
        let id = format!("case-{}", locale.cases.len());
        let (pattern, arguments) = message(case);
        writeln!(locale.ftl, "{id} = {{ {pattern} }}").expect("a string can be written to");
        locale.cases.push(Prepared {
            id,
            arguments,
            expected: text(case, "expect").to_owned(),
            json: serde_json::to_string(case).expect("a case is JSON"),
        });
    }
    by_locale.into_values().collect()
});

/// The files of each locale's catalog: the en-US catalog's, which have the
/// helpers `size` and `duration`, and the file of the cases.
static SOURCES: LazyLock<Vec<Vec<Source>>> = LazyLock::new(|| {
    let english = l10n::catalog::catalogs()
        .iter()
        .find(|catalog| catalog.tag == l10n::catalog::SOURCE_TAG)
        .expect("the source catalog is embedded");
    LOCALES
        .iter()
        .map(|locale| {
            let mut sources = english.sources.to_vec();
            sources.push(Source {
                name: "cases.ftl",
                text: &locale.ftl,
            });
            sources
        })
        .collect()
});

fn text<'a>(case: &'a Case, key: &str) -> &'a str {
    case.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} is text in {case:?}"))
}

/// The FTL of a case's placeable, and the arguments it is given.
fn message(case: &Case) -> (String, Vec<(&'static str, MsgArg)>) {
    if case.contains_key("number") {
        let call = format!("NUMBER({}{})", text(case, "number"), named(case));
        return (call, Vec::new());
    }
    let digits = |key: &str| -> u64 {
        text(case, key)
            .parse()
            .unwrap_or_else(|_| panic!("{key} is a 64-bit number in {case:?}"))
    };
    let (pattern, argument) = if case.contains_key("count") {
        ("$value".to_owned(), MsgArg::Count(digits("count")))
    } else if case.contains_key("size") {
        ("$value".to_owned(), MsgArg::Bytes(digits("size")))
    } else if case.contains_key("duration") {
        ("$value".to_owned(), MsgArg::DurationMs(digits("duration")))
    } else if let Some(instant) = case.get("instant") {
        let when = MsgArg::Timestamp {
            utc_ms: instant["utcMs"]
                .as_i64()
                .unwrap_or_else(|| panic!("utcMs is a number in {case:?}")),
            zone: instant["zone"].as_str().map(|name| {
                IanaZone::new(name).unwrap_or_else(|_| panic!("{name} is a zone name"))
            }),
        };
        (format!("DATETIME($value{})", named(case)), when)
    } else {
        panic!("a case with no input: {case:?}")
    };
    (pattern, vec![("value", argument)])
}

/// A case's `options` as the named arguments of an FTL call: `, name: value`.
fn named(case: &Case) -> String {
    let mut arguments = String::new();
    let Some(options) = case.get("options").and_then(Value::as_object) else {
        return arguments;
    };
    for (name, value) in options {
        match value {
            Value::Number(number) => write!(arguments, ", {name}: {number}"),
            Value::String(word) => {
                assert!(
                    word.chars().all(|c| c.is_ascii_alphanumeric()),
                    "{word:?} is a word"
                );
                write!(arguments, ", {name}: \"{word}\"")
            }
            other => panic!("option {name} is a number or a word, not {other}"),
        }
        .expect("a string can be written to");
    }
    arguments
}
