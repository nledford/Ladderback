//! Turning typed arguments into what a message can use: capped text, a size
//! in the right unit, a duration in whole parts. The words and the number
//! formats come from the catalog and the host formatter, not from here.

use std::borrow::Cow;

/// The most arguments one rendering reads. A peer can send any number; a
/// message uses a handful.
pub(crate) const MAX_ARGS: usize = 32;

/// The longest text one argument or detail keeps, in bytes, ellipsis
/// included. A path is at most 4 KiB, and rustic's longest messages are
/// shorter than this.
pub(crate) const MAX_TEXT: usize = 16 * 1024;

/// `text`, cut at a character boundary and ended with `…` if it is longer
/// than [`MAX_TEXT`].
pub(crate) fn clamp_text(text: &str) -> Cow<'_, str> {
    if text.len() <= MAX_TEXT {
        return Cow::Borrowed(text);
    }
    // The last character boundary that leaves room for the ellipsis.
    let room = MAX_TEXT - '…'.len_utf8();
    let end = (0..=room)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or_default();
    let mut clamped = String::with_capacity(MAX_TEXT);
    clamped.push_str(text.get(..end).unwrap_or_default());
    clamped.push('…');
    Cow::Owned(clamped)
}

/// The IEC unit a byte count is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeUnit {
    B,
    KiB,
    MiB,
    GiB,
    TiB,
    PiB,
}

impl SizeUnit {
    /// The key the catalog's `size` message selects its label by.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::B => "b",
            Self::KiB => "kib",
            Self::MiB => "mib",
            Self::GiB => "gib",
            Self::TiB => "tib",
            Self::PiB => "pib",
        }
    }
}

/// The largest unit that keeps the value at 1 or more, and the value in it,
/// rounded to one decimal. A value that rounds up to 1024 moves to the next
/// unit, so 1,048,575 bytes read as 1 MiB, not 1,024 KiB. Nothing is larger
/// than a PiB: a `u64` tops out at 16,384 of them.
pub(crate) fn size_parts(bytes: u64) -> (f64, SizeUnit) {
    const UNITS: [SizeUnit; 6] = [
        SizeUnit::B,
        SizeUnit::KiB,
        SizeUnit::MiB,
        SizeUnit::GiB,
        SizeUnit::TiB,
        SizeUnit::PiB,
    ];
    let tenth = |value: f64| (value * 10.0).round() / 10.0;
    let mut value = bytes as f64;
    let mut index = 0;
    // Move up while the value, as it will be shown, reaches 1024.
    while index + 1 < UNITS.len() && tenth(value) >= 1024.0 {
        value /= 1024.0;
        index += 1;
    }
    let unit = UNITS.get(index).copied().unwrap_or(SizeUnit::PiB);
    (if index == 0 { value } else { tenth(value) }, unit)
}

/// A duration split into the parts its message shows: the two largest
/// units that apply, smaller ones dropped, never rounded up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurationParts {
    Milliseconds { milliseconds: u64 },
    Seconds { seconds: u64 },
    MinutesSeconds { minutes: u64, seconds: u64 },
    HoursMinutes { hours: u64, minutes: u64 },
    DaysHours { days: u64, hours: u64 },
}

pub(crate) fn duration_parts(milliseconds: u64) -> DurationParts {
    const SECOND: u64 = 1_000;
    const MINUTE: u64 = 60 * SECOND;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    match milliseconds {
        0..SECOND => DurationParts::Milliseconds { milliseconds },
        SECOND..MINUTE => DurationParts::Seconds {
            seconds: milliseconds / SECOND,
        },
        MINUTE..HOUR => DurationParts::MinutesSeconds {
            minutes: milliseconds / MINUTE,
            seconds: milliseconds % MINUTE / SECOND,
        },
        HOUR..DAY => DurationParts::HoursMinutes {
            hours: milliseconds / HOUR,
            minutes: milliseconds % HOUR / MINUTE,
        },
        DAY.. => DurationParts::DaysHours {
            days: milliseconds / DAY,
            hours: milliseconds % DAY / HOUR,
        },
    }
}

