//! Lengths of time in a configuration, written the way `jiff` reads them:
//! `5m`, `90 seconds`, `1h 30m` (technical design §6.1).

use std::str::FromStr;

use jiff::{SignedDuration, Span, SpanRelativeTo};

use super::{InvalidValue, ValueKind};

/// The longest length of time a jitter may be: 366 days. A longer one is a
/// typo, and the arithmetic on instants stays far from their range. (A retry
/// back-off is held to an hour, by [`RetryPolicy`](super::RetryPolicy).)
pub const MAX_DURATION: SignedDuration = SignedDuration::from_hours(366 * 24);

/// Reads a length of time such as a jitter or a retry back-off. Days and
/// weeks count as 24 hours and seven days; a month or a year has no fixed
/// length, so it is refused, and so is a time below zero or above
/// [`MAX_DURATION`].
pub fn parse_duration(text: &str) -> Result<SignedDuration, InvalidValue> {
    length_of(text)
        .and_then(|duration| within_limits(duration).ok())
        .ok_or_else(|| InvalidValue::new(ValueKind::Duration, text))
}

/// The length of time `text` writes, with no limit but that it isn't below
/// zero; `None` for text that is no length of time, or a month or a year,
/// which have no fixed length.
pub(crate) fn length_of(text: &str) -> Option<SignedDuration> {
    let span = Span::from_str(text).ok()?;
    if span.is_negative() {
        return None;
    }
    span.to_duration(SpanRelativeTo::days_are_24_hours()).ok()
}

/// `duration` if it is a length of time a jitter may be: not below zero and
/// at most [`MAX_DURATION`]. Whatever builds a jitter, text or code, goes
/// through this.
pub(crate) fn within_limits(duration: SignedDuration) -> Result<SignedDuration, InvalidValue> {
    if duration < SignedDuration::ZERO || duration > MAX_DURATION {
        Err(InvalidValue::new(
            ValueKind::Duration,
            format_duration(duration),
        ))
    } else {
        Ok(duration)
    }
}

/// The text [`parse_duration`] reads back as `duration`: `jiff`'s compact
/// form, such as `1h 30m`.
pub fn format_duration(duration: SignedDuration) -> String {
    format!("{duration:#}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_written_the_way_jiff_reads_one() {
        for (text, seconds) in [
            ("5m", 300),
            ("10 mins", 600),
            ("90 seconds", 90),
            ("1h 30m", 5_400),
            ("2 days", 172_800),
            ("1 week", 604_800),
            ("PT5M", 300),
            ("0s", 0),
        ] {
            assert_eq!(
                parse_duration(text).unwrap(),
                SignedDuration::from_secs(seconds),
                "{text}"
            );
        }
    }

    #[test]
    fn a_text_that_is_not_a_plain_length_of_time_is_refused() {
        for text in [
            "",
            "soon",
            "five minutes",
            "5",
            "5 parsecs",
            "-5m",
            "5m ago",
            "1 month",
            "1y",
            "P1M",
        ] {
            assert_eq!(
                parse_duration(text),
                Err(InvalidValue::new(ValueKind::Duration, text)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_length_of_time_is_at_most_366_days() {
        assert_eq!(MAX_DURATION, SignedDuration::from_hours(8_784));
        for text in ["366 days", "8784h", "527040 minutes"] {
            assert_eq!(parse_duration(text).unwrap(), MAX_DURATION, "{text}");
        }
        for text in ["366 days 1s", "8784h 1s", "367 days", "1000000 days"] {
            assert_eq!(
                parse_duration(text),
                Err(InvalidValue::new(ValueKind::Duration, text)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_length_built_in_code_is_held_to_the_same_limits() {
        let second = SignedDuration::from_secs(1);
        for fine in [SignedDuration::ZERO, second, MAX_DURATION] {
            assert_eq!(within_limits(fine), Ok(fine));
        }
        for refused in [
            -second,
            MAX_DURATION + second,
            SignedDuration::MIN,
            SignedDuration::MAX,
        ] {
            assert_eq!(
                within_limits(refused),
                Err(InvalidValue::new(
                    ValueKind::Duration,
                    format_duration(refused)
                ))
            );
        }
    }

    #[test]
    fn a_duration_is_written_compactly() {
        for (duration, text) in [
            (SignedDuration::ZERO, "0s"),
            (SignedDuration::from_secs(59), "59s"),
            (SignedDuration::from_secs(300), "5m"),
            (SignedDuration::from_secs(3_661), "1h 1m 1s"),
            (SignedDuration::from_secs(5_400), "1h 30m"),
            (SignedDuration::from_secs(90_061), "25h 1m 1s"),
            (SignedDuration::from_millis(1_500), "1s 500ms"),
        ] {
            assert_eq!(format_duration(duration), text);
        }
    }

    #[test]
    fn a_written_duration_reads_back_the_same() {
        for duration in [
            SignedDuration::from_secs(0),
            SignedDuration::from_secs(59),
            SignedDuration::from_secs(3_661),
            SignedDuration::from_secs(90_061),
            SignedDuration::from_millis(1_500),
        ] {
            let text = format_duration(duration);
            assert_eq!(parse_duration(&text).unwrap(), duration, "{text}");
        }
    }
}
