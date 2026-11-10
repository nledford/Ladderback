//! Locale-aware formatting of numbers and instants: the one place a host's
//! formatter plugs in (technical design §8.4).
//!
//! # The interface a host implements
//!
//! A [`Host`] is a pair of formatters, a [`NumberFormatter`] and an
//! [`InstantFormatter`]. The bundle's memoizer builds them lazily, one per
//! locale and configuration (a [`NumberFormatter`] per [`NumberOptions`], an
//! [`InstantFormatter`] per [`Style`]), and keeps them for the bundle's life;
//! [`install`] is what puts a host on a bundle. A host can decline, and then
//! the text is the plain one, so a host never has to be total:
//!
//! - `new` returns `None` when the host has no data for the locale;
//! - `format` returns `None` for a value it can't write.
//!
//! The plain text is Fluent's own number, and an instant in UTC. It is what a
//! build without a host (neither the daemon's nor the web UI's) writes.
//!
//! No input makes formatting fail, panic or allocate without limit: digit
//! counts are capped before a host sees them, and an instant no calendar can
//! show (beyond the years ±9999) is declined.
//!
//! # What is a host's to decide
//!
//! The vocabulary is closed, so every host can honour all of it:
//!
//! - **Numbers** follow the `NUMBER` options `minimumIntegerDigits`,
//!   `minimumFractionDigits`, `maximumFractionDigits` and `useGrouping`
//!   (`"false"` only). Anything else is ignored, as Fluent's own formatter
//!   ignores it. Without `maximumFractionDigits` every digit of the value is
//!   shown, not Intl's default of three. A value is rounded half away from
//!   zero on its shortest decimal digits, so `1.005` at two digits is `1.01`,
//!   as both ICU4X and `Intl` do.
//! - **Instants** follow `DATETIME`'s [`Style`]. The zone is how the instant
//!   is read: without one, or with one the host doesn't know, it is read as
//!   UTC, on every host, and never as the viewer's: a page that wants the
//!   viewer's zone passes it, as the web UI does (`viewer_zone`).

use std::borrow::Cow;
use std::convert::Infallible;

use api::IanaZone;
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::types::{FluentNumber, FluentNumberOptions, FluentType};
use fluent_bundle::{FluentArgs, FluentValue};
use intl_memoizer::Memoizable;
use intl_memoizer::concurrent::IntlLangMemoizer;
use unic_langid::LanguageIdentifier;

/// The most fraction digits a number is written with: ECMA-402's own limit.
pub(crate) const MAX_FRACTION_DIGITS: u8 = 100;

/// The most integer digits a number is padded to: ECMA-402's own limit.
pub(crate) const MAX_INTEGER_DIGITS: u8 = 21;

/// How a number is written: the digit options of `NUMBER` a host honours,
/// bounded and consistent. A host never sees more than the caps, a minimum
/// above the maximum, or no integer digit at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NumberOptions {
    /// Integer digits at least, padded with zeros: 1 to [`MAX_INTEGER_DIGITS`].
    pub(crate) min_integer: u8,
    /// Fraction digits at least, padded with zeros: 0 to
    /// [`MAX_FRACTION_DIGITS`], and at most `max_fraction`.
    pub(crate) min_fraction: u8,
    /// Fraction digits at most, rounding half away from zero; `None` shows
    /// every digit of the value.
    pub(crate) max_fraction: Option<u8>,
    /// Whether the locale's digit grouping applies.
    pub(crate) grouping: bool,
}

impl From<&FluentNumberOptions> for NumberOptions {
    /// The options of a Fluent number, capped. When the minimum fraction
    /// digits exceed the maximum, the maximum wins: a literal such as
    /// `NUMBER(1.25, maximumFractionDigits: 1)` carries its own decimals as a
    /// minimum, and the author asked for fewer.
    fn from(options: &FluentNumberOptions) -> Self {
        let max_fraction = options
            .maximum_fraction_digits
            .map(|digits| capped(digits, MAX_FRACTION_DIGITS));
        let min_fraction = capped(
            options.minimum_fraction_digits.unwrap_or(0),
            MAX_FRACTION_DIGITS,
        );
        Self {
            min_integer: capped(
                options.minimum_integer_digits.unwrap_or(1),
                MAX_INTEGER_DIGITS,
            )
            .max(1),
            min_fraction: max_fraction.map_or(min_fraction, |max| min_fraction.min(max)),
            max_fraction,
            grouping: options.use_grouping,
        }
    }
}

/// `digits`, no more than `cap`.
fn capped(digits: usize, cap: u8) -> u8 {
    u8::try_from(digits).map_or(cap, |digits| digits.min(cap))
}

/// How much of an instant `DATETIME` shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DateStyle {
    Short,
    Medium,
    Long,
}

/// How much of the time of day `DATETIME` shows: to the minute, or the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TimeStyle {
    Short,
    Medium,
}

/// What `DATETIME` writes of an instant: its closed vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Style {
    /// `dateStyle` alone.
    Date(DateStyle),
    /// `timeStyle` alone.
    Time(TimeStyle),
    /// Both.
    DateTime(DateStyle, TimeStyle),
    /// `timeZoneName: "short"` alone: the zone's short name, such as `CET`
    /// or `GMT+1`. Browsers refuse it together with a style, so it is never
    /// combined with one.
    ZoneName,
}

impl Style {
    /// A call without options: a medium date with a short time.
    pub(crate) const DEFAULT: Self = Self::DateTime(DateStyle::Medium, TimeStyle::Short);

