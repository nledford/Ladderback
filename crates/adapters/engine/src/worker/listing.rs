//! A repository's listing on its way to the daemon: each snapshot cut to a
//! size no message can be crowded out by, and the list sent as a series of
//! events none of which is longer than the protocol's cap (technical design
//! §4.1), instead of as one message that a large repository outgrows.

use std::io;

use rustic_core::repofile::SnapshotFile;

use super::convert::{JobError, snapshot_id};
use crate::job::{
    LISTING_BATCH_BYTES, MAX_LISTED_SNAPSHOTS, MAX_LISTED_TEXT_BYTES, SnapshotResult,
};

/// `text` as a listing carries it, and whether it had to be cut: at most
/// [`MAX_LISTED_TEXT_BYTES`] bytes, cut at a char boundary and ending in `…`
/// where it was.
pub(crate) fn cut(text: &str) -> (String, bool) {
    const MARK: &str = "…";
    if text.len() <= MAX_LISTED_TEXT_BYTES {
        return (text.to_owned(), false);
    }
    let room = MAX_LISTED_TEXT_BYTES - MARK.len();
    let end = (0..=room)
        .rev()
        .find(|&at| text.is_char_boundary(at))
        .unwrap_or(0);
    let mut cut = text.get(..end).unwrap_or_default().to_owned();
    cut.push_str(MARK);
    (cut, true)
}

/// `texts` as a listing carries them: the first `most` of them, each cut,
/// and whether any of them was left out or cut.
fn cut_all<'a>(
    texts: impl ExactSizeIterator<Item = &'a String>,
    most: usize,
) -> (Vec<String>, bool) {
    let mut truncated = texts.len() > most;
    let kept = texts
        .take(most)
        .map(|text| {
            let (text, was_cut) = cut(text);
            truncated |= was_cut;
            text
        })
        .collect();
    (kept, truncated)
}

/// One snapshot of a listing. What a listing may carry of one is bounded
/// (an input-derived size, §13.5): at most [`api::MAX_SNAPSHOT_PATHS`] paths
/// and [`api::MAX_SNAPSHOT_TAGS`] tags, no text longer than
/// [`MAX_LISTED_TEXT_BYTES`]. That keeps any one snapshot well under the
/// message cap whatever a repository holds, and what was cut is said in
/// `paths_truncated` and `tags_truncated`, never left out silently.
pub(crate) fn snapshot_result(snapshot: &SnapshotFile) -> Result<SnapshotResult, JobError> {
    let (paths, paths_truncated) = cut_all(
        snapshot.paths.iter().collect::<Vec<_>>().into_iter(),
        api::MAX_SNAPSHOT_PATHS,
    );
    let (tags, tags_truncated) = cut_all(
        snapshot.tags.iter().collect::<Vec<_>>().into_iter(),
        api::MAX_SNAPSHOT_TAGS,
    );
    Ok(SnapshotResult {
        id: snapshot_id(&snapshot.id)?,
        time: snapshot.time.timestamp(),
        hostname: cut(&snapshot.hostname).0,
        label: cut(&snapshot.label).0,
        tags,
        paths,
        paths_truncated,
        tags_truncated,
    })
}

/// The snapshots of a repository as a listing is sent: newest first, in
/// batches that each serialize to at most [`LISTING_BATCH_BYTES`] (a batch of
/// one snapshot is as long as that snapshot is). Refuses a repository with
/// more than [`MAX_LISTED_SNAPSHOTS`] snapshots rather than send part of its
/// list. Returns how many were sent.
pub(crate) fn send(
    snapshots: &[SnapshotFile],
    emit: impl FnMut(Vec<SnapshotResult>) -> io::Result<()>,
) -> Result<u64, JobError> {
    if snapshots.len() > MAX_LISTED_SNAPSHOTS {
        return Err(JobError::failed(format!(
            "the repository has {} snapshots, more than the {MAX_LISTED_SNAPSHOTS} a listing carries",
            snapshots.len()
        )));
    }
    let mut listed: Vec<SnapshotResult> = snapshots
        .iter()
        .map(snapshot_result)
        .collect::<Result<_, JobError>>()?;
    listed.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
    Ok(batches(listed, emit)?)
}

