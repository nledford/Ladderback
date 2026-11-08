//! The ICU4X host: locale-aware numbers and instants for the daemon
//! (technical design §8.4), behind the `icu` feature. It implements the
//! interface of [`crate::format`]: a [`NumberFormatter`] and an
//! [`InstantFormatter`], which the bundle's memoizer builds per locale.
//!
//! The UTC offset and the daylight-saving state of an instant in a zone come
//! from `jiff`, which the daemon already has; ICU4X only writes the result,
//! and brings the locale's names for the zone. A zone `jiff` doesn't know
//! reads as UTC.

use api::IanaZone;
use unic_langid::LanguageIdentifier;

use crate::format::{Host, InstantFormatter, NumberFormatter, NumberOptions, Style};

/// The host built on ICU4X.
pub(crate) struct Icu;

/// An ICU4X decimal formatter with the digit options it was built for.
pub(crate) struct Numbers;

/// An ICU4X date-time formatter for one [`Style`].
pub(crate) struct Instants;

impl Host for Icu {
    type Numbers = Numbers;
    type Instants = Instants;
}

impl NumberFormatter for Numbers {
    fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self> {
        todo!("a decimal formatter for {locale} with {options:?}")
    }

    fn format(&self, value: f64) -> Option<String> {
        todo!("write {value}")
    }
}

impl InstantFormatter for Instants {
    fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self> {
        todo!("a {style:?} formatter for {locale}")
    }

    fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        todo!("write {utc_ms} in {zone:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{DateStyle, TimeStyle};

    /// 2026-11-08 13:30:45 UTC, and 2026-07-08 13:30:00 UTC.
    const WINTER: i64 = 1_794_144_645_000;
    const SUMMER: i64 = 1_783_517_400_000;

    fn tag(raw: &str) -> LanguageIdentifier {
        raw.parse().unwrap()
    }

    fn zone(name: &str) -> IanaZone {
        IanaZone::new(name).unwrap()
    }

    fn number(
        locale: &str,
        value: f64,
        min_fraction: u8,
        max_fraction: Option<u8>,
    ) -> Option<String> {
        let options = NumberOptions {
            min_integer: 1,
            min_fraction,
            max_fraction,
            grouping: true,
        };
        Numbers::new(&tag(locale), options)?.format(value)
    }

    fn instant(locale: &str, style: Style, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        Instants::new(&tag(locale), style)?.format(utc_ms, zone)
    }

    #[test]
    fn a_number_is_grouped_and_pointed_as_the_locale_writes_it() {
        assert_eq!(
            number("en-US", 1_234_567.0, 0, None).as_deref(),
            Some("1,234,567")
        );
        assert_eq!(
            number("de", 1_234_567.0, 0, None).as_deref(),
            Some("1.234.567")
        );
        assert_eq!(number("de", 1.5, 0, None).as_deref(), Some("1,5"));
        assert_eq!(
            number("ja", 1_234_567.0, 0, None).as_deref(),
            Some("1,234,567")
        );
    }

    #[test]
    fn a_number_is_rounded_half_away_from_zero_and_padded_as_asked() {
        assert_eq!(number("en-US", 1.25, 0, Some(1)).as_deref(), Some("1.3"));
        assert_eq!(number("en-US", 0.04, 0, Some(1)).as_deref(), Some("0"));
        assert_eq!(number("en-US", 1.005, 0, Some(2)).as_deref(), Some("1.01"));
        assert_eq!(number("en-US", 1.5, 2, None).as_deref(), Some("1.50"));
    }

    #[test]
    fn a_number_that_is_not_finite_is_declined() {
        assert_eq!(number("en-US", f64::NAN, 0, None), None);
        assert_eq!(number("en-US", f64::INFINITY, 0, None), None);
        assert_eq!(number("en-US", f64::NEG_INFINITY, 0, None), None);
    }

    #[test]
    fn an_american_time_has_a_narrow_no_break_space_before_the_day_period() {
        let berlin = zone("Europe/Berlin");
        assert_eq!(
            instant("en-US", Style::DEFAULT, WINTER, Some(&berlin)).as_deref(),
            Some("Nov 8, 2026, 2:30\u{202f}PM")
        );
    }

    #[test]
    fn an_instant_is_read_in_its_zone_with_the_locales_wording() {
        let berlin = zone("Europe/Berlin");
        let long_short = Style::DateTime(DateStyle::Long, TimeStyle::Short);
        assert_eq!(
            instant("de", long_short, WINTER, Some(&berlin)).as_deref(),
            Some("8. November 2026 um 14:30")
        );
        assert_eq!(
            instant("ja", long_short, WINTER, Some(&berlin)).as_deref(),
            Some("2026年11月8日 14:30")
        );
    }

    #[test]
    fn the_name_of_a_zone_follows_daylight_saving_time_and_the_locale() {
        let berlin = zone("Europe/Berlin");
        let name = |locale, utc_ms| instant(locale, Style::ZoneName, utc_ms, Some(&berlin));
        assert_eq!(name("de", WINTER).as_deref(), Some("MEZ"));
        assert_eq!(name("de", SUMMER).as_deref(), Some("MESZ"));
        assert_eq!(name("en-US", WINTER).as_deref(), Some("GMT+1"));
        assert_eq!(name("en-US", SUMMER).as_deref(), Some("GMT+2"));
    }

    #[test]
    fn no_zone_and_a_zone_nobody_knows_are_read_as_utc() {
        let atlantis = zone("Europe/Atlantis");
        let utc = instant("en-US", Style::DEFAULT, WINTER, Some(&zone("UTC")));
        assert_eq!(utc.as_deref(), Some("Nov 8, 2026, 1:30\u{202f}PM"));
        assert_eq!(instant("en-US", Style::DEFAULT, WINTER, None), utc);
        assert_eq!(
            instant("en-US", Style::DEFAULT, WINTER, Some(&atlantis)),
            utc
        );
        // The name says UTC, not an offset the zone never had.
        assert_eq!(
            instant("en-US", Style::ZoneName, WINTER, Some(&atlantis)).as_deref(),
            Some("UTC")
        );
    }

    #[test]
    fn an_instant_no_calendar_can_show_is_declined() {
        assert_eq!(instant("en-US", Style::DEFAULT, i64::MAX, None), None);
        assert_eq!(instant("en-US", Style::DEFAULT, i64::MIN, None), None);
    }
}
