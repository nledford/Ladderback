//! The long-backup guard's first layer (invariant I3, technical design §7.3):
//! a backup must write its snapshot before a prune can delete packs that the
//! index it read still points to.

use crate::{SignedDuration, Timestamp};

/// rustic's default `keep-delete`. An agent that hasn't been told the
/// repository's actual value assumes it, which can only make deadlines earlier.
pub const RUSTIC_KEEP_DELETE: SignedDuration = SignedDuration::from_hours(23);

/// How long a pruned pack stays marked before a later prune may delete it.
/// Never shorter than rustic's default, so a backup that read the index
/// before the marking prune has that long to finish (§7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepDelete(SignedDuration);

impl KeepDelete {
    pub const MIN: Self = Self(RUSTIC_KEEP_DELETE);

    pub fn new(value: SignedDuration) -> Result<Self, KeepDeleteTooShort> {
        if value < Self::MIN.0 {
            Err(KeepDeleteTooShort { value })
        } else {
            Ok(Self(value))
        }
    }

    pub fn get(self) -> SignedDuration {
        self.0
    }
}

/// A `keep-delete` shorter than [`KeepDelete::MIN`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepDeleteTooShort {
    pub value: SignedDuration,
}

impl std::fmt::Display for KeepDeleteTooShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for KeepDeleteTooShort {}

/// The fixed part of the prune margin; the rest is the longest prune recorded
/// for the repository.
pub const PRUNE_MARGIN_BASE: SignedDuration = SignedDuration::from_hours(1);

/// How long a backup may run after it starts reading the index:
/// `keep_delete − prune_margin`, where `prune_margin` is 1 h plus the longest
/// prune recorded for the repository. May be zero or negative, and then the
/// backup can't finish in time: the safe direction.
pub fn backup_budget(
    keep_delete: Option<KeepDelete>,
    longest_prune: Option<SignedDuration>,
) -> SignedDuration {
    let margin = PRUNE_MARGIN_BASE.saturating_add(longest_prune.unwrap_or(SignedDuration::ZERO));
    keep_delete
        .unwrap_or(KeepDelete::MIN)
        .get()
        .saturating_sub(margin)
}

/// The latest wall-clock time a backup may write its snapshot. Time asleep
/// counts, because a prune elsewhere doesn't wait for a sleeping laptop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(Timestamp);

impl Deadline {
    pub fn after_index_read(index_read_at: Timestamp, budget: SignedDuration) -> Self {
        Self(
            index_read_at
                .checked_add(budget)
                .unwrap_or(if budget.is_negative() {
                    Timestamp::MIN
                } else {
                    Timestamp::MAX
                }),
        )
    }

    pub fn at(self) -> Timestamp {
        self.0
    }

    pub fn has_passed(self, now: Timestamp) -> bool {
        now >= self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).unwrap()
    }

    fn hours(n: i64) -> SignedDuration {
        SignedDuration::from_hours(n)
    }

    fn keep(n: i64) -> KeepDelete {
        KeepDelete::new(hours(n)).unwrap()
    }

    #[test]
    fn keep_delete_is_at_least_rustics_default() {
        assert_eq!(
            KeepDelete::new(hours(23)).map(KeepDelete::get),
            Ok(hours(23))
        );
        assert_eq!(
            KeepDelete::new(hours(144)).map(KeepDelete::get),
            Ok(hours(144))
        );
        for short in [
            hours(23) - SignedDuration::from_secs(1),
            SignedDuration::ZERO,
            hours(-1),
        ] {
            assert_eq!(
                KeepDelete::new(short),
                Err(KeepDeleteTooShort { value: short }),
                "{short:?}"
            );
        }
    }

    #[test]
    fn a_backup_gets_keep_delete_minus_the_prune_margin() {
        // margin = 1 h + the longest prune (2 h)
        assert_eq!(
            backup_budget(Some(keep(6 * 24)), Some(hours(2))),
            hours(141)
        );
    }

    #[test]
    fn without_a_recorded_prune_the_margin_is_one_hour() {
        assert_eq!(backup_budget(Some(keep(48)), None), hours(47));
    }
    #[test]
    fn an_unknown_keep_delete_is_assumed_to_be_rustics_default() {
        assert_eq!(backup_budget(None, None), hours(22));
        assert_eq!(backup_budget(None, Some(hours(3))), hours(19));
    }

    #[test]
    fn a_margin_longer_than_keep_delete_leaves_no_time() {
        assert_eq!(backup_budget(Some(keep(23)), Some(hours(30))), hours(-8));
    }

    #[test]
    fn the_deadline_counts_from_the_index_read() {
        let deadline = Deadline::after_index_read(t(100), SignedDuration::from_secs(60));
        assert_eq!(deadline.at(), t(160));
        assert!(!deadline.has_passed(t(159)));
        assert!(deadline.has_passed(t(160)));
        assert!(deadline.has_passed(t(161)));
    }

    #[test]
    fn a_negative_budget_has_already_passed_at_the_index_read() {
        let deadline = Deadline::after_index_read(t(100), hours(-3));
        assert!(deadline.has_passed(t(100)));
    }

    #[test]
    fn a_budget_beyond_the_calendar_saturates() {
        let deadline = Deadline::after_index_read(t(0), SignedDuration::MAX);
        assert_eq!(deadline.at(), Timestamp::MAX);
        let deadline = Deadline::after_index_read(t(0), SignedDuration::MIN);
        assert_eq!(deadline.at(), Timestamp::MIN);
    }
}