impl DurationParts {
    /// The key the catalog's `duration` message selects its arrangement by.
    pub(crate) const fn form(self) -> &'static str {
        match self {
            Self::Milliseconds { .. } => "ms",
            Self::Seconds { .. } => "s",
            Self::MinutesSeconds { .. } => "min-s",
            Self::HoursMinutes { .. } => "h-min",
            Self::DaysHours { .. } => "d-h",
        }
    }

    /// The parts, under the names the `duration` message reads them by.
    pub(crate) fn parts(self) -> Vec<(&'static str, u64)> {
        match self {
            Self::Milliseconds { milliseconds } => vec![("milliseconds", milliseconds)],
            Self::Seconds { seconds } => vec![("seconds", seconds)],
            Self::MinutesSeconds { minutes, seconds } => {
                vec![("minutes", minutes), ("seconds", seconds)]
            }
            Self::HoursMinutes { hours, minutes } => vec![("hours", hours), ("minutes", minutes)],
            Self::DaysHours { days, hours } => vec![("days", days), ("hours", hours)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_within_the_cap_is_kept_as_it_is() {
        assert!(matches!(clamp_text("/home/demo/docs"), Cow::Borrowed(_)));
        let exactly = "a".repeat(MAX_TEXT);
        assert_eq!(clamp_text(&exactly), exactly);
    }

    #[test]
    fn text_over_the_cap_is_cut_and_marked_within_the_cap() {
        let long = "a".repeat(MAX_TEXT + 1);
        let clamped = clamp_text(&long);
        assert_eq!(clamped.len(), MAX_TEXT);
        assert!(clamped.ends_with('…'));
        assert!(clamped.starts_with("aaaa"));
    }

    #[test]
    fn text_is_cut_between_characters_never_inside_one() {
        // Two-byte characters, so the byte cap falls inside one.
        let long = "é".repeat(MAX_TEXT);
        let clamped = clamp_text(&long);
        assert!(clamped.len() <= MAX_TEXT);
        assert!(clamped.ends_with("é…"));
    }

    #[test]
    fn the_cap_is_sixteen_kibibytes() {
        // Spelled out in bytes, so the test can't follow the constant.
        let at_cap = "a".repeat(16_384);
        assert!(matches!(clamp_text(&at_cap), Cow::Borrowed(_)));
        let over = "a".repeat(16_385);
        let clamped = clamp_text(&over);
        assert_eq!(clamped.len(), 16_384);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn sizes_below_a_kibibyte_are_bytes() {
        assert_eq!(size_parts(0), (0.0, SizeUnit::B));
        assert_eq!(size_parts(1), (1.0, SizeUnit::B));
        assert_eq!(size_parts(1023), (1023.0, SizeUnit::B));
    }

    #[test]
    fn sizes_use_the_largest_unit_that_keeps_the_value_at_one_or_more() {
        assert_eq!(size_parts(1024), (1.0, SizeUnit::KiB));
        assert_eq!(size_parts(1536), (1.5, SizeUnit::KiB));
        assert_eq!(size_parts(1 << 20), (1.0, SizeUnit::MiB));
        assert_eq!(size_parts(5_905_580_032), (5.5, SizeUnit::GiB));
        assert_eq!(size_parts(3 << 40), (3.0, SizeUnit::TiB));
        assert_eq!(size_parts(1 << 50), (1.0, SizeUnit::PiB));
    }

    #[test]
    fn sizes_round_to_one_decimal() {
        // 976.5625 KiB.
        assert_eq!(size_parts(1_000_000), (976.6, SizeUnit::KiB));
    }

    #[test]
    fn a_size_that_rounds_up_to_the_next_unit_is_shown_in_it() {
        // 1023.949… KiB stays; 1023.950… KiB would print as 1,024 KiB.
        assert_eq!(size_parts(1_048_524), (1023.9, SizeUnit::KiB));
        assert_eq!(size_parts(1_048_525), (1.0, SizeUnit::MiB));
        assert_eq!(size_parts(1_048_575), (1.0, SizeUnit::MiB));
    }

    #[test]
    fn the_largest_size_stays_in_pebibytes() {
        assert_eq!(size_parts(u64::MAX), (16384.0, SizeUnit::PiB));
    }

    #[test]
    fn a_duration_shows_the_two_largest_units_that_apply() {
        use DurationParts::*;
        assert_eq!(duration_parts(0), Milliseconds { milliseconds: 0 });
        assert_eq!(duration_parts(999), Milliseconds { milliseconds: 999 });
        assert_eq!(duration_parts(1_000), Seconds { seconds: 1 });
        assert_eq!(duration_parts(59_999), Seconds { seconds: 59 });
        assert_eq!(
            duration_parts(60_000),
            MinutesSeconds {
                minutes: 1,
                seconds: 0
            }
        );
        assert_eq!(
            duration_parts(3_599_999),
            MinutesSeconds {
                minutes: 59,
                seconds: 59
            }
        );
        assert_eq!(
            duration_parts(3_600_000),
            HoursMinutes {
                hours: 1,
                minutes: 0
            }
        );
        assert_eq!(
            duration_parts(86_399_999),
            HoursMinutes {
                hours: 23,
                minutes: 59
            }
        );
        assert_eq!(duration_parts(90_000_000), DaysHours { days: 1, hours: 1 });
    }

    #[test]
    fn the_longest_duration_is_split_without_overflow() {
        assert_eq!(
            duration_parts(u64::MAX),
            DurationParts::DaysHours {
                days: 213_503_982_334,
                hours: 14
            }
        );
    }
}
