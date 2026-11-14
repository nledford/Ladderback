//! The catalog lane (technical design §8.4, "Tests"): what every `.ftl` file
//! keeps, that each message of the source catalog formats, and how sizes and
//! durations read in the catalog's own words.

use std::collections::BTreeSet;

use api::MsgArg;
use l10n::catalog::{Catalog, SOURCE_TAG, Source, catalogs};
use l10n::{Localizer, MessageId, msg};

use crate::support::catalog::{self as rules, sample_args, source_problems, translation_problems};
use crate::support::{borrowed, english, files, locale, source_locale, tag};

const ISOLATION_MARKS: [char; 2] = ['\u{2068}', '\u{2069}'];

#[test]
fn every_embedded_file_parses_and_defines_each_id_once() {
    assert!(
        catalogs().iter().any(|catalog| catalog.tag == SOURCE_TAG),
        "the source catalog is embedded"
    );
    for catalog in catalogs() {
        if let Err(problems) = rules::parse(&files(catalog)) {
            panic!("{}: {problems:#?}", catalog.tag);
        }
    }
}

#[test]
fn the_message_constants_list_the_source_catalogs_messages_once_each() {
    let source = source_locale();
    let generated: BTreeSet<&str> = msg::ALL.iter().map(|id| id.as_str()).collect();
    let parsed: BTreeSet<&str> = source.messages.keys().map(String::as_str).collect();
    assert!(!generated.is_empty());
    assert_eq!(msg::ALL.len(), generated.len(), "an ID is listed twice");
    assert_eq!(generated, parsed);
}

#[test]
fn every_source_message_formats_without_problems_in_every_branch() {
    let source = source_locale();
    let localizer = english();
    assert!(!msg::ALL.is_empty());
    for id in msg::ALL {
        let value = source
            .messages
            .get(id.as_str())
            .and_then(|message| message.value.as_ref())
            .unwrap_or_else(|| panic!("`{}` has a value", id.as_str()));
        for args in sample_args(value) {
            let shown = localizer.message(*id, &borrowed(&args));
            assert!(
                shown.problems().is_empty(),
                "{} with {args:?}: {:?}",
                id.as_str(),
                shown.problems()
            );
            assert!(
                !shown.text().trim().is_empty(),
                "{} with {args:?} renders as nothing",
                id.as_str()
            );
            assert!(
                !shown.text().contains(ISOLATION_MARKS),
                "{} shows isolation marks while they are off",
                id.as_str()
            );
        }
    }
}

#[test]
fn isolation_marks_only_wrap_the_values_and_change_nothing_else() {
    let source = source_locale();
    let plain = english();
    let isolating = Localizer::new(&[]).with_isolation(true);
    let mut wrapped = 0;
    assert!(!msg::ALL.is_empty());
    for id in msg::ALL {
        let value = source
            .messages
            .get(id.as_str())
            .and_then(|message| message.value.as_ref())
            .unwrap_or_else(|| panic!("`{}` has a value", id.as_str()));
        for args in sample_args(value) {
            let args = borrowed(&args);
            let marked = isolating.message(*id, &args);
            assert!(
                marked.problems().is_empty(),
                "{}: {:?}",
                id.as_str(),
                marked.problems()
            );
            let stripped: String = marked
                .text()
                .chars()
                .filter(|c| !ISOLATION_MARKS.contains(c))
                .collect();
            assert_eq!(
                stripped,
                plain.message(*id, &args).text(),
                "{}",
                id.as_str()
            );
            if marked.text().contains(ISOLATION_MARKS) {
                wrapped += 1;
            }
        }
    }
    assert!(wrapped > 0, "no message showed an isolation mark");
}

