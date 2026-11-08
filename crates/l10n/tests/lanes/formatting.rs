//! The formatting lane (technical design §8.4, "Tests"): how a `MsgArg`
//! becomes text, how a chain of locales is walked, and what a missing message,
//! argument or locale does. The catalogs are the tests' own, so the wording
//! of the real catalog is not under test here.

use std::thread;

use api::MsgArg;
use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};

use crate::support::{borrowed, tag};

const EN: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "en.ftl",
        text: "\
files = { NUMBER($count) ->
    [one] { $count } file
   *[other] { $count } files
  }
exact = { NUMBER($count) ->
    [9007199254740992] the largest exact count
   *[other] { $count }
  }
free = { $space } free
took = took { $time }
at = at { $when }
greeting = Hello { $name }
shared = The English text
english-only = Only in English
size = { $value }|{ $unit }
duration = { $form ->
    [ms] ms={ $milliseconds }
    [s] s={ $seconds }
    [min-s] min={ $minutes } s={ $seconds }
    [h-min] h={ $hours } min={ $minutes }
   *[d-h] d={ $days } h={ $hours }
  }
",
    }],
};

const DE: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "de.ftl",
        text: "\
shared = Der deutsche Text
free = { $space } frei
lasted = dauerte { $time }
size = { $value } { $unit }-de
",
    }],
};

fn english() -> Localizer {
    Localizer::from_catalogs(&[], &[EN])
}

fn id(id: &'static str) -> MessageId {
    MessageId::new(id)
}

fn text(value: &str) -> MsgArg {
    MsgArg::Text(value.to_owned())
}

#[test]
fn a_count_picks_its_plural_form() {
    let localizer = english();
    let files = |count| localizer.message(id("files"), &[("count", MsgArg::Count(count))]);
    assert_eq!(files(1).text(), "1 file");
    assert_eq!(files(0).text(), "0 files");
    assert_eq!(files(2).text(), "2 files");
    assert_eq!(files(1_000_000).text(), "1000000 files");
}

#[test]
fn a_count_is_shown_digit_for_digit_even_past_what_a_float_holds() {
    let localizer = english();
    for count in [(1 << 53) - 1, 1 << 53, (1 << 53) + 1, u64::MAX] {
        let shown = localizer.message(id("files"), &[("count", MsgArg::Count(count))]);
        assert_eq!(shown.text(), format!("{count} files"));
    }
}

#[test]
fn a_count_stays_a_number_up_to_the_largest_a_float_holds_exactly() {
    let localizer = english();
    let exact = |count| {
        localizer
            .message(id("exact"), &[("count", MsgArg::Count(count))])
            .into_text()
    };
    assert_eq!(exact(1 << 53), "the largest exact count");
    assert_eq!(exact((1 << 53) - 1), "9007199254740991");
    assert_eq!(exact((1 << 53) + 1), "9007199254740993");
}

#[test]
fn text_that_looks_like_a_count_selects_no_plural_form() {
    let shown = english().message(id("files"), &[("count", text("1"))]);
    assert_eq!(shown.text(), "1 files");
}

#[test]
fn a_size_reaches_its_message_in_the_largest_unit_that_keeps_it_at_one_or_more() {
    let localizer = english();
    for (bytes, shown) in [
        (0, "0|b free"),
        (1023, "1023|b free"),
        (1536, "1.5|kib free"),
        (1_048_575, "1|mib free"),
        (5_905_580_032, "5.5|gib free"),
        (3 << 40, "3|tib free"),
        (u64::MAX, "16384|pib free"),
    ] {
        let free = localizer.message(id("free"), &[("space", MsgArg::Bytes(bytes))]);
        assert!(free.problems().is_empty(), "{:?}", free.problems());
        assert_eq!(free.text(), shown, "{bytes} bytes");
    }
}

#[test]
fn a_duration_reaches_its_message_as_its_two_largest_parts() {
    let localizer = english();
    for (milliseconds, shown) in [
        (500, "took ms=500"),
        (1_500, "took s=1"),
        (61_000, "took min=1 s=1"),
        (3_900_000, "took h=1 min=5"),
        (90_000_000, "took d=1 h=1"),
        (u64::MAX, "took d=213503982334 h=14"),
    ] {
        let took = localizer.message(id("took"), &[("time", MsgArg::DurationMs(milliseconds))]);
        assert!(took.problems().is_empty(), "{:?}", took.problems());
        assert_eq!(took.text(), shown, "{milliseconds} ms");
    }
}

