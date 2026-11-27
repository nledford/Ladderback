//! The daily copy of the database (technical design §9; §16, decision 174):
//! when the daemon takes the next one. A copy is due a day after the one
//! there is was taken, and at once when there is none. Nothing here reads a
//! clock: the time comes from the `Clock` port, and when the copy there is
//! was taken from the copy itself.

use crate::{SignedDuration, Timestamp};

/// How long a daily copy stands before the next is due.
pub const COPY_EVERY: SignedDuration = SignedDuration::from_hours(24);

/// How long after a copy that wasn't taken the daemon tries again.
pub const RETRY_AFTER: SignedDuration = SignedDuration::from_hours(1);

/// When the next daily copy is due, at `now`, the copy there is having been
/// taken at `taken`: a day after it, which may be `now` or before, when it
/// is due already. At once when there is none, and when it was taken after
/// `now`: the clock has been set back since, and a copy that seems to come
/// from the future would otherwise stand until the clock caught up with it.
pub fn next_copy(taken: Option<Timestamp>, now: Timestamp) -> Timestamp {
    let _ = (taken, now);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn with_no_copy_the_first_is_due_at_once() {
        let now = at("2026-06-10T09:00:00Z");
        assert_eq!(next_copy(None, now), now);
    }

    #[test]
    fn a_copy_taken_today_makes_the_next_due_a_day_after_it() {
        let taken = at("2026-06-10T08:00:00Z");
        let now = at("2026-06-10T09:00:00Z");
        assert_eq!(next_copy(Some(taken), now), at("2026-06-11T08:00:00Z"));
    }

    #[test]
    fn a_copy_taken_this_instant_makes_the_next_due_a_day_later() {
        let now = at("2026-06-10T09:00:00Z");
        assert_eq!(next_copy(Some(now), now), at("2026-06-11T09:00:00Z"));
    }

    #[test]
    fn a_copy_more_than_a_day_old_makes_the_next_due_already() {
        let taken = at("2026-06-08T09:00:00Z");
        let now = at("2026-06-10T09:00:00Z");
        assert_eq!(next_copy(Some(taken), now), at("2026-06-09T09:00:00Z"));
    }

    #[test]
    fn a_copy_taken_after_now_makes_the_next_due_at_once() {
        let taken = at("2026-06-10T09:00:01Z");
        let now = at("2026-06-10T09:00:00Z");
        assert_eq!(next_copy(Some(taken), now), now);
    }
}