#[test]
fn the_source_catalog_keeps_the_catalog_rules() {
    let problems = source_problems(&source_locale());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_catalog_rules_name_what_they_catch() {
    let problems = |text: &str| source_problems(&locale(text));
    assert!(problems("plan-run-now = Run now\n").is_empty());
    assert!(problems("# { $name } is the plan's name.\nplan-title = Plan { $name }\n").is_empty());
    // IDs are kebab-case.
    assert_eq!(problems("planRunNow = Run now\n").len(), 1);
    assert_eq!(problems("plan_run_now = Run now\n").len(), 1);
    // A message with variables says what they hold.
    assert_eq!(problems("plan-title = Plan { $name }\n").len(), 1);
    // An attribute no renderer reads is a typo.
    let typo =
        "# $detail is rustic's message.\nplan-run = Run\n    .with-details = Run: { $detail }\n";
    assert_eq!(problems(typo).len(), 1);
}

#[test]
fn a_translation_may_leave_messages_out_but_not_add_or_reshape_them() {
    let source = locale(
        "hello = Hello\n\
         error-x = Failed.\n    .with-detail = Failed: { $detail }\n\
         greeting = Hi { $name }\n",
    );
    let check = |text: &str| translation_problems(&source, &locale(text));
    // Messages it leaves out fall back.
    assert!(check("").is_empty());
    assert!(check("hello = Hallo\n").is_empty());
    assert!(
        check("error-x = Fehler.\n    .with-detail = Fehler: { $detail }\ngreeting = Hallo { $name }\n")
            .is_empty()
    );
    // A message the source lacks, an attribute it lacks or leaves out, a
    // variable it doesn't read.
    assert_eq!(check("extra = Zusatz\n").len(), 1);
    assert_eq!(check("error-x = Fehler.\n").len(), 1);
    assert_eq!(
        check("hello = Hallo\n    .with-detail = Hallo: { $detail }\n").len(),
        1
    );
    assert_eq!(check("greeting = Hallo { $name } { $other }\n").len(), 1);

    // And so do the translations that ship: fallback happens per message.
    let source = source_locale();
    for catalog in catalogs()
        .iter()
        .filter(|catalog| catalog.tag != SOURCE_TAG)
    {
        let translation = rules::parse(&files(catalog))
            .unwrap_or_else(|problems| panic!("{}: {problems:#?}", catalog.tag));
        let problems = translation_problems(&source, &translation);
        assert!(problems.is_empty(), "{}: {problems:#?}", catalog.tag);
    }
}

/// A `de` catalog with one message that only shows its value, so the sizes
/// and durations in it are written by the embedded `en-US` messages.
const CARRIER: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "carrier.ftl",
        text: "carrier = { $value }\n",
    }],
};

fn carrier() -> Localizer {
    let mut all = catalogs().to_vec();
    all.push(CARRIER);
    Localizer::from_catalogs(&[tag("de")], &all)
}

