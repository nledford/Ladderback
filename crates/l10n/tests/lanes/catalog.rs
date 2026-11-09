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
