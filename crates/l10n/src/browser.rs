//! The browser host: locale-aware numbers and instants for the web UI
//! (technical design §8.4), behind the `browser-intl` feature, on wasm32. It
//! implements the interface of [`crate::format`] over `Intl.NumberFormat` and
//! `Intl.DateTimeFormat`, which the bundle's memoizer builds per locale.
//!
//! A `JsValue` is `Send` and `Sync` on wasm32 without the `atomics` target
//! feature (wasm-bindgen's `src/lib.rs`), which is how the web UI builds, so
//! the formatters satisfy [`Host`]'s `Send + Sync` as they are and the bundle
//! stays the concurrent one the daemon uses. Built with `atomics`, this does
//! not compile, rather than share a `JsValue` across threads.
#![allow(dead_code, reason = "stubs, until the host is written")]

use api::IanaZone;
use js_sys::Object;
use wasm_bindgen::JsValue;

use crate::format::{Host, InstantFormatter, NumberFormatter, NumberOptions, Style};
use unic_langid::LanguageIdentifier;

/// The host built on the browser's `Intl`.
pub(crate) struct Browser;

/// An `Intl.NumberFormat` with the digit options it was built for.
pub(crate) struct Numbers {
    formatter: js_sys::Intl::NumberFormat,
    options: NumberOptions,
}

/// An `Intl.DateTimeFormat` for one [`Style`].
pub(crate) struct Instants {
    formatter: js_sys::Intl::DateTimeFormat,
}

impl Host for Browser {
    type Numbers = Numbers;
    type Instants = Instants;
}

/// `new Intl[name](locale, options)`, or `None` when the browser has no such
/// constructor or it throws: a `RangeError` for an option or a time zone it
/// refuses. A bad option never reaches the caller as a panic or an exception.
pub(crate) fn construct(name: &str, locale: &str, options: &Object) -> Option<JsValue> {
    let _ = (name, locale, options);
    todo!("construct the Intl formatter with Reflect::construct")
}

impl NumberFormatter for Numbers {
    fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self> {
        let _ = (locale, options);
        todo!("build an Intl.NumberFormat")
    }

    fn format(&self, value: f64) -> Option<String> {
        let _ = (value, &self.formatter, self.options);
        todo!("format with the Intl.NumberFormat")
    }
}

impl InstantFormatter for Instants {
    fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self> {
        let _ = (locale, style);
        todo!("build an Intl.DateTimeFormat")
    }

    fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        let _ = (utc_ms, zone, &self.formatter);
        todo!("format with the Intl.DateTimeFormat")
    }
}

#[cfg(test)]
mod tests {
    use js_sys::Reflect;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use crate::format::{DateStyle, TimeStyle};

    /// 2026-11-08 13:30:45 UTC.
    const WINTER: i64 = 1_794_144_645_000;

    fn tag(raw: &str) -> LanguageIdentifier {
        raw.parse().unwrap()
    }

    fn zone(name: &str) -> IanaZone {
        IanaZone::new(name).unwrap()
    }

    /// V8 writes U+0020 where CLDR writes U+202F or U+00A0.
    fn fold(text: &str) -> String {
        text.replace(['\u{202f}', '\u{a0}'], " ")
    }

    fn options(entries: &[(&str, JsValue)]) -> Object {
        let object = Object::new();
        for (name, value) in entries {
            Reflect::set(&object, &JsValue::from_str(name), value).unwrap();
        }
        object
    }

    fn digits(max_fraction: Option<u8>) -> NumberOptions {
        NumberOptions {
            min_integer: 1,
            min_fraction: 0,
            max_fraction,
            grouping: true,
        }
    }

    fn instant(locale: &str, style: Style, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        Instants::new(&tag(locale), style)?
            .format(utc_ms, zone)
            .map(|text| fold(&text))
    }

    #[wasm_bindgen_test]
    fn an_option_the_browser_refuses_is_declined_not_thrown() {
        // `Intl` allows 100 fraction digits at most: a RangeError.
        let bad = options(&[("maximumFractionDigits", JsValue::from_f64(101.0))]);
        assert!(construct("NumberFormat", "en-US", &bad).is_none());
        let good = options(&[("maximumFractionDigits", JsValue::from_f64(2.0))]);
        assert!(construct("NumberFormat", "en-US", &good).is_some());
    }

    #[wasm_bindgen_test]
    fn a_time_zone_the_browser_does_not_have_is_declined_not_thrown() {
        let atlantis = options(&[("timeZone", JsValue::from_str("Europe/Atlantis"))]);
        assert!(construct("DateTimeFormat", "en-US", &atlantis).is_none());
        let utc = options(&[("timeZone", JsValue::from_str("UTC"))]);
        assert!(construct("DateTimeFormat", "en-US", &utc).is_some());
    }

