//! The ICU4X host: locale-aware numbers and instants for the daemon
//! (technical design §8.4), behind the `icu` feature. It implements the
//! interface of [`crate::format`]: a [`NumberFormatter`] and an
//! [`InstantFormatter`], which the bundle's memoizer builds per locale.
//!
//! The UTC offset and the wall clock of an instant in a zone come from
//! `jiff`, which the daemon already has; ICU4X only writes the result, and
//! brings the locale's names for the zone. A zone `jiff` doesn't know reads as
//! UTC.

use api::IanaZone;
use fixed_decimal::{Decimal, SignedRoundingMode, UnsignedRoundingMode};
use icu_calendar::{Date, Iso};
use icu_datetime::fieldsets::zone::SpecificShort;
use icu_datetime::fieldsets::{T, YMD, YMDT};
use icu_datetime::options::{Length, TimePrecision};
use icu_datetime::{DateTimeFormatter, DateTimeFormatterPreferences, NoCalendarFormatter};
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use icu_decimal::{DecimalFormatter, DecimalFormatterPreferences};
use icu_time::zone::models::AtTime;
use icu_time::zone::{IanaParser, UtcOffset, ZoneNameTimestamp};
use icu_time::{DateTime, Time, TimeZoneInfo};
use unic_langid::LanguageIdentifier;

use crate::format::{
    DateStyle, Host, InstantFormatter, NumberFormatter, NumberOptions, Style, TimeStyle,
};

/// The host built on ICU4X.
pub(crate) struct Icu;

/// An ICU4X decimal formatter with the digit options it was built for.
pub(crate) struct Numbers {
    formatter: DecimalFormatter,
    options: NumberOptions,
}

/// An ICU4X date-time formatter for one [`Style`].
pub(crate) struct Instants(Fields);

/// What a [`Style`] is written with. A time or a zone name needs no calendar,
/// so no calendar data is loaded for them.
enum Fields {
    Date(DateTimeFormatter<YMD>),
    DateTime(DateTimeFormatter<YMDT>),
    Time(NoCalendarFormatter<T>),
    ZoneName(NoCalendarFormatter<SpecificShort>),
}

impl Host for Icu {
    type Numbers = Numbers;
    type Instants = Instants;
}

/// The locale ICU4X reads from one `unic-langid` parsed. Both are BCP 47
/// language identifiers, so this only fails for a tag that ICU4X is stricter
/// about.
fn language(locale: &LanguageIdentifier) -> Option<icu_locale_core::LanguageIdentifier> {
    icu_locale_core::LanguageIdentifier::try_from_str(&locale.to_string()).ok()
}

impl NumberFormatter for Numbers {
    fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self> {
        let grouping = if options.grouping {
            GroupingStrategy::Auto
        } else {
            GroupingStrategy::Never
        };
        let formatter = DecimalFormatter::try_new(
            DecimalFormatterPreferences::from(&language(locale)?),
            DecimalFormatterOptions::from(grouping),
        )
        .ok()?;
        Some(Self { formatter, options })
    }

    fn format(&self, value: f64) -> Option<String> {
        if !value.is_finite() {
            return None;
        }
        // The shortest digits that read back as `value`, and never an
        // exponent: what `f64`'s `Display` writes.
        let mut decimal = Decimal::try_from_str(&value.to_string()).ok()?;
        let options = self.options;
        if let Some(max_fraction) = options.max_fraction {
            decimal.round_with_mode(
                -i16::from(max_fraction),
                SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfExpand),
            );
            // Rounding pads to its position; only `min_fraction` pads.
            decimal.trim_end();
        }
        decimal.pad_end(-i16::from(options.min_fraction));
        decimal.pad_start(i16::from(options.min_integer));
        Some(self.formatter.format(&decimal).to_string())
    }
}

impl InstantFormatter for Instants {
    fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self> {
        let prefs = DateTimeFormatterPreferences::from(&language(locale)?);
        let fields = match style {
            Style::Date(date) => {
                Fields::Date(DateTimeFormatter::try_new(prefs, YMD::for_length(length(date))).ok()?)
            }
            Style::Time(time) => {
                let fields = match time {
                    TimeStyle::Short => T::hm(),
                    TimeStyle::Medium => T::hms(),
                };
                Fields::Time(NoCalendarFormatter::try_new(prefs, fields).ok()?)
            }
            Style::DateTime(date, time) => {
                let fields = YMDT::for_length(length(date)).with_time_precision(precision(time));
                Fields::DateTime(DateTimeFormatter::try_new(prefs, fields).ok()?)
            }
            Style::ZoneName => {
                Fields::ZoneName(NoCalendarFormatter::try_new(prefs, SpecificShort).ok()?)
            }
        };
        Some(Self(fields))
    }

    fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
        let moment = Moment::of(utc_ms, zone)?;
        Some(match &self.0 {
            Fields::Date(formatter) => formatter.format(&moment.when.date).to_string(),
            Fields::DateTime(formatter) => formatter.format(&moment.when).to_string(),
            Fields::Time(formatter) => formatter.format(&moment.when.time).to_string(),
            Fields::ZoneName(formatter) => formatter.format(&moment.zone).to_string(),
        })
    }
}

/// The length of the date part of a [`Style`].
fn length(date: DateStyle) -> Length {
    match date {
        DateStyle::Short => Length::Short,
        DateStyle::Medium => Length::Medium,
        DateStyle::Long => Length::Long,
    }
}

/// How far the time of day of a [`Style`] is written.
fn precision(time: TimeStyle) -> TimePrecision {
    match time {
        TimeStyle::Short => TimePrecision::Minute,
        TimeStyle::Medium => TimePrecision::Second,
    }
}

/// An instant as ICU4X takes it: the wall clock of the zone it is read in,
/// and the zone.
struct Moment {
    when: DateTime<Iso>,
    zone: TimeZoneInfo<AtTime>,
}

impl Moment {
    /// The instant `utc_ms` milliseconds after the Unix epoch in `zone`, or
    /// in UTC when there is none or `jiff` doesn't have it. `None` beyond the
    /// years ±9999, which is where `jiff` stops.
    fn of(utc_ms: i64, zone: Option<&IanaZone>) -> Option<Self> {
        let instant = jiff::Timestamp::from_millisecond(utc_ms).ok()?;
        let (name, rules) = zone
            .and_then(|zone| Some((zone.as_str(), jiff::tz::TimeZone::get(zone.as_str()).ok()?)))
            .unwrap_or(("UTC", jiff::tz::TimeZone::UTC));
        let offset = rules.to_offset(instant);
        let wall = offset.to_datetime(instant);
        let when = DateTime {
            date: Date::try_new_iso(
                i32::from(wall.year()),
                u8::try_from(wall.month()).ok()?,
                u8::try_from(wall.day()).ok()?,
            )
            .ok()?,
            time: Time::try_new(
                u8::try_from(wall.hour()).ok()?,
                u8::try_from(wall.minute()).ok()?,
                u8::try_from(wall.second()).ok()?,
                u32::try_from(wall.subsec_nanosecond()).ok()?,
            )
            .ok()?,
        };
        // The zone's names change at a metazone's transitions, which the
        // instant (to 15 minutes) places; the offset says whether it is on
        // daylight saving time.
        let zone = IanaParser::new()
            .parse(name)
            .with_offset(UtcOffset::try_from_seconds(offset.seconds()).ok())
            .with_zone_name_timestamp(ZoneNameTimestamp::from_epoch_seconds(utc_ms / 1000));
        Some(Self { when, zone })
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
