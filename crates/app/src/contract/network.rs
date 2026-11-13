use std::assert_matches;
use std::sync::Arc;

use crate::ports::{Network, PlatformError};

/// Sets up a network in a known state: a fake told what to say, or a real
/// adapter reading a machine the harness made up.
pub trait NetworkHarness: Send + Sync {
    /// A machine with a working connection.
    fn online(&self) -> Arc<dyn Network>;
    /// A machine whose connections are all down.
    fn offline(&self) -> Arc<dyn Network>;
    /// A machine with nothing but its loopback interface up.
    fn only_loopback(&self) -> Arc<dyn Network>;
    /// A source the adapter can read but doesn't understand.
    fn unrecognized(&self) -> Arc<dyn Network>;
    /// A source the adapter can't read.
    fn unreadable(&self) -> Arc<dyn Network>;
}

pub async fn run(harness: &dyn NetworkHarness) {
    assert_eq!(
        harness.online().is_online().await,
        Ok(true),
        "a machine with a working connection is online"
    );
    assert_eq!(
        harness.offline().is_online().await,
        Ok(false),
        "a machine whose connections are down is offline"
    );
    assert_eq!(
        harness.only_loopback().is_online().await,
        Ok(false),
        "a machine that can reach only itself is offline"
    );
    let answer = harness.unrecognized().is_online().await;
    assert_matches!(
        answer,
        Err(PlatformError::Unrecognized { .. }),
        "a source this adapter doesn't understand is no answer, not a guess: {answer:?}"
    );
    let answer = harness.unreadable().is_online().await;
    assert_matches!(
        answer,
        Err(PlatformError::Unreadable { .. }),
        "a source that can't be read is no answer, not a guess: {answer:?}"
    );
}
