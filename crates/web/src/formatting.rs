//! The web UI's lane for numbers and instants (technical design §8.4), run by
//! `just test-wasm` under Node: a localizer built in this crate's own
//! feature set, which is the one the page runs with, writes `NUMBER` and
//! `DATETIME` as the browser's `Intl` does, not as the plain fallbacks do. The
//! table of every case a host writes is `l10n`'s own lane; this is the check
//! that `web` is built with that host.
//!
//! The German catalog is the test's own, for the host's writing of `DATETIME`;
//! the page's own instants come out of the real `ui-instant` message, which
//! the last test reads through `routes::instant`, with the viewer's zone as
//! the pages pass it.

use api::{IanaZone, MsgArg};
use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};
use wasm_bindgen_test::wasm_bindgen_test;

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
long = { DATETIME($when, dateStyle: \"long\", timeStyle: \"short\") }
",
    }],
};

fn german() -> Localizer {
    Localizer::from_catalogs(&["de".parse().unwrap()], &[DE])
}

fn written(localizer: &Localizer, message: &'static str, args: &[(&str, MsgArg)]) -> String {
    let shown = localizer.message(MessageId::new(message), args);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    // V8 writes U+0020 where CLDR writes U+202F or U+00A0.
    shown.text().replace(['\u{202f}', '\u{a0}'], " ")
}

fn when() -> [(&'static str, MsgArg); 1] {
    [(
        "when",
        MsgArg::Timestamp {
            // 2026-11-08 13:30:45 UTC.
            utc_ms: 1_794_144_645_000,
            zone: IanaZone::new("Europe/Berlin").ok(),
        },
    )]
}

#[wasm_bindgen_test]
fn a_count_in_the_web_bundle_is_grouped_as_the_locale_groups_it() {
    let shown = written(&german(), "files", &[("count", MsgArg::Count(1_234_567))]);
    assert_eq!(shown, "1.234.567 Dateien");
}

#[wasm_bindgen_test]
fn an_instant_in_the_web_bundle_is_written_in_its_zone_as_the_locale_writes_it() {
    assert_eq!(written(&german(), "at", &when()), "um 08.11.2026, 14:30");
    assert_eq!(
        written(&german(), "long", &when()),
        "8. November 2026 um 14:30"
    );
}

#[wasm_bindgen_test]
fn a_page_writes_an_instant_in_the_viewers_zone_and_the_formatter_without_one_in_utc() {
    let localizer = Localizer::new(&["en-US".parse().unwrap()]);
    let raw = "2026-11-08T13:30:45Z";
    let viewer = l10n::viewer_zone();
    let fold = |text: String| text.replace(['\u{202f}', '\u{a0}'], " ");
    // `just test-wasm` pins `TZ=America/New_York`: 13:30 UTC is 8:30 there.
    assert_eq!(
        fold(crate::routes::instant(&localizer, raw, viewer.as_ref())),
        "11/8/26, 8:30:45 AM"
    );
    assert_eq!(
        fold(crate::routes::instant(&localizer, raw, None)),
        "11/8/26, 1:30:45 PM"
    );
}

/// A listed snapshot's time is what its own clock read (§16, decision 56),
/// written by the browser; its offset follows only when the viewer's differs.
#[wasm_bindgen_test]
fn a_snapshot_time_is_its_own_clocks_reading_with_an_offset_the_viewer_lacks() {
    let localizer = Localizer::new(&["en-US".parse().unwrap()]);
    assert!(localizer.writes_instants());
    let raw = "2026-11-11T05:30:00Z";
    let viewer = l10n::viewer_zone();
    let fold = |text: String| text.replace(['\u{202f}', '\u{a0}'], " ");
    let shown = |seconds: Option<i32>| {
        let offset = seconds
            .map(|seconds| api::ClockOffset::new(jiff::tz::Offset::from_seconds(seconds).unwrap()));
        fold(crate::routes::snapshot_time(
            &localizer,
            raw,
            offset,
            viewer.as_ref(),
            crate::snapshots::viewer_offset,
            localizer.writes_instants(),
        ))
    };
    // `just test-wasm` pins `TZ=America/New_York`, five hours behind UTC in
    // November.
    assert_eq!(shown(Some(-6 * 3600)), "11/10/26, 11:30:00 PM UTC-06:00");
    assert_eq!(shown(Some(-5 * 3600)), "11/11/26, 12:30:00 AM");
    assert_eq!(shown(Some(0)), "11/11/26, 5:30:00 AM UTC+00:00");
    assert_eq!(shown(None), "11/11/26, 12:30:00 AM", "an older daemon's");
}

/// The viewer's offset is the browser's own clock's at each instant,
/// daylight saving time included.
#[wasm_bindgen_test]
fn the_viewers_offset_is_the_one_the_browsers_clock_had_at_that_instant() {
    use crate::snapshots::viewer_offset;
    use jiff::tz::Offset;
    // `just test-wasm` pins `TZ=America/New_York`.
    let at = |raw: &str| viewer_offset(raw.parse().unwrap());
    assert_eq!(at("2026-11-11T05:30:00Z"), Offset::constant(-5));
    assert_eq!(at("2026-07-01T05:30:00Z"), Offset::constant(-4));
    assert_eq!(
        at("2026-11-01T05:59:59Z"),
        Offset::constant(-4),
        "the last second of summer time"
    );
    assert_eq!(at("2026-11-01T06:00:00Z"), Offset::constant(-5));
}
