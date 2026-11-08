//! The host's formatting of numbers and instants: the one place a locale-aware
//! formatter plugs in (technical design §8.4).
//!
//! For now both are plain. Numbers keep Fluent's own output, because no
//! number formatter is installed on the bundles (`localizer::new_bundle` is
//! where one goes, through `FluentBundle::set_formatter`), and an instant is
//! written in UTC, with the zone's name, by [`instant`].

/// An instant, as written in a message: `2026-11-08 13:30:45 UTC`. An
/// instant outside the years a calendar date can show (±9999) is written as
/// its millisecond count, so no input can make this fail.
pub(crate) fn instant(utc_ms: i64) -> String {
    jiff::Timestamp::from_millisecond(utc_ms).map_or_else(
        |_| utc_ms.to_string(),
        |instant| instant.strftime("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
