use std::sync::Arc;

use domain::Timestamp;
use domain::execution::daily_copy::COPY_EVERY;

use super::KeepDailyCopy;
use crate::fakes::{FakeClock, InMemoryDailyCopy};

/// A copy exactly a day old is due now: one look takes a new copy and
/// answers a day from now, rather than answering "now" and taking none.
#[tokio::test]
async fn a_copy_exactly_a_day_old_is_taken_again_at_once() {
    let now: Timestamp = "2026-11-27T12:00:00Z".parse().unwrap();
    let clock = Arc::new(FakeClock::new(now));
    let copies = Arc::new(InMemoryDailyCopy::new(clock.clone()));
    copies.taken_before(now.checked_sub(COPY_EVERY).unwrap());
    let keep = KeepDailyCopy {
        clock,
        copies: copies.clone(),
        reports: Arc::new(|error| panic!("nothing to report, got {error:?}")),
    };

    let next = keep.keep().await;

    assert_eq!(copies.taken(), vec![now]);
    assert_eq!(next, now.checked_add(COPY_EVERY).unwrap());
}