fn carried(localizer: &Localizer, value: MsgArg) -> String {
    let shown = localizer.message(MessageId::new("carrier"), &[("value", value)]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    shown.into_text()
}

#[test]
fn a_size_reads_in_the_unit_the_catalog_names_for_it() {
    let localizer = carrier();
    for (bytes, text) in [
        (0, "0 B"),
        (1023, "1,023 B"),
        (1536, "1.5 KiB"),
        (1_048_575, "1 MiB"),
        (5 << 20, "5 MiB"),
        (5_905_580_032, "5.5 GiB"),
        (3 << 40, "3 TiB"),
        (1 << 50, "1 PiB"),
    ] {
        assert_eq!(
            carried(&localizer, MsgArg::Bytes(bytes)),
            text,
            "{bytes} bytes"
        );
    }
}

#[test]
fn a_duration_reads_in_the_parts_the_catalog_arranges() {
    let localizer = carrier();
    for (milliseconds, text) in [
        (0, "0 ms"),
        (999, "999 ms"),
        (1_000, "1 s"),
        (61_000, "1 min 1 s"),
        (3_900_000, "1 h 5 min"),
        (90_000_000, "1 d 1 h"),
    ] {
        assert_eq!(
            carried(&localizer, MsgArg::DurationMs(milliseconds)),
            text,
            "{milliseconds} ms"
        );
    }
}

const EXPOSING_CAUSES: [&str; 4] = [
    "plain-http",
    "unchecked-certificate",
    "plain-endpoint",
    "plain-ftp",
];

/// The plain-HTTP location warning as English shows it, for one cause and
/// one `rule` (decision 80, point 7).
fn exposed(localizer: &Localizer, cause: &str, rule: &str) -> String {
    let shown = localizer.message(
        msg::WARNING_LOCATION_PLAIN_HTTP_CREDENTIALS,
        &[
            ("position", MsgArg::Text("2".to_owned())),
            ("cause", MsgArg::Text(cause.to_owned())),
            ("rule", MsgArg::Text(rule.to_owned())),
            ("locations", MsgArg::Text("several".to_owned())),
        ],
    );
    assert!(shown.problems().is_empty(), "{cause} {rule}: {shown}");
    shown.text().to_owned()
}

/// A repository of one location is listed without its locations, so what
/// it is told about them names no position (#473).
#[test]
fn a_repository_of_one_location_is_told_about_itself_and_no_location() {
    let localizer = english();
    let one = || ("locations", MsgArg::Text("one".to_owned()));
    let mut seen = BTreeSet::new();
    for cause in EXPOSING_CAUSES {
        for rule in ["network", "none"] {
            let shown = localizer.message(
                msg::WARNING_LOCATION_PLAIN_HTTP_CREDENTIALS,
                &[
                    ("position", MsgArg::Text("1".to_owned())),
                    ("cause", MsgArg::Text(cause.to_owned())),
                    ("rule", MsgArg::Text(rule.to_owned())),
                    one(),
                ],
            );
            assert!(shown.problems().is_empty(), "{cause} {rule}: {shown}");
            let text = shown.text().to_owned();
            assert!(
                text.starts_with("This repository"),
                "{cause} {rule}: {text}"
            );
            assert!(
                !text.to_lowercase().contains("location"),
                "{cause} {rule}: {text}"
            );
            assert!(seen.insert(text.clone()), "{cause} {rule}: {text}");
        }
    }
    for (id, args) in [
        (msg::WARNING_LOCATION_EVERY_RULED, vec![one()]),
        (
            msg::WARNING_LOCATION_LAST_RULED_NO_RETRY,
            vec![("plan", MsgArg::Text("home".to_owned())), one()],
        ),
        (
            msg::WARNING_LOCATION_RULE_NAME_UNREADABLE,
            vec![("position", MsgArg::Text("1".to_owned())), one()],
        ),
    ] {
        let shown = localizer.message(id, &args);
        assert!(shown.problems().is_empty(), "{}: {shown}", id.as_str());
        assert!(
            !shown.text().to_lowercase().contains("location"),
            "{}: {shown}",
            id.as_str()
        );
    }
}

/// A secret written as text is pointed out by its setting, said to be one,
/// never as if the setting were a place, and by the position of the
/// location that holds it when that is a further one (#505).
#[test]
fn a_written_secret_is_named_by_its_setting_and_its_further_location() {
    use api::WarningCode;
    let localizer = english();
    let text = |value: &str| MsgArg::Text(value.to_owned());
    let shown = |code, args: &[(&str, MsgArg)]| {
        let shown = localizer.warning(code, args, None);
        assert!(shown.problems().is_empty(), "{code:?}: {shown}");
        shown.text().to_owned()
    };
    for (code, args, start) in [
        (
            WarningCode::LocationSecret,
            vec![("field", text("repository")), ("kind", text("url"))],
            "The address in the repository setting holds a password as text",
        ),
        (
            WarningCode::LocationSecret,
            vec![
                ("field", text("repo-hot")),
                ("kind", text("rclone")),
                ("position", text("2")),
            ],
            "The address in the repo-hot setting of location 2 holds a credential as text",
        ),
        (
            WarningCode::LocationSecret,
            vec![
                ("field", text("repository")),
                ("kind", text("url")),
                ("position", text("3")),
            ],
            "The address in the repository setting of location 3 holds a password as text",
        ),
        (
            WarningCode::ConfigInlineSecret,
            vec![("field", text("options.password"))],
            "The options.password setting of this repository holds a secret written as text",
        ),
        (
            WarningCode::ConfigInlineSecret,
            vec![
                ("field", text("options-cold.secret_access_key")),
                ("position", text("2")),
            ],
            "The options-cold.secret_access_key setting of location 2 holds a secret written as text",
        ),
    ] {
        let text = shown(code, &args);
        assert!(text.starts_with(start), "{code:?}: {text}");
    }
}

#[test]
fn the_plain_http_warning_has_its_own_words_for_every_cause_and_rule() {
    let localizer = english();
    let mut seen = BTreeSet::new();
    for cause in EXPOSING_CAUSES {
        let ruled = exposed(&localizer, cause, "network");
        let open = exposed(&localizer, cause, "none");
        assert!(
            ruled.contains("its rule can't tell the network it is meant for"),
            "{cause} with a rule: {ruled}"
        );
        assert!(!open.contains("rule"), "{cause} with no rule: {open}");
        // A variant key that doesn't match falls back to the default text,
        // which would show up here as a text seen before.
        assert!(seen.insert(ruled.clone()), "{cause} with a rule: {ruled}");
        assert!(seen.insert(open.clone()), "{cause} with no rule: {open}");
    }
}

#[test]
fn the_plain_http_warning_gives_http_cacert_a_certificate_authority() {
    // `http-cacert` names the certificate of the authority that signed the
    // server's: rustls refuses a self-signed server certificate marked as a
    // CA (decision 80, point 7), so "the server's own certificate" sends a
    // person the wrong way.
    let localizer = english();
    for cause in EXPOSING_CAUSES {
        for rule in ["network", "none"] {
            let shown = exposed(&localizer, cause, rule);
            if shown.contains("http-cacert") {
                assert!(
                    shown.contains("certificate authority"),
                    "{cause} {rule}: {shown}"
                );
                assert!(
                    !shown.contains("own certificate"),
                    "{cause} {rule}: {shown}"
                );
            }
        }
    }
}
