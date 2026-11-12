//! The browser lane (technical design §8.4, "Tests"): the web UI writes
//! numbers, sizes, durations and instants with the browser's `Intl`, here
//! Node's, through the public [`Localizer`], as the page does. It runs under
//! `wasm-bindgen-test` (`just test-wasm`), with the `browser-intl` feature on,
//! and is empty anywhere else.
//!
//! The strings every host has to write are in `tests/expectations`, shared
//! with the ICU lane (`tests/lanes/icu.rs`). V8 writes U+0020 where CLDR writes
//! U+202F before AM and PM and U+00A0 elsewhere, so both sides of every
//! comparison are folded to U+0020 first. The catalogs here are the tests' own,
//! so the wording of the real catalog is not under test.
//!
//! Node may be built with another ICU than the one that wrote the table, so an
//! instant is held to what the running `Intl` writes for the case and to the
//! table's numbers, not to the table's text (technical design §8.4, "Tests").
#![cfg(all(target_arch = "wasm32", feature = "browser-intl"))]

#[path = "expectations/mod.rs"]
mod expectations;

use api::{IanaZone, MsgArg};
use js_sys::{Array, Function, Object, Reflect};
use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};
use serde_json::{Map, Value};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

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
at = at { $when }
good = { DATETIME($when, dateStyle: \"long\") }
bad = { DATETIME($when, dateStyle: \"full\") }
ignored = { NUMBER(1234.5, style: \"percent\", currency: \"EUR\", minimumSignificantDigits: 1, maximumSignificantDigits: 2) }
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
at = um { $when }
",
    }],
};

fn tag(raw: &str) -> unic_langid::LanguageIdentifier {
    raw.parse().unwrap()
}

fn speaking(locale: &str) -> Localizer {
    Localizer::from_catalogs(&[tag(locale)], &[EN, DE])
}

fn fold(text: &str) -> String {
    text.replace(['\u{202f}', '\u{a0}'], " ")
}

fn written(localizer: &Localizer, message: &'static str, args: &[(&str, MsgArg)]) -> String {
    let shown = localizer.message(MessageId::new(message), args);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    fold(shown.text())
}

fn count(localizer: &Localizer, message: &'static str, count: u64) -> String {
    written(localizer, message, &[("count", MsgArg::Count(count))])
}

fn instant(localizer: &Localizer, utc_ms: i64, zone: Option<&str>) -> String {
    let when = MsgArg::Timestamp {
        utc_ms,
        zone: zone.map(|name| IanaZone::new(name).unwrap()),
    };
    written(localizer, "at", &[("when", when)])
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
    let wrong: Vec<String> = outcomes.into_iter().filter_map(differs).collect();
    assert!(
        wrong.is_empty(),
        "{} cases of {locale} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// How a case went wrong, or `None` when it holds. A number, count, size or
/// duration is the table's text. An instant is the running `Intl`'s text,
/// with the table's numbers in it.
fn differs(outcome: expectations::Outcome) -> Option<String> {
    let case: Map<String, Value> = serde_json::from_str(&outcome.case).expect("a case is JSON");
    let table = fold(&outcome.expected);
    let outcome = expectations::Outcome {
        expected: match case.get("instant") {
            Some(_) => fold(&intl_writes(&case)),
            None => table.clone(),
        },
        shown: fold(&outcome.shown),
        ..outcome
    };
    let (shown, pinned) = (numbers(&outcome.shown), numbers(&table));
    (!outcome.holds() || shown != pinned).then(|| {
        format!(
            "{}\n    table    {table:?}\n    numbers  {shown:?}, the table's {pinned:?}",
            outcome.describe()
        )
    })
}

/// What the running `Intl` writes for an instant case, as `generate.ts`
/// wrote the table: the case's options, or a medium date and a short time
/// when it has none, in the case's zone, or in UTC when there is none or
/// `Intl` doesn't have it.
fn intl_writes(case: &Map<String, Value>) -> String {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let option = |name: &str| case.get("options").and_then(|options| text(&options[name]));
    let options = Object::new();
    let set = |name: &str, value: &str| {
        Reflect::set(&options, &name.into(), &value.into()).unwrap();
    };
    let zone_name = option("timeZoneName").is_some();
    if zone_name {
        set("timeZoneName", "short");
    } else if option("dateStyle").is_none() && option("timeStyle").is_none() {
        set("dateStyle", "medium");
        set("timeStyle", "short");
    } else {
        for name in ["dateStyle", "timeStyle"] {
            if let Some(style) = option(name) {
                set(name, &style);
            }
        }
    }
    let instant = &case["instant"];
    let formatter = text(&instant["zone"])
        .and_then(|zone| date_time_format(&case["locale"], &options, &zone))
        .or_else(|| date_time_format(&case["locale"], &options, "UTC"))
        .unwrap_or_else(|| panic!("Intl refuses the options of {case:?}"));
    let utc_ms = JsValue::from_f64(instant["utcMs"].as_f64().unwrap());
    let method = |name: &str| -> JsValue {
        Reflect::get(&formatter, &name.into())
            .unwrap()
            .unchecked_into::<Function>()
            .call1(&formatter, &utc_ms)
            .unwrap_or_else(|error| panic!("Intl can't write {case:?}: {error:?}"))
    };
    if !zone_name {
        return method("format").as_string().unwrap();
    }
    method("formatToParts")
        .unchecked_into::<Array>()
        .iter()
        .find(|part| {
            Reflect::get(part, &"type".into())
                .unwrap()
                .as_string()
                .as_deref()
                == Some("timeZoneName")
        })
        .and_then(|part| Reflect::get(&part, &"value".into()).unwrap().as_string())
        .unwrap_or_else(|| panic!("Intl writes no zone name for {case:?}"))
}

/// `new Intl.DateTimeFormat(locale, { ...options, timeZone: zone })`, or
/// `None` when `Intl` refuses the zone.
fn date_time_format(locale: &Value, options: &Object, zone: &str) -> Option<JsValue> {
    let options = Object::assign(&Object::new(), options);
    Reflect::set(&options, &"timeZone".into(), &zone.into()).unwrap();
    let intl = Reflect::get(&js_sys::global(), &"Intl".into()).unwrap();
    let constructor: Function = Reflect::get(&intl, &"DateTimeFormat".into())
        .unwrap()
        .unchecked_into();
    let locale = JsValue::from_str(locale.as_str().unwrap());
    Reflect::construct(&constructor, &Array::of2(&locale, &options)).ok()
}

/// The zero of each script of digits the table's locales write: Latin,
/// Arabic-Indic, Persian, Devanagari, Bengali and Thai.
const ZEROS: [char; 6] = ['0', '\u{660}', '\u{6f0}', '\u{966}', '\u{9e6}', '\u{e50}'];

/// The numbers a text writes, in order, as Latin digits without the zeros
/// that lead them: `08:30` and `٠٨:٣٠` are both `["8", "30"]`.
fn numbers(text: &str) -> Vec<String> {
    let mut numbers = Vec::new();
    let mut run = String::new();
    for c in text.chars() {
        let digit = ZEROS.iter().find_map(|&zero| {
            let value = u32::from(c).checked_sub(u32::from(zero))?;
            char::from_digit(value, 10)
        });
        match digit {
            Some('0') if run.is_empty() => run.push('0'),
            Some(digit) if run == "0" => run = digit.to_string(),
            Some(digit) => run.push(digit),
            None => {
                assert!(
                    !c.is_numeric(),
                    "{c:?} in {text:?} is a digit no script here has"
                );
                if !run.is_empty() {
                    numbers.push(std::mem::take(&mut run));
                }
            }
        }
    }
    if !run.is_empty() {
        numbers.push(run);
    }
    numbers
}

#[wasm_bindgen_test]
fn the_table_holds_for_american_english() {
    the_table_holds("en-US");
}

#[wasm_bindgen_test]
fn the_table_holds_for_german() {
    the_table_holds("de");
}

#[wasm_bindgen_test]
fn the_table_holds_for_japanese() {
    the_table_holds("ja");
}

#[wasm_bindgen_test]
fn the_table_holds_for_arabic() {
    the_table_holds("ar");
}

#[wasm_bindgen_test]
fn the_table_holds_for_bengali() {
    the_table_holds("bn");
}

#[wasm_bindgen_test]
fn the_table_holds_for_hindi() {
    the_table_holds("hi");
}

#[wasm_bindgen_test]
fn the_table_holds_for_persian() {
    the_table_holds("fa");
}

#[wasm_bindgen_test]
fn the_table_holds_for_thai() {
    the_table_holds("th");
}

#[wasm_bindgen_test]
fn a_count_is_written_by_its_locale_and_still_picks_its_plural_form() {
    for (locale, one, many) in [
        ("en-US", "1 file", "1,234,567 files"),
        ("de", "1 Datei", "1.234.567 Dateien"),
    ] {
        let localizer = speaking(locale);
        assert_eq!(count(&localizer, "files", 1), one, "{locale}");
        assert_eq!(count(&localizer, "files", 1_234_567), many, "{locale}");
    }
}

#[wasm_bindgen_test]
fn a_count_still_matches_a_number_in_a_selector() {
    // The selector takes the number, not the text the host wrote for it.
    let localizer = speaking("en-US");
    assert_eq!(count(&localizer, "exactly", 1), "one thing");
    assert_eq!(count(&localizer, "exactly", 1_200), "1,200 things");
}

#[wasm_bindgen_test]
fn a_bare_instant_is_a_medium_date_with_a_short_time_in_its_zone() {
    assert_eq!(
        instant(&speaking("en-US"), WINTER, Some("Europe/Berlin")),
        "at Nov 8, 2026, 2:30 PM"
    );
    assert_eq!(
        instant(&speaking("de"), WINTER, Some("Europe/Berlin")),
        "um 08.11.2026, 14:30"
    );
}

#[wasm_bindgen_test]
fn an_instant_with_no_zone_or_a_zone_nobody_has_is_read_as_utc() {
    let at = |zone| instant(&speaking("en-US"), WINTER, zone);
    assert_eq!(at(None), "at Nov 8, 2026, 1:30 PM");
    assert_eq!(at(Some("Europe/Atlantis")), at(None));
    assert_eq!(at(Some("UTC")), at(None));
}

#[wasm_bindgen_test]
fn an_instant_no_calendar_can_show_is_written_as_its_count() {
    let localizer = speaking("en-US");
    for utc_ms in [i64::MAX, i64::MIN] {
        assert_eq!(
            instant(&localizer, utc_ms, Some("Europe/Berlin")),
            format!("at {utc_ms}")
        );
    }
    // The last instant `jiff` takes is written, and the next millisecond is not.
    let last = jiff::Timestamp::MAX.as_second() * 1000;
    assert_eq!(instant(&localizer, last, None), "at Dec 30, 9999, 10:00 PM");
    assert_eq!(
        instant(&localizer, last + 1, None),
        format!("at {}", last + 1)
    );
}

#[wasm_bindgen_test]
fn the_datetime_vocabulary_is_written_and_a_bad_call_is_shown_as_the_function() {
    let localizer = speaking("en-US");
    let when = || {
        [(
            "when",
            MsgArg::Timestamp {
                utc_ms: WINTER,
                zone: IanaZone::new("Europe/Berlin").ok(),
            },
        )]
    };
    assert_eq!(written(&localizer, "good", &when()), "November 8, 2026");
    assert_eq!(written(&localizer, "bad", &when()), "DATETIME()");
}

#[wasm_bindgen_test]
fn options_the_host_does_not_honour_change_nothing() {
    let localizer = speaking("en-US");
    assert_eq!(written(&localizer, "ignored", &[]), "1,234.5");
    assert_eq!(written(&localizer, "grouping-false", &[]), "1234567");
}

#[wasm_bindgen_test]
fn absurd_digit_options_are_capped_before_the_browser_sees_them() {
    let localizer = speaking("en-US");
    // The host writes numbers: that comes first, so the cap is what is tested next.
    assert_eq!(count(&localizer, "files", 1_000), "1,000 files");
    let shown = written(&localizer, "huge", &[]);
    let (integer, fraction) = shown.split_once('.').unwrap();
    assert_eq!(integer, "000,000,000,000,000,000,001");
    assert_eq!(fraction.len(), 100, "{fraction}");
    assert!(fraction.starts_with('5'), "{fraction}");
}

#[wasm_bindgen_test]
fn isolation_wraps_a_written_number_and_a_written_instant_once() {
    let localizer = speaking("de").with_isolation(true);
    let raw = |message, args: &[(&str, MsgArg)]| {
        localizer.message(MessageId::new(message), args).into_text()
    };
    assert_eq!(
        fold(&raw("files", &[("count", MsgArg::Count(1_234))])),
        "\u{2068}1.234\u{2069} Dateien"
    );
    let when = MsgArg::Timestamp {
        utc_ms: WINTER,
        zone: IanaZone::new("Europe/Berlin").ok(),
    };
    assert_eq!(
        fold(&raw("at", &[("when", when)])),
        "um \u{2068}08.11.2026, 14:30\u{2069}"
    );
}

#[wasm_bindgen_test]
fn the_viewers_zone_is_the_zone_the_browser_reads_its_own_clock_in() {
    // `just test-wasm` pins `TZ` to a zone that is not UTC, so a viewer's zone
    // and the formatter's default are two different readings of one instant.
    assert_eq!(
        l10n::viewer_zone(),
        Some(IanaZone::new("America/New_York").unwrap())
    );
    let localizer = speaking("en-US");
    let viewer = l10n::viewer_zone();
    assert_ne!(
        instant(&localizer, WINTER, viewer.as_ref().map(IanaZone::as_str)),
        instant(&localizer, WINTER, None),
        "the viewer's zone and the default are told apart"
    );
}

/// A locale the browser has no data for leaves its instants to the plain
/// text, which says UTC, and the localizer tells a page so.
#[wasm_bindgen_test]
fn the_browser_writes_the_instants_only_of_the_locales_it_has() {
    assert!(speaking("en-US").writes_instants());
    assert!(speaking("de").writes_instants());
    const KLINGON: Catalog = Catalog {
        tag: "tlh",
        sources: &[Source {
            name: "tlh.ftl",
            text: "at = { $when }\n",
        }],
    };
    let unknown = Localizer::from_catalogs(&[tag("tlh")], &[KLINGON, EN]);
    assert_eq!(unknown.locales().first(), Some(&tag("tlh")));
    assert!(!unknown.writes_instants());
}
