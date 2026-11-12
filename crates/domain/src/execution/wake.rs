//! Wake detection (technical design §5; the domain service `WakeDetector` of
//! §3.1). Every [`WAKE_CHECK_EVERY`] the agent samples wall-clock time and
//! monotonic time, which stops while the machine is asleep. When wall-clock
//! time ran more than [`WAKE_GAP`] ahead of monotonic time between two
//! samples, the machine slept, or its clock was set forward, and a run it
//! missed meanwhile can catch up. Nothing here reads a clock: the samples
//! come from the `Clock` port.

use crate::{SignedDuration, Timestamp};

/// How often the agent samples its clocks to notice a wake.
pub const WAKE_CHECK_EVERY: SignedDuration = SignedDuration::from_secs(30);

/// How far wall-clock time must run ahead of monotonic time between two
/// samples for the gap to be a wake: more than this. A sample taken late
/// because the machine was busy shows no gap, since both clocks ran.
pub const WAKE_GAP: SignedDuration = SignedDuration::from_mins(2);

/// One reading of both clocks, taken together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSample {
    /// Wall-clock time: it runs on while the machine sleeps.
    pub wall: Timestamp,
    /// Monotonic time, from any start: it stands still while the machine
    /// sleeps, and never goes back.
    pub monotonic: SignedDuration,
}

/// The machine woke: it slept, or its clock was set forward, for `slept`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wake {
    /// Wall-clock time at the sample that noticed the wake.
    pub at: Timestamp,
    /// How far wall-clock time ran ahead of monotonic time.
    pub slept: SignedDuration,
}

/// Compares each sample of the clocks with the one before it.
#[derive(Clone, Debug)]
pub struct WakeDetector {
    last: ClockSample,
}

impl WakeDetector {
    pub fn new(first: ClockSample) -> Self {
        Self { last: first }
    }

    /// Takes the next sample, and returns the wake it shows: wall-clock time
    /// ran more than [`WAKE_GAP`] ahead of monotonic time since the sample
    /// before. A clock set back is no wake: no run was missed.
    pub fn sample(&mut self, now: ClockSample) -> Option<Wake> {
        let wall = now.wall.duration_since(self.last.wall);
        let monotonic = now.monotonic.saturating_sub(self.last.monotonic);
        self.last = now;
        let slept = wall.saturating_sub(monotonic);
        (slept > WAKE_GAP).then_some(Wake {
            at: now.wall,
            slept,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(wall_secs: i64, monotonic_secs: i64) -> ClockSample {
        ClockSample {
            wall: Timestamp::from_second(1_800_000_000 + wall_secs).unwrap(),
            monotonic: SignedDuration::from_secs(monotonic_secs),
        }
    }

    #[test]
    fn a_sample_after_a_sleep_is_a_wake_of_that_long() {
        let mut detector = WakeDetector::new(at(0, 100));
        let woke = detector.sample(at(3 * 3600 + 30, 130));
        assert_eq!(
            woke,
            Some(Wake {
                at: at(3 * 3600 + 30, 0).wall,
                slept: SignedDuration::from_hours(3),
            })
        );
    }

    #[test]
    fn a_gap_of_exactly_two_minutes_is_no_wake_and_one_second_more_is() {
        let mut detector = WakeDetector::new(at(0, 0));
        assert_eq!(detector.sample(at(150, 30)), None, "a gap of 2 min");
        let woke = detector.sample(at(150 + 151, 60));
        assert_eq!(
            woke.map(|wake| wake.slept),
            Some(SignedDuration::from_secs(121))
        );
    }

    #[test]
    fn a_late_sample_on_a_busy_machine_is_no_wake() {
        let mut detector = WakeDetector::new(at(0, 0));
        assert_eq!(detector.sample(at(600, 600)), None);
    }

    #[test]
    fn a_clock_set_back_is_no_wake() {
        let mut detector = WakeDetector::new(at(0, 0));
        assert_eq!(detector.sample(at(-3600, 30)), None);
    }

    #[test]
    fn each_sample_is_compared_with_the_one_just_before() {
        let mut detector = WakeDetector::new(at(0, 0));
        assert!(detector.sample(at(3600, 30)).is_some());
        // The gap before the last sample is not counted again.
        assert_eq!(detector.sample(at(3630, 60)), None);
        // Nor is a gap spread over many samples, each within the bound.
        for n in 1..=10 {
            assert_eq!(detector.sample(at(3630 + n * 90, 60 + n * 30)), None);
        }
    }

    #[test]
    fn clocks_at_the_ends_of_their_ranges_never_overflow() {
        let mut detector = WakeDetector::new(ClockSample {
            wall: Timestamp::MIN,
            monotonic: SignedDuration::MAX,
        });
        let woke = detector.sample(ClockSample {
            wall: Timestamp::MAX,
            monotonic: SignedDuration::MIN,
        });
        assert!(woke.is_some_and(|wake| wake.slept > WAKE_GAP));
        assert_eq!(
            detector.sample(ClockSample {
                wall: Timestamp::MIN,
                monotonic: SignedDuration::MAX,
            }),
            None
        );
    }
}