#[test]
fn an_instant_is_written_in_utc() {
    let when = MsgArg::Timestamp {
        utc_ms: 1_794_144_645_000,
        zone: None,
    };
    let shown = english().message(id("at"), &[("when", when)]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    assert_eq!(shown.text(), "at 2026-11-08 13:30:45 UTC");
}

#[test]
fn text_is_shown_as_given_and_cut_to_the_cap() {
    let localizer = english();
    let path = localizer.message(id("greeting"), &[("name", text("/home/demo/docs"))]);
    assert_eq!(path.text(), "Hello /home/demo/docs");

    let long = "x".repeat(100_000);
    let cut = localizer.message(id("greeting"), &[("name", text(&long))]);
    assert!(
        cut.text().len() <= "Hello ".len() + 16 * 1024,
        "{}",
        cut.text().len()
    );
    assert!(cut.text().ends_with('…'));
}

#[test]
fn arguments_past_the_cap_are_not_read() {
    let localizer = english();
    let filler = |count: usize| (0..count).map(|n| (format!("filler-{n}"), MsgArg::Count(1)));
    let late: Vec<(String, MsgArg)> = filler(32)
        .chain([("name".to_owned(), text("demo"))])
        .collect();
    let shown = localizer.message(id("greeting"), &borrowed(&late));
    assert!(!shown.problems().is_empty(), "the 33rd argument was read");

    let early: Vec<(String, MsgArg)> = std::iter::once(("name".to_owned(), text("demo")))
        .chain(filler(1000))
        .collect();
    let shown = localizer.message(id("greeting"), &borrowed(&early));
    assert_eq!(shown.text(), "Hello demo");
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
}

#[test]
fn the_last_argument_within_the_cap_is_read() {
    let filler = |count: usize| (0..count).map(|n| (format!("filler-{n}"), MsgArg::Count(1)));
    let last: Vec<(String, MsgArg)> = filler(31)
        .chain([("name".to_owned(), text("demo"))])
        .collect();
    let shown = english().message(id("greeting"), &borrowed(&last));
    assert_eq!(shown.text(), "Hello demo");
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
}

#[test]
fn an_argument_the_message_never_reads_is_ignored() {
    let shown = english().message(
        id("greeting"),
        &[("name", text("demo")), ("extra", MsgArg::Count(7))],
    );
    assert_eq!(shown.text(), "Hello demo");
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
}

#[test]
fn a_variable_nobody_supplied_is_reported_and_shown_by_name() {
    let shown = english().message(id("greeting"), &[]);
    assert!(!shown.problems().is_empty());
    assert!(shown.text().contains("name"), "{shown}");
}

#[test]
fn a_formatted_message_displays_as_its_text() {
    let shown = english().message(id("greeting"), &[("name", text("demo"))]);
    assert_eq!(shown.to_string(), "Hello demo");
}

#[test]
fn a_localizer_shows_the_locales_it_speaks_when_debugged() {
    let localizer = english();
    let debugged = format!("{localizer:?}");
    assert!(
        debugged.contains(&format!("{:?}", localizer.locales())),
        "{debugged}"
    );
}

#[test]
fn a_message_nobody_has_is_shown_by_its_id_and_reported() {
    let shown = english().message(id("no-such-message"), &[]);
    assert_eq!(shown.text(), "no-such-message");
    assert!(!shown.problems().is_empty());
}

#[test]
fn isolation_marks_wrap_each_value_only_when_asked() {
    let args = [("name", text("demo"))];
    let off = english();
    assert_eq!(off.message(id("greeting"), &args).text(), "Hello demo");
    let on = Localizer::from_catalogs(&[], &[EN]).with_isolation(true);
    assert_eq!(
        on.message(id("greeting"), &args).text(),
        "Hello \u{2068}demo\u{2069}"
    );
}

#[test]
fn isolation_wraps_a_size_once_not_around_each_of_its_parts() {
    let on = Localizer::from_catalogs(&[], &[EN]).with_isolation(true);
    let shown = on.message(id("free"), &[("space", MsgArg::Bytes(1536))]);
    assert_eq!(shown.text(), "\u{2068}1.5|kib\u{2069} free");
}

#[test]
fn the_chain_lists_the_preferred_locales_first_and_ends_with_en_us() {
    let localizer = Localizer::from_catalogs(&[tag("de-DE")], &[EN, DE]);
    assert_eq!(localizer.locales(), [tag("de"), tag("en-US")]);
    // A locale nobody has a catalog for is left out.
    let localizer = Localizer::from_catalogs(&[tag("ja")], &[EN, DE]);
    assert_eq!(localizer.locales(), [tag("en-US")]);
}

#[test]
fn the_embedded_catalogs_answer_for_any_preference() {
    let localizer = Localizer::new(&[tag("de-DE"), tag("ja")]);
    assert_eq!(localizer.locales(), [tag("en-US")]);
    let shown = localizer.message(
        id("cli-listening"),
        &[("url", text("http://127.0.0.1:9898"))],
    );
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
}

#[test]
fn a_message_the_preferred_locale_lacks_falls_back_to_the_next() {
    let localizer = Localizer::from_catalogs(&[tag("de")], &[EN, DE]);
    assert_eq!(
        localizer.message(id("shared"), &[]).text(),
        "Der deutsche Text"
    );
    let fallen = localizer.message(id("english-only"), &[]);
    assert_eq!(fallen.text(), "Only in English");
    assert!(fallen.problems().is_empty(), "{:?}", fallen.problems());
}

#[test]
fn a_size_or_duration_is_written_by_the_locale_that_writes_its_message() {
    let localizer = Localizer::from_catalogs(&[tag("de")], &[EN, DE]);
    // The message and the size are both German.
    let free = localizer.message(id("free"), &[("space", MsgArg::Bytes(1536))]);
    assert_eq!(free.text(), "1.5 kib-de frei");
    // The German message has no duration of its own, so the next locale's
    // is used for the parts.
    let lasted = localizer.message(id("lasted"), &[("time", MsgArg::DurationMs(3_900_000))]);
    assert_eq!(lasted.text(), "dauerte h=1 min=5");
    assert!(lasted.problems().is_empty(), "{:?}", lasted.problems());
}

#[test]
fn a_localizer_is_shared_between_threads() {
    let localizer = Localizer::from_catalogs(&[tag("de")], &[EN, DE]);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|worker| {
                let localizer = &localizer;
                scope.spawn(move || {
                    for round in 0..50u64 {
                        let shown = localizer.message(
                            id("free"),
                            &[("space", MsgArg::Bytes(1024 * (worker + round)))],
                        );
                        assert!(shown.problems().is_empty());
                        assert!(shown.text().ends_with(" frei"), "{shown}");
                    }
                })
            })
            .collect();
        for worker in workers {
            assert!(worker.join().is_ok());
        }
    });
}