    /// The style the named options of a `DATETIME` call ask for. `None` for
    /// anything outside the vocabulary: an option it doesn't have
    /// (`dateStyle: "full"`, `timeStyle: "long"`, any other name or any value
    /// that is not text) or `timeZoneName` together with a style.
    pub(crate) fn from_options(options: &FluentArgs) -> Option<Self> {
        let (mut date, mut time, mut zone_name) = (None, None, false);
        for (name, value) in options.iter() {
            let FluentValue::String(word) = value else {
                return None;
            };
            match (name, word.as_ref()) {
                ("dateStyle", "short") => date = Some(DateStyle::Short),
                ("dateStyle", "medium") => date = Some(DateStyle::Medium),
                ("dateStyle", "long") => date = Some(DateStyle::Long),
                ("timeStyle", "short") => time = Some(TimeStyle::Short),
                ("timeStyle", "medium") => time = Some(TimeStyle::Medium),
                ("timeZoneName", "short") => zone_name = true,
                _ => return None,
            }
        }
        match (date, time, zone_name) {
            (None, None, false) => Some(Self::DEFAULT),
            (Some(date), None, false) => Some(Self::Date(date)),
            (None, Some(time), false) => Some(Self::Time(time)),
            (Some(date), Some(time), false) => Some(Self::DateTime(date, time)),
            (None, None, true) => Some(Self::ZoneName),
            (_, _, true) => None,
        }
    }
}

/// A host's formatter of numbers for one locale and one [`NumberOptions`].
pub(crate) trait NumberFormatter: Sized {
    /// A formatter, or `None` when the host has no data for `locale`.
    fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self>;

    /// `value` as the locale writes it, or `None` when the host can't write
    /// it (not a number, infinite).
    fn format(&self, value: f64) -> Option<String>;
}

/// A host's formatter of instants for one locale and one [`Style`].
pub(crate) trait InstantFormatter: Sized {
    /// A formatter, or `None` when the host has no data for `locale`.
    fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self>;

    /// The instant `utc_ms` milliseconds after the Unix epoch, read in `zone`:
    /// UTC when there is none, or the host doesn't know it. `None` when the
    /// host can't write the instant (beyond the years ±9999).
    fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String>;
}

/// A host: the formatters it builds. They are shared by every thread that
/// formats with a bundle, so they are `Send` and `Sync`.
pub(crate) trait Host {
    type Numbers: NumberFormatter + Send + Sync + 'static;
    type Instants: InstantFormatter + Send + Sync + 'static;
}

/// Where no host formatter is built in: every number and instant is written
/// plainly. A build with the `icu` feature has no use for it, but the tests
/// of the interface do.
#[cfg(any(
    test,
    not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32")))
))]
pub(crate) struct NoHost;

/// What [`NoHost`] builds: formatters that don't exist.
#[cfg(any(
    test,
    not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32")))
))]
pub(crate) struct Nothing;

#[cfg(any(
    test,
    not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32")))
))]
impl Host for NoHost {
    type Numbers = Nothing;
    type Instants = Nothing;
}

/// The host a build writes with where the `browser-intl` feature is on, for
/// wasm32: the browser's `Intl`, the web UI's. It comes before ICU4X's, which
/// a test build of `l10n` turns on as well.
#[cfg(all(feature = "browser-intl", target_arch = "wasm32"))]
pub(crate) type Active = crate::browser::Browser;

/// The host a build writes with where the `icu` feature is on, and the
/// browser's is not: ICU4X's, the daemon's.
#[cfg(all(
    feature = "icu",
    not(all(feature = "browser-intl", target_arch = "wasm32"))
))]
pub(crate) type Active = crate::icu::Icu;

/// The host a build writes with where neither feature is on: none.
#[cfg(not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32"))))]
pub(crate) type Active = NoHost;

#[cfg(any(
    test,
    not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32")))
))]
impl NumberFormatter for Nothing {
    fn new(_: &LanguageIdentifier, _: NumberOptions) -> Option<Self> {
        None
    }

    fn format(&self, _: f64) -> Option<String> {
        None
    }
}

#[cfg(any(
    test,
    not(any(feature = "icu", all(feature = "browser-intl", target_arch = "wasm32")))
))]
impl InstantFormatter for Nothing {
    fn new(_: &LanguageIdentifier, _: Style) -> Option<Self> {
        None
    }

    fn format(&self, _: i64, _: Option<&IanaZone>) -> Option<String> {
        None
    }
}

/// A number written plainly: Fluent's own text for it, padded with zeros to
/// the options' minimum fraction digits. No grouping, no rounding.
pub(crate) fn plain_number(value: f64, options: NumberOptions) -> String {
    let mut text = value.to_string();
    if value.is_finite() && options.min_fraction > 0 {
        let present = text.split_once('.').map_or(0, |(_, digits)| digits.len());
        if present == 0 {
            text.push('.');
        }
        let missing = usize::from(options.min_fraction).saturating_sub(present);
        text.extend(std::iter::repeat_n('0', missing));
    }
    text
}

/// An instant written plainly, whatever its zone: the parts the style shows,
/// in UTC and numerically (`2026-11-08 13:30:45 UTC`, `2026-11-08 UTC`,
/// `13:30:45 UTC`, `UTC`). Lengths make no difference. An instant outside the
/// years a calendar date can show (±9999) is written as its millisecond
/// count, unless only the zone is asked for.
pub(crate) fn plain_instant(utc_ms: i64, style: Style) -> String {
    match style {
        Style::ZoneName => "UTC".to_owned(),
        Style::Date(_) => in_utc(utc_ms, "%Y-%m-%d UTC"),
        Style::Time(_) => in_utc(utc_ms, "%H:%M:%S UTC"),
        Style::DateTime(..) => instant(utc_ms),
    }
}