/// Hands `listed`, in order, to `emit` in batches of at most
/// [`LISTING_BATCH_BYTES`] serialized (a snapshot alone in a batch may be
/// longer), and returns how many there were.
fn batches(
    listed: Vec<SnapshotResult>,
    mut emit: impl FnMut(Vec<SnapshotResult>) -> io::Result<()>,
) -> io::Result<u64> {
    let mut count = 0_u64;
    let mut batch = Vec::new();
    let mut bytes = 0_usize;
    for snapshot in listed {
        // One more for the comma that separates it from the one before.
        let size = serde_json::to_vec(&snapshot)?.len().saturating_add(1);
        if !batch.is_empty() && bytes.saturating_add(size) > LISTING_BATCH_BYTES {
            emit(std::mem::take(&mut batch))?;
            bytes = 0;
        }
        bytes = bytes.saturating_add(size);
        batch.push(snapshot);
        count = count.saturating_add(1);
    }
    if !batch.is_empty() {
        emit(batch)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use domain::{SnapshotId, Timestamp};

    use super::*;
    use crate::job::WorkerEvent;

    fn listed(n: i64, label: &str) -> SnapshotResult {
        SnapshotResult {
            id: SnapshotId::parse(&format!("{n:064x}")).expect("a snapshot ID"),
            time: Timestamp::from_second(n).expect("a time"),
            hostname: "demo-host".to_owned(),
            label: label.to_owned(),
            tags: Vec::new(),
            paths: Vec::new(),
            paths_truncated: false,
            tags_truncated: false,
        }
    }

    fn sent(listed_snapshots: Vec<SnapshotResult>) -> Vec<Vec<SnapshotResult>> {
        let mut sent = Vec::new();
        batches(listed_snapshots, |batch| {
            sent.push(batch);
            Ok(())
        })
        .expect("nothing breaks");
        sent
    }

    #[test]
    fn a_text_within_the_limit_is_kept_whole() {
        let text = "x".repeat(MAX_LISTED_TEXT_BYTES);
        assert_eq!(cut(&text), (text, false));
    }

    #[test]
    fn a_text_one_byte_over_is_cut_to_the_limit_with_a_mark() {
        let (cut, was_cut) = cut(&"x".repeat(MAX_LISTED_TEXT_BYTES + 1));
        assert!(was_cut);
        assert_eq!(cut.len(), MAX_LISTED_TEXT_BYTES);
        assert!(cut.ends_with('…'));
        assert!(cut.starts_with("xxx"));
    }

    #[test]
    fn a_cut_never_splits_a_character() {
        // Two-byte characters: an odd limit minus the mark falls inside one.
        let (cut, was_cut) = cut(&"é".repeat(MAX_LISTED_TEXT_BYTES));
        assert!(was_cut);
        assert!(cut.len() <= MAX_LISTED_TEXT_BYTES);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_few_snapshots_go_out_as_one_batch() {
        let sent = sent(vec![listed(3, ""), listed(2, ""), listed(1, "")]);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].len(), 3);
    }

    #[test]
    fn nothing_listed_sends_nothing() {
        assert_eq!(sent(Vec::new()), Vec::<Vec<SnapshotResult>>::new());
    }

    #[test]
    fn a_long_listing_goes_out_in_batches_under_the_limit_that_keep_every_snapshot_in_order() {
        let label = "x".repeat(2_000);
        let all: Vec<SnapshotResult> = (0..1_500).rev().map(|n| listed(n, &label)).collect();
        let sent = sent(all.clone());
        assert!(sent.len() > 1, "{} batches", sent.len());
        for batch in &sent {
            let line = serde_json::to_vec(batch).expect("a batch serializes").len();
            assert!(
                line <= LISTING_BATCH_BYTES,
                "a batch of {} is {line} bytes",
                batch.len()
            );
        }
        let flat: Vec<SnapshotResult> = sent.into_iter().flatten().collect();
        assert_eq!(flat, all);
    }

    #[test]
    fn a_batch_closes_exactly_where_the_next_snapshot_would_not_fit() {
        let one = listed(0, &"x".repeat(1_000));
        let size = serde_json::to_vec(&one).expect("serializes").len() + 1;
        let fits = LISTING_BATCH_BYTES / size;
        let at_the_limit = sent(
            (0..fits as i64)
                .map(|n| listed(n, &"x".repeat(1_000)))
                .collect(),
        );
        assert_eq!(at_the_limit.len(), 1, "{fits} snapshots fill a batch");
        let past_it = sent(
            (0..=fits as i64)
                .map(|n| listed(n, &"x".repeat(1_000)))
                .collect(),
        );
        assert_eq!(past_it.len(), 2, "one more starts the next");
        assert_eq!(past_it[1].len(), 1);
    }

    #[test]
    fn a_snapshot_longer_than_a_batch_still_goes_out_alone() {
        let huge = listed(1, &"x".repeat(LISTING_BATCH_BYTES + 10));
        let sent = sent(vec![listed(3, ""), huge.clone(), listed(2, "")]);
        assert_eq!(sent.len(), 3, "{sent:?}");
        assert_eq!(sent[1], vec![huge]);
    }

    #[test]
    fn a_broken_stream_ends_the_listing_with_its_error() {
        let result = batches(vec![listed(1, "")], |_| {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        });
        assert_eq!(
            result.expect_err("the stream is broken").kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    fn file(paths: &[String], tags: &[String]) -> SnapshotFile {
        let list = |texts: &[String]| {
            if texts.is_empty() {
                rustic_core::repofile::StringList::default()
            } else {
                texts.join(",").parse().expect("a list")
            }
        };
        SnapshotFile {
            paths: list(paths),
            tags: list(tags),
            ..SnapshotFile::default()
        }
    }

    fn numbered(prefix: &str, count: usize) -> Vec<String> {
        (0..count).map(|n| format!("{prefix}{n:03}")).collect()
    }

    #[test]
    fn exactly_as_many_paths_and_tags_as_a_listing_takes_are_not_truncated() {
        let paths = numbered("/p", api::MAX_SNAPSHOT_PATHS);
        let tags = numbered("t", api::MAX_SNAPSHOT_TAGS);
        let result = snapshot_result(&file(&paths, &tags)).expect("converts");
        assert_eq!(result.paths, paths);
        assert_eq!(result.tags, tags);
        assert!(!result.paths_truncated && !result.tags_truncated);
    }

    #[test]
    fn one_path_more_is_left_out_and_said_so_and_one_tag_more_likewise() {
        let paths = numbered("/p", api::MAX_SNAPSHOT_PATHS + 1);
        let tags = numbered("t", api::MAX_SNAPSHOT_TAGS + 1);
        let result = snapshot_result(&file(&paths, &tags)).expect("converts");
        assert_eq!(result.paths, paths[..api::MAX_SNAPSHOT_PATHS]);
        assert_eq!(result.tags, tags[..api::MAX_SNAPSHOT_TAGS]);
        assert!(result.paths_truncated && result.tags_truncated);
        let only_paths = snapshot_result(&file(&paths, &[])).expect("converts");
        assert!(only_paths.paths_truncated && !only_paths.tags_truncated);
        let only_tags = snapshot_result(&file(&[], &tags)).expect("converts");
        assert!(!only_tags.paths_truncated && only_tags.tags_truncated);
    }

    #[test]
    fn a_path_or_tag_cut_short_says_so() {
        let long = "x".repeat(MAX_LISTED_TEXT_BYTES + 1);
        let result = snapshot_result(&file(
            std::slice::from_ref(&long),
            std::slice::from_ref(&long),
        ))
        .expect("converts");
        assert!(result.paths_truncated && result.tags_truncated);
        assert!(result.paths[0].ends_with('…') && result.tags[0].ends_with('…'));
    }

    #[test]
    fn a_host_or_label_too_long_is_cut_with_its_mark() {
        let long = "y".repeat(MAX_LISTED_TEXT_BYTES + 1);
        let snapshot = SnapshotFile {
            hostname: long.clone(),
            label: long,
            ..SnapshotFile::default()
        };
        let result = snapshot_result(&snapshot).expect("converts");
        assert_eq!(result.hostname.len(), MAX_LISTED_TEXT_BYTES);
        assert_eq!(result.label.len(), MAX_LISTED_TEXT_BYTES);
        assert!(result.hostname.ends_with('…') && result.label.ends_with('…'));
    }

    #[test]
    fn the_worst_snapshot_a_listing_allows_fits_one_message_alone() {
        // Every text at its longest, each character one the JSON escapes to
        // six bytes: what `MAX_LISTED_TEXT_BYTES` is sized against.
        let worst = "\u{1}".repeat(MAX_LISTED_TEXT_BYTES);
        let all = |count: usize| vec![worst.clone(); count];
        let snapshot = SnapshotResult {
            hostname: worst.clone(),
            label: worst.clone(),
            tags: all(api::MAX_SNAPSHOT_TAGS),
            paths: all(api::MAX_SNAPSHOT_PATHS),
            ..listed(1, "")
        };
        let line = serde_json::to_vec(&WorkerEvent::Snapshots {
            snapshots: vec![snapshot],
        })
        .expect("serializes")
        .len();
        assert!(line < crate::job::MAX_MESSAGE_BYTES, "{line} bytes");
    }

    #[test]
    fn more_snapshots_than_a_listing_carries_fail_it_before_anything_is_sent() {
        let snapshots = vec![SnapshotFile::default(); MAX_LISTED_SNAPSHOTS + 1];
        let mut sent = false;
        let error = send(&snapshots, |_| {
            sent = true;
            Ok(())
        })
        .expect_err("too many to list");
        assert!(!sent, "no part of the list went out");
        let JobError::Failed { detail, .. } = error else {
            panic!("a failure to report: {error:?}");
        };
        assert!(
            detail.contains(&MAX_LISTED_SNAPSHOTS.to_string()),
            "{detail}"
        );
    }

    #[test]
    fn a_listing_is_sent_newest_first_and_counted() {
        let at = |second: i64| SnapshotFile {
            time: jiff::Timestamp::from_second(second)
                .expect("a time")
                .to_zoned(jiff::tz::TimeZone::UTC),
            ..SnapshotFile::default()
        };
        let mut times = Vec::new();
        let count = send(&[at(10), at(30), at(20)], |batch| {
            times.extend(batch.iter().map(|s| s.time.as_second()));
            Ok(())
        })
        .expect("sent");
        assert_eq!(count, 3);
        assert_eq!(times, [30, 20, 10]);
    }

    #[test]
    fn the_count_is_every_snapshot_sent() {
        let all: Vec<SnapshotResult> = (0..7).map(|n| listed(n, "")).collect();
        assert_eq!(batches(all, |_| Ok(())).expect("sent"), 7);
    }
}
