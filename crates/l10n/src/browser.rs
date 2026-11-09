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

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use api::IanaZone;
use js_sys::{Array, Date, Function, Object, Reflect};
use unic_langid::LanguageIdentifier;
use wasm_bindgen::{JsCast, JsValue};

use crate::format::{
    DateStyle, Host, InstantFormatter, NumberFormatter, NumberOptions, Style, TimeStyle,
};

/// The most time zones an [`Instants`] keeps a formatter for. Zone names come
/// from the input, so the cache is bounded; a zone beyond it is built for the
/// one call.
const MAX_ZONES: usize = 64;

/// The host built on the browser's `Intl`.
pub(crate) struct Browser;

/// An `Intl.NumberFormat`.
pub(crate) struct Numbers {
    formatter: JsValue,
}

/// An `Intl.DateTimeFormat` per time zone, for one [`Style`]: the zone is a
/// constructor option in `Intl`, not an argument of `format`.
pub(crate) struct Instants {
    locale: String,
    style: Style,
    /// The formatter of UTC: no zone, and a zone the browser doesn't have,
    /// are read as UTC, never as the viewer's.
    utc: JsValue,
    /// The formatters built so far, by zone name; `None` for a zone the
    /// browser refused.
    zones: Mutex<HashMap<String, Option<JsValue>>>,
}

impl Host for Browser {
    type Numbers = Numbers;
    type Instants = Instants;
}

/// `new Intl[name](locale, options)`, or `None` when the browser has no such
/// constructor or it throws: a `RangeError` for an option or a time zone it
/// refuses. A bad option never reaches the caller as a panic or an exception.
pub(crate) fn construct(name: &str, locale: &str, options: &Object) -> Option<JsValue> {
    let constructor = intl(name)?.dyn_into::<Function>().ok()?;
    let arguments = Array::of2(&JsValue::from_str(locale), options);
    Reflect::construct(&constructor, &arguments).ok()
}

/// `Intl[name]`, if the browser has it.
fn intl(name: &str) -> Option<JsValue> {
    let global = Reflect::get(&js_sys::global(), &JsValue::from_str("Intl")).ok()?;
    Reflect::get(&global, &JsValue::from_str(name))
        .ok()
        .filter(|member| !member.is_undefined())
}

/// Whether `Intl[name]` has data for `locale` itself. `Intl` otherwise
/// writes a locale it doesn't know in the runtime's own, and the plain text
/// is better than another locale's.
fn supports(name: &str, locale: &str) -> bool {
    let Some(constructor) = intl(name) else {
        return false;
    };
    let Some(supported) = member(&constructor, "supportedLocalesOf") else {
        return false;
    };
    supported
        .call1(&constructor, &Array::of1(&JsValue::from_str(locale)))
        .ok()
        .and_then(|list| list.dyn_into::<Array>().ok())
        .is_some_and(|list| list.length() > 0)
}

/// The function `object[name]`.
fn member(object: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(object, &JsValue::from_str(name))
        .ok()?
        .dyn_into()
        .ok()
}

/// `object[name](argument)`: a call that can't throw into Rust, for `None`.
fn call(object: &JsValue, name: &str, argument: &JsValue) -> Option<JsValue> {
    member(object, name)?.call1(object, argument).ok()
}

/// Sets `name` on `options`. A plain object takes any property.
fn set(options: &Object, name: &str, value: &JsValue) {
    Reflect::set(options, &JsValue::from_str(name), value).ok();
}

impl NumberFormatter for Numbers {
    fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self> {
        let locale = locale.to_string();
        if !supports("NumberFormat", &locale) {
            return None;
        }
        let object = Object::new();
        set(
            &object,
            "minimumIntegerDigits",
            &JsValue::from(options.min_integer),
        );
        set(
            &object,
            "minimumFractionDigits",
            &JsValue::from(options.min_fraction),
        );
        // Without a maximum every digit shows, not `Intl`'s default of three.
        let max_fraction = options
            .max_fraction
            .unwrap_or(crate::format::MAX_FRACTION_DIGITS);
        set(
            &object,
            "maximumFractionDigits",
            &JsValue::from(max_fraction),
        );
        if !options.grouping {
            set(&object, "useGrouping", &JsValue::FALSE);
        }
        let formatter = construct("NumberFormat", &locale, &object)?;
        Some(Self { formatter })
    }

