//! The web UI's lane for numbers and instants (technical design §8.4), run by
//! `just test-wasm` under Node: a localizer built in this crate's own
//! feature set, which is the one the page runs with, writes `NUMBER` and
//! `DATETIME` as the browser's `Intl` does, not as the plain fallbacks do. The
//! table of every case a host writes is `l10n`'s own lane; this is the check
//! that `web` is built with that host.
//!
//! The catalog is the test's own: the web UI has no message that shows a
//! number or an instant yet. The first page that does adds the `@ui` scenario.

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
