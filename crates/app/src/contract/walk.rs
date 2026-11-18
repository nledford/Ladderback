//! The `WalkSources` port's contract (§16, decision 89, point 13): what a
//! walk counts, what a `!name` glob leaves out, and where each limit stops
//! it. Each case builds its tree through a [`WalkHarness`].

use std::sync::Arc;

use domain::SignedDuration;
use domain::planning::{Source, SourceFilter};

use crate::ports::{GiveWay, WalkLimits, WalkSources, WalkStop, Walked};

/// Builds a tree for one case.
pub trait WalkHarness {
    /// A walker over a tree of `files`, each a path relative to the tree's
    /// folder and its size in bytes, their folders made as needed, and the
    /// tree's folder, resolved.
    fn tree(&self, files: &[(&str, u64)]) -> (Arc<dyn WalkSources>, Source);
}

/// Three files of 3, 5 and 7 bytes, two of them in `docs`: four entries.
const TREE: &[(&str, u64)] = &[("a.txt", 3), ("docs/b.txt", 5), ("docs/c.txt", 7)];

const ROOMY: WalkLimits = WalkLimits {
    entries: 1000,
    time: SignedDuration::from_secs(60),
};

fn never() -> GiveWay {
    Arc::new(|| false)
}

pub async fn run(harness: &dyn WalkHarness) {
    counts_every_file_and_its_bytes(harness).await;
    a_glob_leaves_out_a_folder(harness).await;
    stops_after_its_limit_of_entries(harness).await;
    stops_when_its_time_is_out(harness).await;
    stops_to_give_way(harness).await;
}

async fn walk(
    harness: &dyn WalkHarness,
    filter: &SourceFilter,
    limits: WalkLimits,
    give_way: GiveWay,
) -> Walked {
    let (walker, root) = harness.tree(TREE);
    let walked = walker.walk(&[root], filter, limits, give_way).await;
    assert!(walked.is_ok(), "the walk failed with {walked:?}");
    walked.unwrap_or_default()
}

async fn counts_every_file_and_its_bytes(harness: &dyn WalkHarness) {
    let walked = walk(harness, &SourceFilter::default(), ROOMY, never()).await;
    assert_eq!(
        walked,
        Walked {
            files: 3,
            bytes: 15,
            unreadable: 0,
            warnings: 0,
            stopped: None
        }
    );
}

async fn a_glob_leaves_out_a_folder(harness: &dyn WalkHarness) {
    let filter = SourceFilter {
        globs: vec!["!docs".to_owned()],
        ..SourceFilter::default()
    };
    let walked = walk(harness, &filter, ROOMY, never()).await;
    assert_eq!((walked.files, walked.bytes, walked.stopped), (1, 3, None));
}

/// Four entries: a limit of four reads them all, of three stops at the
/// fourth, having read `a.txt`, `docs` and `docs/b.txt`.
async fn stops_after_its_limit_of_entries(harness: &dyn WalkHarness) {
    let at = |entries| WalkLimits { entries, ..ROOMY };
    let all = walk(harness, &SourceFilter::default(), at(4), never()).await;
    assert_eq!((all.files, all.stopped), (3, None));
    let short = walk(harness, &SourceFilter::default(), at(3), never()).await;
    assert_eq!(
        (short.files, short.bytes, short.stopped),
        (2, 8, Some(WalkStop::Entries))
    );
}

async fn stops_when_its_time_is_out(harness: &dyn WalkHarness) {
    let none = WalkLimits {
        time: SignedDuration::ZERO,
        ..ROOMY
    };
    let walked = walk(harness, &SourceFilter::default(), none, never()).await;
    assert_eq!((walked.files, walked.stopped), (0, Some(WalkStop::Time)));
}

async fn stops_to_give_way(harness: &dyn WalkHarness) {
    let walked = walk(harness, &SourceFilter::default(), ROOMY, Arc::new(|| true)).await;
    assert_eq!((walked.files, walked.stopped), (0, Some(WalkStop::GaveWay)));
}