    fn format(&self, value: f64) -> Option<String> {
        if !value.is_finite() {
            return None;
        }
        // The shortest digits that read back as `value`, never in an
        // exponent form: what `f64`'s `Display` writes. `Intl` takes a
        // numeric string as the exact decimal it spells, so it rounds half
        // away from zero on these digits, as the other hosts do.
        let digits = JsValue::from_str(&value.to_string());
        call(&self.formatter, "format", &digits)?.as_string()
    }
}

impl Instants {
    /// An `Intl.DateTimeFormat` of `style` in the time zone `zone`; `None`
    /// when the browser has no such zone.
    fn build(locale: &str, style: Style, zone: &str) -> Option<JsValue> {
        let options = Object::new();
        match style {
            Style::Date(date) => set(&options, "dateStyle", &length(date)),
            Style::Time(time) => set(&options, "timeStyle", &precision(time)),
            Style::DateTime(date, time) => {
                set(&options, "dateStyle", &length(date));
                set(&options, "timeStyle", &precision(time));
            }
            // The name is one part of the date written with it.
            Style::ZoneName => set(&options, "timeZoneName", &JsValue::from_str("short")),
        }
        set(&options, "timeZone", &JsValue::from_str(zone));
        construct("DateTimeFormat", locale, &options)
    }

    /// The formatter that reads an instant in `zone`: the zone's, or UTC's
    /// where there is no zone or the browser doesn't have it.
    fn formatter(&self, zone: Option<&IanaZone>) -> JsValue {
        let Some(zone) = zone else {
            return self.utc.clone();
        };
        let mut zones = self.zones.lock().unwrap_or_else(PoisonError::into_inner);
        let found = match zones.get(zone.as_str()) {
            Some(found) => found.clone(),
            None => {
                let built = Self::build(&self.locale, self.style, zone.as_str());
                if zones.len() < MAX_ZONES {
                    zones.insert(zone.as_str().to_owned(), built.clone());
                }
                built
            }
        };
        found.unwrap_or_else(|| self.utc.clone())
    }
}

/// The `dateStyle` word of a date length.
fn length(date: DateStyle) -> JsValue {
    JsValue::from_str(match date {
        DateStyle::Short => "short",
        DateStyle::Medium => "medium",
        DateStyle::Long => "long",
    })
}

/// The `timeStyle` word of a time precision: to the minute or the second.
fn precision(time: TimeStyle) -> JsValue {
    JsValue::from_str(match time {
        TimeStyle::Short => "short",
        TimeStyle::Medium => "medium",
    })
}

/// The short name of the zone in the `parts` of a formatted date
/// (`Intl.DateTimeFormat.prototype.formatToParts`).
fn zone_name(parts: &JsValue) -> Option<String> {
    let parts = parts.dyn_ref::<Array>()?;
    parts.iter().find_map(|part| {
        let kind = Reflect::get(&part, &JsValue::from_str("type")).ok()?;
        if kind.as_string()? != "timeZoneName" {
            return None;
        }
        Reflect::get(&part, &JsValue::from_str("value"))
            .ok()?
            .as_string()
    })
}

impl InstantFormatter for Instants {
    fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self> {
        let locale = locale.to_string();
        if !supports("DateTimeFormat", &locale) {
            return None;
        }
        let utc = Self::build(&locale, style, "UTC")?;
        Some(Self {
            locale,
            style,
            utc,
            zones: Mutex::new(HashMap::new()),
        })
    }

    fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        // The years a calendar can show, ±9999: the bound of the other
        // hosts, and well inside the ±8.64e15 ms a `Date` takes.
        let milliseconds = jiff::Timestamp::from_millisecond(utc_ms)
            .ok()?
            .as_millisecond();
        #[allow(
            clippy::cast_precision_loss,
            reason = "within the years ±9999, far below 2^53 milliseconds"
        )]
        let date = Date::new(&JsValue::from_f64(milliseconds as f64));
        let formatter = self.formatter(zone);
        match self.style {
            Style::ZoneName => zone_name(&call(&formatter, "formatToParts", &date)?),
            _ => call(&formatter, "format", &date)?.as_string(),
        }
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
