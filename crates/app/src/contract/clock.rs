use std::sync::Arc;

use async_trait::async_trait;
use domain::SignedDuration;
use futures::FutureExt;

use crate::ports::Clock;

/// Lets the suite make time pass: by advancing a fake clock, or by really
/// waiting for a system clock.
#[async_trait]
pub trait ClockHarness: Send + Sync {
    fn clock(&self) -> Arc<dyn Clock>;
    async fn let_time_pass(&self, by: SignedDuration);
}

/// `step` must be long enough for the harness to tell "not yet" from "now"
/// reliably: a few milliseconds for a fake, a few hundred for a real clock.
pub async fn run(harness: &dyn ClockHarness, step: SignedDuration) {
    sleeping_until_a_past_time_returns_at_once(harness).await;
    a_sleeper_wakes_only_once_its_deadline_has_passed(harness, step).await;
    monotonic_time_moves_on_with_the_time_that_passes(harness, step).await;
    sleeping_for_no_time_returns_at_once(harness).await;
    a_sleeper_wakes_only_once_its_time_has_passed(harness, step).await;
}

async fn sleeping_until_a_past_time_returns_at_once(harness: &dyn ClockHarness) {
    let clock = harness.clock();
    let past = clock.now() - SignedDuration::from_secs(1);
    assert!(
        clock.sleep_until(past).now_or_never().is_some(),
        "sleep_until a past time must not wait"
    );
    let now = clock.now();
    assert!(
        clock.sleep_until(now).now_or_never().is_some(),
        "sleep_until the current time must not wait"
    );
}

async fn a_sleeper_wakes_only_once_its_deadline_has_passed(
    harness: &dyn ClockHarness,
    step: SignedDuration,
) {
    let clock = harness.clock();
    let deadline = clock.now() + step * 2;
    let mut sleeper = clock.sleep_until(deadline);

    harness.let_time_pass(step).await;
    assert!(
        (&mut sleeper).now_or_never().is_none(),
        "a sleeper must not wake before its deadline"
    );

    harness.let_time_pass(step * 2).await;
    assert!(
        sleeper.now_or_never().is_some(),
        "a sleeper must wake once its deadline has passed"
    );
    assert!(
        clock.now() >= deadline,
        "after waking, now ({}) must be at or after the deadline ({deadline})",
        clock.now()
    );
}

async fn monotonic_time_moves_on_with_the_time_that_passes(
    harness: &dyn ClockHarness,
    step: SignedDuration,
) {
    let clock = harness.clock();
    let before = clock.monotonic();
    harness.let_time_pass(step).await;
    let after = clock.monotonic();
    assert!(
        after - before >= step,
        "monotonic time moved {} while {step} passed",
        after - before
    );
}

async fn sleeping_for_no_time_returns_at_once(harness: &dyn ClockHarness) {
    let clock = harness.clock();
    assert!(
        clock.sleep(SignedDuration::ZERO).now_or_never().is_some(),
        "a sleep of no time must not wait"
    );
    assert!(
        clock
            .sleep(SignedDuration::from_secs(-1))
            .now_or_never()
            .is_some(),
        "a sleep of a negative time must not wait"
    );
}

async fn a_sleeper_wakes_only_once_its_time_has_passed(
    harness: &dyn ClockHarness,
    step: SignedDuration,
) {
    let clock = harness.clock();
    let start = clock.monotonic();
    let mut sleeper = clock.sleep(step * 2);
    // A sleep counts from its first poll, as every future starts.
    assert!(
        (&mut sleeper).now_or_never().is_none(),
        "a sleeper must not wake at once"
    );

    harness.let_time_pass(step).await;
    assert!(
        (&mut sleeper).now_or_never().is_none(),
        "a sleeper must not wake before its time has passed"
    );

    harness.let_time_pass(step * 2).await;
    assert!(
        sleeper.now_or_never().is_some(),
        "a sleeper must wake once its time has passed"
    );
    assert!(
        clock.monotonic() - start >= step * 2,
        "after waking, monotonic time must have moved at least {}",
        step * 2
    );
}