    #[wasm_bindgen_test]
    fn a_constructor_the_browser_does_not_have_is_declined() {
        assert!(construct("NoSuchFormat", "en-US", &Object::new()).is_none());
    }

    #[wasm_bindgen_test]
    fn a_locale_the_browser_has_no_data_for_is_declined_not_read_as_its_default() {
        // `Intl` silently writes an unsupported locale in the runtime's own
        // (`new Intl.NumberFormat("tlh")` writes en-US), and the plain text is
        // better than another locale's.
        assert!(Numbers::new(&tag("tlh"), digits(None)).is_none());
        assert!(Instants::new(&tag("tlh"), Style::DEFAULT).is_none());
        assert!(Numbers::new(&tag("de"), digits(None)).is_some());
        assert!(Instants::new(&tag("de"), Style::DEFAULT).is_some());
    }

    #[wasm_bindgen_test]
    fn a_number_that_is_not_finite_is_declined() {
        let numbers = Numbers::new(&tag("en-US"), digits(None)).unwrap();
        assert_eq!(numbers.format(1234.5).as_deref(), Some("1,234.5"));
        assert_eq!(numbers.format(f64::NAN), None);
        assert_eq!(numbers.format(f64::INFINITY), None);
        assert_eq!(numbers.format(f64::NEG_INFINITY), None);
    }

    #[wasm_bindgen_test]
    fn a_number_is_rounded_half_away_from_zero_on_its_shortest_digits() {
        // `Intl` rounds the exact binary value of 1.005, which is below the
        // half; the interface rounds its shortest decimal digits.
        let numbers = Numbers::new(&tag("en-US"), digits(Some(2))).unwrap();
        assert_eq!(numbers.format(1.005).as_deref(), Some("1.01"));
        assert_eq!(numbers.format(-1.005).as_deref(), Some("-1.01"));
    }

    #[wasm_bindgen_test]
    fn an_instant_beyond_the_years_a_calendar_can_show_is_declined() {
        let last = jiff::Timestamp::MAX.as_second() * 1000;
        let first = jiff::Timestamp::MIN.as_millisecond();
        for utc_ms in [i64::MAX, i64::MIN, last + 1, first - 1] {
            assert_eq!(instant("en-US", Style::DEFAULT, utc_ms, None), None);
        }
        assert_eq!(
            instant("en-US", Style::DEFAULT, last, None).as_deref(),
            Some("Dec 30, 9999, 10:00 PM")
        );
        let shown = instant("en-US", Style::DEFAULT, first, None).unwrap();
        assert!(shown.starts_with("Jan 2, "), "{shown}");
    }

    #[wasm_bindgen_test]
    fn no_zone_and_a_zone_nobody_has_are_read_as_utc_not_as_the_viewers() {
        let atlantis = zone("Europe/Atlantis");
        let utc = instant("en-US", Style::DEFAULT, WINTER, Some(&zone("UTC")));
        assert_eq!(utc.as_deref(), Some("Nov 8, 2026, 1:30 PM"));
        assert_eq!(instant("en-US", Style::DEFAULT, WINTER, None), utc);
        assert_eq!(
            instant("en-US", Style::DEFAULT, WINTER, Some(&atlantis)),
            utc
        );
        // The name says UTC, not the offset of a zone the browser never had.
        assert_eq!(
            instant("en-US", Style::ZoneName, WINTER, Some(&atlantis)).as_deref(),
            Some("UTC")
        );
    }

    #[wasm_bindgen_test]
    fn an_instant_is_read_in_its_zone_with_the_locales_wording() {
        let berlin = zone("Europe/Berlin");
        let long_short = Style::DateTime(DateStyle::Long, TimeStyle::Short);
        assert_eq!(
            instant("de", long_short, WINTER, Some(&berlin)).as_deref(),
            Some("8. November 2026 um 14:30")
        );
        // The same formatter serves two zones, one after the other.
        let formatter = Instants::new(&tag("en-US"), Style::DEFAULT).unwrap();
        let tokyo = zone("Asia/Tokyo");
        assert_eq!(
            fold(&formatter.format(WINTER, Some(&berlin)).unwrap()),
            "Nov 8, 2026, 2:30 PM"
        );
        assert_eq!(
            fold(&formatter.format(WINTER, Some(&tokyo)).unwrap()),
            "Nov 8, 2026, 10:30 PM"
        );
    }
}