/// An instant, as written in a message: `2026-11-08 13:30:45 UTC`. An
/// instant outside the years a calendar date can show (±9999) is written as
/// its millisecond count, so no input can make this fail.
fn instant(utc_ms: i64) -> String {
    in_utc(utc_ms, "%Y-%m-%d %H:%M:%S UTC")
}

/// An instant in UTC as the `strftime` `pattern` writes it, or its
/// millisecond count when no calendar date can show it.
fn in_utc(utc_ms: i64, pattern: &str) -> String {
    jiff::Timestamp::from_millisecond(utc_ms).map_or_else(
        |_| utc_ms.to_string(),
        |instant| instant.strftime(pattern).to_string(),
    )
}

/// An instant as a Fluent value: what a [`MsgArg::Timestamp`](api::MsgArg)
/// becomes, and what `DATETIME` returns. It is written by the bundle's host,
/// in its [`Style`], or plainly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Timestamp {
    pub(crate) utc_ms: i64,
    pub(crate) zone: Option<IanaZone>,
    pub(crate) style: Style,
}

impl Timestamp {
    /// An instant in the default style.
    pub(crate) fn new(utc_ms: i64, zone: Option<IanaZone>) -> Self {
        Self {
            utc_ms,
            zone,
            style: Style::DEFAULT,
        }
    }
}

impl FluentType for Timestamp {
    fn duplicate(&self) -> Box<dyn FluentType + Send> {
        Box::new(self.clone())
    }

    fn as_string(&self, _: &intl_memoizer::IntlLangMemoizer) -> Cow<'static, str> {
        Cow::Owned(plain_instant(self.utc_ms, self.style))
    }

    fn as_string_threadsafe(&self, _: &IntlLangMemoizer) -> Cow<'static, str> {
        Cow::Owned(plain_instant(self.utc_ms, self.style))
    }
}

/// Puts the host `H` on `bundle`: the `DATETIME` function, and a formatter of
/// every written number and [`Timestamp`]. `NUMBER` is Fluent's own, which
/// keeps a count numeric for plural selectors. Call it once per bundle: a
/// second call finds `DATETIME` already there.
pub(crate) fn install<H: Host, R>(bundle: &mut FluentBundle<R>) {
    bundle.set_formatter(Some(written::<H>));
    // An error only means `DATETIME` was already added.
    let _ = bundle.add_function("DATETIME", datetime);
}

/// A number formatter of the host `N`, or the lack of one, as the bundle's
/// memoizer keeps it: one per [`NumberOptions`] of a locale.
struct NumberMemo<N>(Option<N>);

impl<N: NumberFormatter + Send + Sync + 'static> Memoizable for NumberMemo<N> {
    type Args = NumberOptions;
    type Error = Infallible;

    fn construct(locale: LanguageIdentifier, options: NumberOptions) -> Result<Self, Infallible> {
        Ok(Self(N::new(&locale, options)))
    }
}

/// The same for instants: one formatter per [`Style`] of a locale.
struct InstantMemo<I>(Option<I>);

impl<I: InstantFormatter + Send + Sync + 'static> Memoizable for InstantMemo<I> {
    type Args = Style;
    type Error = Infallible;

    fn construct(locale: LanguageIdentifier, style: Style) -> Result<Self, Infallible> {
        Ok(Self(I::new(&locale, style)))
    }
}

/// The bundle's formatter hook: what the host writes for a number or a
/// [`Timestamp`], and the plain text where it declines. Every other value is
/// left to Fluent. The memoizer holds its lock while it builds and runs a
/// formatter, so a host must never panic.
fn written<H: Host>(value: &FluentValue<'_>, intls: &IntlLangMemoizer) -> Option<String> {
    if let FluentValue::Number(number) = value {
        return Some(written_number::<H>(number, intls));
    }
    timestamp_of(value).map(|timestamp| written_instant::<H>(timestamp, intls))
}

/// The [`Timestamp`] a Fluent value holds, if it holds one. The value is
/// asked through its trait object: a `Box` of it is `Any` itself, and would
/// never downcast to the type inside.
fn timestamp_of<'v>(value: &'v FluentValue<'_>) -> Option<&'v Timestamp> {
    match value {
        FluentValue::Custom(custom) => custom.as_ref().as_any().downcast_ref(),
        _ => None,
    }
}

fn written_number<H: Host>(number: &FluentNumber, intls: &IntlLangMemoizer) -> String {
    let options = NumberOptions::from(&number.options);
    intls
        .with_try_get::<NumberMemo<H::Numbers>, _, _>(options, |memo| {
            memo.0
                .as_ref()
                .and_then(|formatter| formatter.format(number.value))
        })
        .unwrap_or_else(|never| match never {})
        .unwrap_or_else(|| plain_number(number.value, options))
}

fn written_instant<H: Host>(timestamp: &Timestamp, intls: &IntlLangMemoizer) -> String {
    intls
        .with_try_get::<InstantMemo<H::Instants>, _, _>(timestamp.style, |memo| {
            memo.0
                .as_ref()
                .and_then(|formatter| formatter.format(timestamp.utc_ms, timestamp.zone.as_ref()))
        })
        .unwrap_or_else(|never| match never {})
        .unwrap_or_else(|| plain_instant(timestamp.utc_ms, timestamp.style))
}

