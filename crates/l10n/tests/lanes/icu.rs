//! The ICU lane (technical design §8.4, "Tests"): the daemon writes numbers,
//! sizes, durations and instants as the locale does. The lanes run with the
//! `icu` feature on, the daemon's own configuration. The catalogs here are the
//! tests' own, so the wording of the real catalog is not under test.
//!
//! The strings every host has to write are in `tests/expectations`, shared with
//! the lane of the browser's `Intl`; the tests below are what only the daemon
//! has: the messages of a locale chain, instants that no calendar can show, the
//! caps on what a catalog can ask for, and sharing a localizer between threads.

#[path = "../expectations/mod.rs"]
mod expectations;

use std::thread;

use api::{IanaZone, MsgArg};
use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};

use crate::support::tag;

const EN: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "en.ftl",
        text: "\
files = { NUMBER($count) ->
    [one] { $count } file
   *[other] { $count } files
  }
exactly = { NUMBER($count) ->
    [1] one thing
   *[other] { $count } things
  }
english-only = { $count } things
at = at { $when }
good = { DATETIME($when, dateStyle: \"long\") }
bad = { DATETIME($when, dateStyle: \"full\") }
ignored = { NUMBER(1234.5, style: \"percent\", currency: \"EUR\", minimumSignificantDigits: 1, maximumSignificantDigits: 2) }
grouping-true = { NUMBER(1234567, useGrouping: \"true\") }
grouping-no = { NUMBER(1234567, useGrouping: \"no\") }
grouping-false = { NUMBER(1234567, useGrouping: \"false\") }
huge = { NUMBER(1.5, minimumFractionDigits: 4503599627370496, minimumIntegerDigits: 4503599627370496, maximumFractionDigits: 4503599627370496) }
",
    }],
};

const DE: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "de.ftl",
        text: "\
files = { NUMBER($count) ->
    [one] { $count } Datei
   *[other] { $count } Dateien
  }
exactly = { NUMBER($count) ->
    [1] eine Sache
   *[other] { $count } Sachen
  }
at = um { $when }
",
    }],
};

const JA: Catalog = Catalog {
    tag: "ja",
    sources: &[Source {
        name: "ja.ftl",
        text: "\
files = { $count }個
exactly = { NUMBER($count) ->
    [1] 1つ
   *[other] { $count }つ
  }
at = { $when }に
",
    }],
};

fn speaking(locale: &str) -> Localizer {
    Localizer::from_catalogs(&[tag(locale)], &[EN, DE, JA])
}

fn id(id: &'static str) -> MessageId {
    MessageId::new(id)
}

fn count(localizer: &Localizer, message: &'static str, count: u64) -> String {
    let shown = localizer.message(id(message), &[("count", MsgArg::Count(count))]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    shown.into_text()
}

fn instant(
    localizer: &Localizer,
    message: &'static str,
    utc_ms: i64,
    zone: Option<&str>,
) -> String {
    let when = MsgArg::Timestamp {
        utc_ms,
        zone: zone.map(|name| IanaZone::new(name).unwrap()),
    };
    let shown = localizer.message(id(message), &[("when", when)]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    shown.into_text()
}

/// 2026-11-08 13:30:45 UTC.
const WINTER: i64 = 1_794_144_645_000;

fn the_table_holds(locale: &str) {
    let outcomes = expectations::run(locale);
    assert!(
        outcomes.len() >= 70,
        "{locale} has {} cases",
        outcomes.len()
    );
    let wrong: Vec<String> = outcomes
        .iter()
        .filter(|outcome| !outcome.holds())
        .map(expectations::Outcome::describe)
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} cases of {locale} differ:\n{}",
        wrong.len(),
        outcomes.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_table_holds_for_american_english() {
    the_table_holds("en-US");
}

#[test]
fn the_table_holds_for_german() {
    the_table_holds("de");
}

#[test]
fn the_table_holds_for_japanese() {
    the_table_holds("ja");
}

#[test]
fn a_count_is_written_by_its_locale_and_still_picks_its_plural_form() {
    for (locale, one, many) in [
        ("en-US", "1 file", "1,234,567 files"),
        ("de", "1 Datei", "1.234.567 Dateien"),
        ("ja", "1個", "1,234,567個"),
    ] {
        let localizer = speaking(locale);
        assert_eq!(count(&localizer, "files", 1), one, "{locale}");
        assert_eq!(count(&localizer, "files", 1_234_567), many, "{locale}");
    }
}

#[test]
fn a_count_still_matches_a_number_in_a_selector() {
    // The selector takes the number, not the text the host wrote for it.
    for (locale, one, many) in [
        ("en-US", "one thing", "1,200 things"),
        ("de", "eine Sache", "1.200 Sachen"),
        ("ja", "1つ", "1,200つ"),
    ] {
        let localizer = speaking(locale);
        assert_eq!(count(&localizer, "exactly", 1), one, "{locale}");
        assert_eq!(count(&localizer, "exactly", 1_200), many, "{locale}");
    }
}

#[test]
fn a_message_is_written_with_the_numbers_of_the_locale_that_has_it() {
    let localizer = speaking("de");
    // `files` is in the German catalog, `english-only` is not.
    assert_eq!(count(&localizer, "files", 1_234), "1.234 Dateien");
    assert_eq!(count(&localizer, "english-only", 1_234), "1,234 things");
}

#[test]
fn a_bare_instant_is_a_medium_date_with_a_short_time_in_its_zone() {
    let at = |locale, zone| instant(&speaking(locale), "at", WINTER, zone);
    assert_eq!(
        at("en-US", Some("Europe/Berlin")),
        "at Nov 8, 2026, 2:30\u{202f}PM"
    );
    assert_eq!(at("de", Some("Europe/Berlin")), "um 08.11.2026, 14:30");
    assert_eq!(at("ja", Some("Asia/Tokyo")), "2026/11/08 22:30に");
}

#[test]
fn an_instant_with_no_zone_or_a_zone_nobody_has_is_read_as_utc() {
    let at = |zone| instant(&speaking("en-US"), "at", WINTER, zone);
    assert_eq!(at(None), "at Nov 8, 2026, 1:30\u{202f}PM");
    assert_eq!(at(Some("Europe/Atlantis")), at(None));
    assert_eq!(at(Some("UTC")), at(None));
}

#[test]
fn an_instant_no_calendar_can_show_is_written_as_its_count_in_every_locale() {
    for (locale, before, after, written) in [
        ("en-US", "at ", "", "at Nov 8, 2026, 2:30\u{202f}PM"),
        ("de", "um ", "", "um 08.11.2026, 14:30"),
        ("ja", "", "に", "2026/11/08 14:30に"),
    ] {
        let localizer = speaking(locale);
        let berlin = Some("Europe/Berlin");
        // An instant it can show is written by the locale; the two it cannot are their counts.
        assert_eq!(
            instant(&localizer, "at", WINTER, berlin),
            written,
            "{locale}"
        );
        for utc_ms in [i64::MAX, i64::MIN] {
            assert_eq!(
                instant(&localizer, "at", utc_ms, berlin),
                format!("{before}{utc_ms}{after}"),
                "{locale}"
            );
        }
    }
}

#[test]
fn the_last_instant_a_calendar_can_show_is_written_and_the_next_is_not() {
    let localizer = speaking("en-US");
    // `jiff` takes milliseconds up to the last whole second of its range, so
    // the 999 milliseconds after it, which `Timestamp::MAX` has, are not an
    // instant to write.
    let last = jiff::Timestamp::MAX.as_second() * 1000;
    assert_eq!(
        instant(&localizer, "at", last, None),
        "at Dec 30, 9999, 10:00\u{202f}PM"
    );
    assert_eq!(
        instant(&localizer, "at", last + 1, None),
        format!("at {}", last + 1)
    );
    let first = jiff::Timestamp::MIN.as_millisecond();
    let shown = instant(&localizer, "at", first, None);
    assert!(shown.starts_with("at Jan 2, "), "{shown}");
    assert_eq!(
        instant(&localizer, "at", first - 1, None),
        format!("at {}", first - 1)
    );
}

#[test]
fn the_datetime_vocabulary_is_written_and_a_bad_call_is_shown_as_the_function() {
    let localizer = speaking("en-US");
    // The good call first, so that it is the host that is being asked.
    assert_eq!(
        instant(&localizer, "good", WINTER, Some("Europe/Berlin")),
        "November 8, 2026"
    );
    // `full` is not in the vocabulary; nothing is written for it but the call.
    assert_eq!(
        instant(&localizer, "bad", WINTER, Some("Europe/Berlin")),
        "DATETIME()"
    );
}

#[test]
fn options_a_host_does_not_honour_change_nothing() {
    let localizer = speaking("en-US");
    let shown = localizer.message(id("ignored"), &[]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    assert_eq!(shown.text(), "1,234.5");
}

#[test]
fn only_the_word_false_turns_grouping_off() {
    let localizer = speaking("en-US");
    let grouping = |message: &'static str| {
        let shown = localizer.message(id(message), &[]);
        assert!(shown.problems().is_empty(), "{:?}", shown.problems());
        shown.into_text()
    };
    assert_eq!(grouping("grouping-true"), "1,234,567");
    assert_eq!(grouping("grouping-no"), "1,234,567");
    assert_eq!(grouping("grouping-false"), "1234567");
}

#[test]
fn absurd_digit_options_are_capped_before_anything_is_written() {
    let localizer = speaking("en-US");
    // The host writes numbers: that comes first, so the cap is what is tested next.
    assert_eq!(count(&localizer, "files", 1_000), "1,000 files");

    // A catalog's own literal asks for 2^52 digits of each kind.
    let shown = localizer.message(id("huge"), &[]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    let (integer, fraction) = shown.text().split_once('.').unwrap();
    // 21 integer digits, the most there are, and 100 fraction digits.
    assert_eq!(integer, "000,000,000,000,000,000,001");
    assert_eq!(fraction.len(), 100, "{fraction}");
    assert!(fraction.starts_with('5'), "{fraction}");
    assert!(
        fraction[1..].bytes().all(|digit| digit == b'0'),
        "{fraction}"
    );
}

#[test]
fn isolation_wraps_a_written_number_and_a_written_instant_once() {
    let localizer = Localizer::from_catalogs(&[tag("de")], &[EN, DE, JA]).with_isolation(true);
    assert_eq!(
        count(&localizer, "files", 1_234),
        "\u{2068}1.234\u{2069} Dateien"
    );
    assert_eq!(
        instant(&localizer, "at", WINTER, Some("Europe/Berlin")),
        "um \u{2068}08.11.2026, 14:30\u{2069}"
    );
}

#[test]
fn a_localizer_with_formatters_is_shared_between_threads() {
    let localizer = speaking("de");
    let berlin = Some("Europe/Berlin");
    let numbers = |round: u64| count(&localizer, "files", 1_000 * round);
    let instants = |round: i64| instant(&localizer, "at", WINTER + 60_000 * round, berlin);
    // What one thread writes is what a locale writes.
    assert_eq!(numbers(1_234), "1.234.000 Dateien");
    assert_eq!(instants(1), "um 08.11.2026, 14:31");

    let expected_numbers: Vec<String> = (0..50).map(numbers).collect();
    let expected_instants: Vec<String> = (0..50).map(instants).collect();
    thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for round in 0..50 {
                        assert_eq!(numbers(round), expected_numbers[round as usize]);
                        assert_eq!(instants(round as i64), expected_instants[round as usize]);
                    }
                })
            })
            .collect();
        for worker in workers {
            assert!(worker.join().is_ok());
        }
    });
}