/// `DATETIME($instant, dateStyle: ..., timeStyle: ...)`: the instant with the
/// style the options ask for, which replace those of an earlier call. A call
/// outside the vocabulary is an error, which Fluent shows as `DATETIME()`.
fn datetime<'a>(positional: &[FluentValue<'a>], named: &FluentArgs) -> FluentValue<'a> {
    let [value] = positional else {
        return FluentValue::Error;
    };
    let (Some(timestamp), Some(style)) = (timestamp_of(value), Style::from_options(named)) else {
        return FluentValue::Error;
    };
    FluentValue::Custom(Box::new(Timestamp {
        style,
        ..timestamp.clone()
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use fluent_bundle::FluentResource;

    use super::*;

    const WINTER: i64 = 1_794_144_645_000;

    fn tag(raw: &str) -> LanguageIdentifier {
        raw.parse().unwrap()
    }

    fn zone(name: &str) -> IanaZone {
        IanaZone::new(name).unwrap()
    }

    fn options(
        min_integer: u8,
        min_fraction: u8,
        max_fraction: Option<u8>,
        grouping: bool,
    ) -> NumberOptions {
        NumberOptions {
            min_integer,
            min_fraction,
            max_fraction,
            grouping,
        }
    }

    fn plain_options(min_fraction: u8) -> NumberOptions {
        options(1, min_fraction, None, true)
    }

    // The old plain instant, kept as the date-and-time form of the plain one.

    #[test]
    fn an_instant_is_written_in_utc_to_the_second() {
        assert_eq!(instant(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(instant(1_794_144_645_000), "2026-11-08 13:30:45 UTC");
        // The milliseconds are cut, not rounded up into the next second.
        assert_eq!(instant(1_794_144_645_999), "2026-11-08 13:30:45 UTC");
    }

    #[test]
    fn an_instant_before_the_epoch_counts_back_from_it() {
        assert_eq!(instant(-1), "1969-12-31 23:59:59 UTC");
        assert_eq!(instant(-86_400_000), "1969-12-31 00:00:00 UTC");
    }

    #[test]
    fn an_instant_no_calendar_date_can_show_is_written_as_its_count() {
        assert_eq!(instant(i64::MAX), "9223372036854775807");
        assert_eq!(instant(i64::MIN), "-9223372036854775808");
    }

    // The options a Fluent number carries, as a host sees them.

    fn fluent(
        min_integer: Option<usize>,
        min_fraction: Option<usize>,
        max_fraction: Option<usize>,
    ) -> FluentNumberOptions {
        FluentNumberOptions {
            minimum_integer_digits: min_integer,
            minimum_fraction_digits: min_fraction,
            maximum_fraction_digits: max_fraction,
            ..FluentNumberOptions::default()
        }
    }

    #[test]
    fn a_number_without_options_is_grouped_and_shows_every_digit() {
        assert_eq!(
            NumberOptions::from(&FluentNumberOptions::default()),
            options(1, 0, None, true)
        );
    }

    #[test]
    fn the_digit_options_a_number_asks_for_are_the_ones_a_host_gets() {
        assert_eq!(
            NumberOptions::from(&fluent(Some(4), Some(2), Some(3))),
            options(4, 2, Some(3), true)
        );
        assert_eq!(
            NumberOptions::from(&fluent(Some(1), Some(0), Some(0))),
            options(1, 0, Some(0), true)
        );
    }

    #[test]
    fn grouping_is_off_only_when_use_grouping_is_off() {
        let off = FluentNumberOptions {
            use_grouping: false,
            ..FluentNumberOptions::default()
        };
        assert!(!NumberOptions::from(&off).grouping);
        assert!(NumberOptions::from(&FluentNumberOptions::default()).grouping);
    }

    #[test]
    fn no_digit_count_goes_past_its_cap() {
        let huge = Some(usize::MAX);
        assert_eq!(
            NumberOptions::from(&fluent(huge, huge, huge)),
            options(
                MAX_INTEGER_DIGITS,
                MAX_FRACTION_DIGITS,
                Some(MAX_FRACTION_DIGITS),
                true
            )
        );
        // The caps themselves are allowed, one past them is not.
        assert_eq!(
            NumberOptions::from(&fluent(Some(21), Some(100), Some(100))),
            options(21, 100, Some(100), true)
        );
        assert_eq!(
            NumberOptions::from(&fluent(Some(22), Some(101), Some(101))),
            options(21, 100, Some(100), true)
        );
    }

    #[test]
    fn a_number_has_at_least_one_integer_digit() {
        assert_eq!(
            NumberOptions::from(&fluent(Some(0), None, None)).min_integer,
            1
        );
    }

    #[test]
    fn the_maximum_fraction_digits_win_over_a_larger_minimum() {
        assert_eq!(
            NumberOptions::from(&fluent(None, Some(2), Some(1))),
            options(1, 1, Some(1), true)
        );
        assert_eq!(
            NumberOptions::from(&fluent(None, Some(5), Some(0))),
            options(1, 0, Some(0), true)
        );
        // Equal is no conflict.
        assert_eq!(
            NumberOptions::from(&fluent(None, Some(2), Some(2))),
            options(1, 2, Some(2), true)
        );
    }

    // What a number looks like without a host.

    #[test]
    fn a_plain_number_is_the_one_fluent_writes() {
        for (value, min_fraction) in [
            (0.0, 0),
            (7.0, 0),
            (1234567.0, 0),
            (-1234.5, 0),
            (0.05, 0),
            (1.5, 3),
            (1.25, 1),
            (7.0, 2),
            (-3.0, 1),
            (1234567.891, 5),
        ] {
            let fluent_number = fluent_bundle::types::FluentNumber::new(
                value,
                fluent(
                    None,
                    (min_fraction > 0).then_some(min_fraction.into()),
                    None,
                ),
            );
            assert_eq!(
                plain_number(value, plain_options(min_fraction)),
                fluent_number.as_string(),
                "{value} with {min_fraction} fraction digits"
            );
        }
    }

    #[test]
    fn a_plain_number_is_padded_to_its_minimum_fraction_digits() {
        assert_eq!(plain_number(1.5, plain_options(3)), "1.500");
        assert_eq!(plain_number(7.0, plain_options(2)), "7.00");
        assert_eq!(plain_number(7.0, plain_options(1)), "7.0");
        assert_eq!(plain_number(1.25, plain_options(1)), "1.25");
        assert_eq!(plain_number(7.0, plain_options(0)), "7");
        assert_eq!(plain_number(-0.5, plain_options(2)), "-0.50");
    }

    #[test]
    fn a_plain_number_ignores_grouping_integer_digits_and_the_maximum() {
        assert_eq!(
            plain_number(1234567.891, options(9, 0, Some(1), true)),
            "1234567.891"
        );
        assert_eq!(
            plain_number(1234567.0, options(1, 0, None, false)),
            "1234567"
        );
    }

    #[test]
    fn a_plain_number_that_is_not_finite_is_written_without_digits_added() {
        assert_eq!(plain_number(f64::NAN, plain_options(2)), "NaN");
        assert_eq!(plain_number(f64::INFINITY, plain_options(2)), "inf");
        assert_eq!(plain_number(f64::NEG_INFINITY, plain_options(2)), "-inf");
    }

    #[test]
    fn a_plain_number_pads_no_further_than_the_cap() {
        let padded = plain_number(1.5, plain_options(MAX_FRACTION_DIGITS));
        assert_eq!(padded.len(), "1.".len() + usize::from(MAX_FRACTION_DIGITS));
        assert!(padded.starts_with("1.5000"));
        assert!(padded.ends_with('0'));
    }

    #[test]
    fn a_plain_instant_shows_the_parts_its_style_asks_for() {
        let medium = DateStyle::Medium;
        assert_eq!(
            plain_instant(WINTER, Style::DEFAULT),
            "2026-11-08 13:30:45 UTC"
        );
        assert_eq!(plain_instant(WINTER, Style::Date(medium)), "2026-11-08 UTC");
        assert_eq!(
            plain_instant(WINTER, Style::Time(TimeStyle::Short)),
            "13:30:45 UTC"
        );
        assert_eq!(plain_instant(WINTER, Style::ZoneName), "UTC");
    }

    #[test]
    fn a_plain_instant_is_the_same_in_every_length() {
        for date in [DateStyle::Short, DateStyle::Medium, DateStyle::Long] {
            assert_eq!(plain_instant(WINTER, Style::Date(date)), "2026-11-08 UTC");
            for time in [TimeStyle::Short, TimeStyle::Medium] {
                assert_eq!(
                    plain_instant(WINTER, Style::DateTime(date, time)),
                    "2026-11-08 13:30:45 UTC"
                );
            }
        }
        for time in [TimeStyle::Short, TimeStyle::Medium] {
            assert_eq!(plain_instant(WINTER, Style::Time(time)), "13:30:45 UTC");
        }
    }

    #[test]
    fn a_plain_instant_no_calendar_can_show_is_its_count_unless_only_the_zone_is_asked_for() {
        for style in [
            Style::DEFAULT,
            Style::Date(DateStyle::Long),
            Style::Time(TimeStyle::Medium),
        ] {
            assert_eq!(plain_instant(i64::MAX, style), "9223372036854775807");
            assert_eq!(plain_instant(i64::MIN, style), "-9223372036854775808");
        }
        assert_eq!(plain_instant(i64::MAX, Style::ZoneName), "UTC");
    }

    #[test]
    fn a_timestamp_is_a_medium_date_with_a_short_time_until_told_otherwise() {
        let timestamp = Timestamp::new(WINTER, Some(zone("Europe/Berlin")));
        assert_eq!(timestamp.utc_ms, WINTER);
        assert_eq!(timestamp.zone, Some(zone("Europe/Berlin")));
        assert_eq!(timestamp.style, Style::DEFAULT);
        assert_eq!(Timestamp::new(0, None).zone, None);
    }

    #[test]
    fn a_timestamp_with_no_host_to_write_it_is_written_plainly() {
        let timestamp = Timestamp::new(WINTER, Some(zone("Europe/Berlin")));
        let langid = tag("de");
        assert_eq!(
            timestamp.as_string(&intl_memoizer::IntlLangMemoizer::new(langid.clone())),
            "2026-11-08 13:30:45 UTC"
        );
        assert_eq!(
            timestamp.as_string_threadsafe(&IntlLangMemoizer::new(langid)),
            "2026-11-08 13:30:45 UTC"
        );
        let long = Timestamp {
            style: Style::Date(DateStyle::Long),
            ..timestamp
        };
        assert_eq!(
            long.as_string_threadsafe(&IntlLangMemoizer::new(tag("en-US"))),
            "2026-11-08 UTC"
        );
    }

    #[test]
    fn a_duplicated_timestamp_is_equal_to_the_original() {
        let timestamp = Timestamp::new(WINTER, Some(zone("Asia/Tokyo")));
        let copy = FluentValue::Custom(timestamp.duplicate());
        assert_eq!(copy, FluentValue::Custom(Box::new(timestamp)));
    }

    // The vocabulary of DATETIME.

    fn named(pairs: &[(&'static str, &'static str)]) -> FluentArgs<'static> {
        let mut args = FluentArgs::new();
        for (name, value) in pairs {
            args.set(*name, *value);
        }
        args
    }

    #[test]
    fn a_call_without_options_is_a_medium_date_with_a_short_time() {
        assert_eq!(
            Style::from_options(&FluentArgs::new()),
            Some(Style::DEFAULT)
        );
    }

    #[test]
    fn each_date_and_time_style_the_vocabulary_has_is_read() {
        let dates = [
            ("short", DateStyle::Short),
            ("medium", DateStyle::Medium),
            ("long", DateStyle::Long),
        ];
        let times = [("short", TimeStyle::Short), ("medium", TimeStyle::Medium)];
        for (word, date) in dates {
            assert_eq!(
                Style::from_options(&named(&[("dateStyle", word)])),
                Some(Style::Date(date)),
                "dateStyle {word}"
            );
            for (time_word, time) in times {
                assert_eq!(
                    Style::from_options(&named(&[("dateStyle", word), ("timeStyle", time_word)])),
                    Some(Style::DateTime(date, time)),
                    "dateStyle {word}, timeStyle {time_word}"
                );
            }
        }
        for (word, time) in times {
            assert_eq!(
                Style::from_options(&named(&[("timeStyle", word)])),
                Some(Style::Time(time)),
                "timeStyle {word}"
            );
        }
    }

    #[test]
    fn the_zone_name_stands_alone() {
        assert_eq!(
            Style::from_options(&named(&[("timeZoneName", "short")])),
            Some(Style::ZoneName)
        );
        for with in [
            ("dateStyle", "short"),
            ("timeStyle", "short"),
            ("dateStyle", "long"),
            ("timeStyle", "medium"),
        ] {
            assert_eq!(
                Style::from_options(&named(&[("timeZoneName", "short"), with])),
                None,
                "{with:?}"
            );
        }
        assert_eq!(
            Style::from_options(&named(&[
                ("timeZoneName", "short"),
                ("dateStyle", "short"),
                ("timeStyle", "short"),
            ])),
            None
        );
    }

    #[test]
    fn an_option_outside_the_vocabulary_is_refused() {
        for pairs in [
            &[("dateStyle", "full")][..],
            &[("timeStyle", "long")],
            &[("timeStyle", "full")],
            &[("timeZoneName", "long")],
            &[("timeZoneName", "shortOffset")],
            &[("dateStyle", "Short")],
            &[("dateStyle", "")],
            &[("weekday", "long")],
            &[("hour12", "true")],
            &[("dateStyle", "short"), ("hour12", "true")],
            &[("timeZone", "UTC")],
        ] {
            assert_eq!(Style::from_options(&named(pairs)), None, "{pairs:?}");
        }
    }

    #[test]
    fn an_option_whose_value_is_not_text_is_refused() {
        let mut number = FluentArgs::new();
        number.set("dateStyle", 3);
        assert_eq!(Style::from_options(&number), None);
        let mut absent = FluentArgs::new();
        absent.set("timeStyle", FluentValue::None);
        assert_eq!(Style::from_options(&absent), None);
    }

    // The glue: a host that writes what it was asked shows what the bundle
    // handed over.

    /// What the hosts below were asked to build, a line each, so a test can
    /// count the formatters one locale got.
    static BUILT: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn built(prefix: &str) -> usize {
        BUILT
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }

    fn describe(options: NumberOptions) -> String {
        let max = options
            .max_fraction
            .map_or_else(|| "-".to_owned(), |max| max.to_string());
        format!(
            "int>={} frac={}..{max} group={}",
            options.min_integer, options.min_fraction, options.grouping
        )
    }

    struct Echo;

    struct EchoNumbers {
        locale: String,
        options: NumberOptions,
    }

    struct EchoInstants {
        locale: String,
        style: Style,
    }

    impl Host for Echo {
        type Numbers = EchoNumbers;
        type Instants = EchoInstants;
    }

    impl NumberFormatter for EchoNumbers {
        fn new(locale: &LanguageIdentifier, options: NumberOptions) -> Option<Self> {
            BUILT
                .lock()
                .unwrap()
                .push(format!("number {locale} {}", describe(options)));
            Some(Self {
                locale: locale.to_string(),
                options,
            })
        }

        fn format(&self, value: f64) -> Option<String> {
            Some(format!(
                "[{} {value} {}]",
                self.locale,
                describe(self.options)
            ))
        }
    }

    impl InstantFormatter for EchoInstants {
        fn new(locale: &LanguageIdentifier, style: Style) -> Option<Self> {
            BUILT
                .lock()
                .unwrap()
                .push(format!("instant {locale} {style:?}"));
            Some(Self {
                locale: locale.to_string(),
                style,
            })
        }

        fn format(&self, utc_ms: i64, zone: Option<&IanaZone>) -> Option<String> {
            Some(format!(
                "[{} {:?} {utc_ms} {}]",
                self.locale,
                self.style,
                zone.map_or("-", IanaZone::as_str)
            ))
        }
    }

    /// A host whose formatters exist and write nothing.
    struct Mute;

    struct MuteNumbers;

    struct MuteInstants;

    impl Host for Mute {
        type Numbers = MuteNumbers;
        type Instants = MuteInstants;
    }

    impl NumberFormatter for MuteNumbers {
        fn new(_: &LanguageIdentifier, _: NumberOptions) -> Option<Self> {
            Some(Self)
        }

        fn format(&self, _: f64) -> Option<String> {
            None
        }
    }

    impl InstantFormatter for MuteInstants {
        fn new(_: &LanguageIdentifier, _: Style) -> Option<Self> {
            Some(Self)
        }

        fn format(&self, _: i64, _: Option<&IanaZone>) -> Option<String> {
            None
        }
    }

    const FTL: &str = "\
plain = { $n }
fraction = { NUMBER($n, minimumFractionDigits: 2) }
every = { NUMBER($n, minimumIntegerDigits: 4, minimumFractionDigits: 2, maximumFractionDigits: 3, useGrouping: \"false\") }
literal = { NUMBER(1.25, maximumFractionDigits: 1) }
text = { $s }
bare = { $when }
styled = { DATETIME($when, dateStyle: \"long\", timeStyle: \"medium\") }
dated = { DATETIME($when, dateStyle: \"short\") }
timed = { DATETIME($when, timeStyle: \"short\") }
named = { DATETIME($when, timeZoneName: \"short\") }
default = { DATETIME($when) }
fullstyle = { DATETIME($when, dateStyle: \"full\") }
zoned = { DATETIME($when, timeZoneName: \"short\", dateStyle: \"short\") }
extra = { DATETIME($when, \"more\") }
number-in = { DATETIME($n) }
text-in = { DATETIME($s) }
none = { DATETIME() }
unknown = { NOPE($n) }
";

    fn bundle<H: Host>(locale: &str) -> FluentBundle<FluentResource> {
        bundle_from::<H>(locale, FTL)
    }

    fn bundle_from<H: Host>(locale: &str, ftl: &str) -> FluentBundle<FluentResource> {
        let mut bundle = FluentBundle::new_concurrent(vec![tag(locale)]);
        bundle.set_use_isolating(false);
        bundle.add_builtins().unwrap();
        install::<H, _>(&mut bundle);
        bundle
            .add_resource(FluentResource::try_new(ftl.to_owned()).unwrap())
            .unwrap();
        bundle
    }

    fn render(bundle: &FluentBundle<FluentResource>, id: &str, args: &FluentArgs) -> String {
        let message = bundle.get_message(id).unwrap();
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(message.value().unwrap(), Some(args), &mut errors)
            .into_owned();
        assert!(errors.is_empty(), "{id}: {errors:?}");
        text
    }

    fn number(n: f64) -> FluentArgs<'static> {
        let mut args = FluentArgs::new();
        args.set("n", n);
        args
    }

    fn when(utc_ms: i64, zone: Option<&str>) -> FluentArgs<'static> {
        let mut args = FluentArgs::new();
        args.set(
            "when",
            FluentValue::Custom(Box::new(Timestamp::new(utc_ms, zone.map(self::zone)))),
        );
        args
    }

    #[test]
    fn a_written_number_goes_to_the_host_with_the_locale_and_the_options_of_its_call() {
        let bundle = bundle::<Echo>("de");
        assert_eq!(
            render(&bundle, "plain", &number(1234.5)),
            "[de 1234.5 int>=1 frac=0..- group=true]"
        );
        assert_eq!(
            render(&bundle, "fraction", &number(1234.5)),
            "[de 1234.5 int>=1 frac=2..- group=true]"
        );
        assert_eq!(
            render(&bundle, "every", &number(1234.5)),
            "[de 1234.5 int>=4 frac=2..3 group=false]"
        );
    }

    #[test]
    fn a_literal_number_hands_the_host_the_options_the_author_asked_for() {
        // Its own decimals make a minimum of two; the author asked for one at most.
        let bundle = bundle::<Echo>("de");
        assert_eq!(
            render(&bundle, "literal", &FluentArgs::new()),
            "[de 1.25 int>=1 frac=1..1 group=true]"
        );
    }

    #[test]
    fn text_is_never_given_to_the_host() {
        let bundle = bundle::<Echo>("de");
        let mut args = FluentArgs::new();
        args.set("s", "1234.5");
        assert_eq!(render(&bundle, "text", &args), "1234.5");
    }

    #[test]
    fn a_formatter_is_built_once_for_a_locale_and_the_options_it_is_asked_with() {
        let bundle = bundle::<Echo>("fi");
        for n in [1.0, 2.0, 3.0] {
            render(&bundle, "plain", &number(n));
            render(&bundle, "fraction", &number(n));
        }
        assert_eq!(built("number fi int>=1 frac=0..- group=true"), 1);
        assert_eq!(built("number fi int>=1 frac=2..- group=true"), 1);
        assert_eq!(built("number fi"), 2);
        // Another bundle of the locale has its own.
        let other = self::bundle::<Echo>("fi");
        render(&other, "plain", &number(1.0));
        assert_eq!(built("number fi"), 3);
    }

    #[test]
    fn an_instant_goes_to_the_host_with_its_zone_and_the_style_of_its_call() {
        let bundle = bundle::<Echo>("de");
        let at = when(WINTER, Some("Europe/Berlin"));
        assert_eq!(
            render(&bundle, "styled", &at),
            format!("[de DateTime(Long, Medium) {WINTER} Europe/Berlin]")
        );
        assert_eq!(
            render(&bundle, "dated", &at),
            format!("[de Date(Short) {WINTER} Europe/Berlin]")
        );
        assert_eq!(
            render(&bundle, "timed", &at),
            format!("[de Time(Short) {WINTER} Europe/Berlin]")
        );
        assert_eq!(
            render(&bundle, "named", &at),
            format!("[de ZoneName {WINTER} Europe/Berlin]")
        );
    }

    #[test]
    fn a_call_without_options_and_a_bare_instant_are_a_medium_date_with_a_short_time() {
        let bundle = bundle::<Echo>("ja");
        let at = when(WINTER, None);
        let expected = format!("[ja DateTime(Medium, Short) {WINTER} -]");
        assert_eq!(render(&bundle, "default", &at), expected);
        assert_eq!(render(&bundle, "bare", &at), expected);
    }

    #[test]
    fn an_instant_formatter_is_built_once_for_a_locale_and_a_style() {
        let bundle = bundle::<Echo>("sv");
        let at = when(WINTER, None);
        for _ in 0..3 {
            render(&bundle, "bare", &at);
            render(&bundle, "dated", &at);
        }
        assert_eq!(built("instant sv DateTime(Medium, Short)"), 1);
        assert_eq!(built("instant sv Date(Short)"), 1);
        assert_eq!(built("instant sv"), 2);
    }

    #[test]
    fn a_bad_datetime_call_is_shown_as_the_function_with_no_arguments() {
        let bundle = bundle::<Echo>("de");
        let at = when(WINTER, None);
        // The good call beside them is written by the host.
        assert!(render(&bundle, "default", &at).starts_with("[de "));
        for id in ["fullstyle", "zoned", "extra"] {
            assert_eq!(render(&bundle, id, &at), "DATETIME()", "{id}");
        }
        assert_eq!(render(&bundle, "none", &FluentArgs::new()), "DATETIME()");
        assert_eq!(render(&bundle, "number-in", &number(1.0)), "DATETIME()");
        let mut args = FluentArgs::new();
        args.set("s", "2026-11-08");
        assert_eq!(render(&bundle, "text-in", &args), "DATETIME()");
    }

    #[test]
    fn a_host_with_no_formatter_for_the_locale_leaves_the_plain_text() {
        let bundle = bundle::<NoHost>("de");
        assert_eq!(render(&bundle, "plain", &number(1234.5)), "1234.5");
        assert_eq!(render(&bundle, "fraction", &number(1234.5)), "1234.50");
        let at = when(WINTER, Some("Europe/Berlin"));
        assert_eq!(render(&bundle, "bare", &at), "2026-11-08 13:30:45 UTC");
        assert_eq!(render(&bundle, "styled", &at), "2026-11-08 13:30:45 UTC");
        assert_eq!(render(&bundle, "dated", &at), "2026-11-08 UTC");
        assert_eq!(render(&bundle, "timed", &at), "13:30:45 UTC");
        assert_eq!(render(&bundle, "named", &at), "UTC");
    }

    #[test]
    fn a_host_that_cannot_write_a_value_leaves_the_plain_text() {
        let bundle = bundle::<Mute>("de");
        assert_eq!(render(&bundle, "fraction", &number(1234.5)), "1234.50");
        let at = when(WINTER, None);
        assert_eq!(render(&bundle, "bare", &at), "2026-11-08 13:30:45 UTC");
        assert_eq!(render(&bundle, "dated", &at), "2026-11-08 UTC");
        // A number no host can write is Fluent's own text, with no digits added.
        assert_eq!(render(&bundle, "fraction", &number(f64::NAN)), "NaN");
        assert_eq!(render(&bundle, "plain", &number(f64::NEG_INFINITY)), "-inf");
    }

    #[test]
    fn the_options_of_a_datetime_call_replace_the_style_of_an_earlier_one() {
        let ftl = "\
nested = { DATETIME(DATETIME($when, dateStyle: \"long\"), timeStyle: \"short\") }
again = { DATETIME(DATETIME($when, dateStyle: \"long\")) }
";
        let bundle = bundle_from::<Echo>("de", ftl);
        let at = when(WINTER, None);
        assert_eq!(
            render(&bundle, "nested", &at),
            format!("[de Time(Short) {WINTER} -]")
        );
        assert_eq!(
            render(&bundle, "again", &at),
            format!("[de DateTime(Medium, Short) {WINTER} -]")
        );
    }

    #[test]
    fn a_number_is_capped_before_a_host_or_the_plain_text_sees_it() {
        // A catalog's own literal asks for 2^52 digits of each kind: a named
        // argument is a literal, never a variable.
        let ftl = "huge = { NUMBER($n, minimumFractionDigits: 4503599627370496, minimumIntegerDigits: 4503599627370496, maximumFractionDigits: 4503599627370496) }\n";
        let args = number(1.5);
        let echoed = bundle_from::<Echo>("de", ftl);
        assert_eq!(
            render(&echoed, "huge", &args),
            "[de 1.5 int>=21 frac=100..100 group=true]"
        );
        let plain = bundle_from::<NoHost>("de", ftl);
        let shown = render(&plain, "huge", &args);
        assert_eq!(shown.len(), "1.".len() + usize::from(MAX_FRACTION_DIGITS));
    }

    #[test]
    fn a_host_that_builds_nothing_declines_everything() {
        assert!(<Nothing as NumberFormatter>::new(&tag("de"), plain_options(0)).is_none());
        assert!(<Nothing as InstantFormatter>::new(&tag("de"), Style::DEFAULT).is_none());
        assert_eq!(NumberFormatter::format(&Nothing, 1.5), None);
        assert_eq!(InstantFormatter::format(&Nothing, WINTER, None), None);
    }
}
