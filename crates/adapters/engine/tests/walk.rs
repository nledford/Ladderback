//! The real `WalkSources` adapter: rustic's own walker, counted, meets the
//! walk contract on a tree in a scratch folder (§16, decision 89, point 13).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use app::contract::walk::{WalkHarness, run};
use app::ports::{WalkError, WalkLimits, WalkSources};
use domain::SignedDuration;
use domain::planning::{GlobFile, Source, SourceFilter};
use engine::RusticWalk;
use tempfile::TempDir;

/// Builds each case's tree in its own scratch folder, kept until the
/// harness is dropped.
#[derive(Default)]
struct ScratchTrees(Mutex<Vec<TempDir>>);

impl WalkHarness for ScratchTrees {
    fn tree(&self, files: &[(&str, u64)]) -> (Arc<dyn WalkSources>, Source) {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let root = std::fs::canonicalize(scratch.path()).expect("resolved");
        for (path, size) in files {
            let file = root.join(path);
            if let Some(folder) = file.parent() {
                std::fs::create_dir_all(folder).expect("its folder");
            }
            let bytes = vec![b'x'; usize::try_from(*size).expect("small")];
            std::fs::write(&file, bytes).expect("a file");
        }
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(scratch);
        (
            Arc::new(RusticWalk),
            Source::parse(root.to_str().expect("UTF-8")).expect("a source"),
        )
    }
}

#[tokio::test]
async fn rustic_walk_meets_the_walk_contract() {
    run(&ScratchTrees::default()).await;
}

#[tokio::test]
async fn a_glob_rustic_refuses_or_a_glob_file_it_cannot_read_is_a_filter_error() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let root = Source::parse(scratch.path().to_str().expect("UTF-8")).expect("a source");
    let limits = WalkLimits {
        entries: 10,
        time: SignedDuration::from_secs(10),
    };
    let missing: PathBuf = scratch.path().join("no-such-glob-file");
    for filter in [
        SourceFilter {
            globs: vec!["!docs/[".to_owned()],
            ..SourceFilter::default()
        },
        SourceFilter {
            glob_files: vec![GlobFile::parse(missing.to_str().expect("UTF-8")).expect("a path")],
            ..SourceFilter::default()
        },
    ] {
        let walked = RusticWalk
            .walk(
                std::slice::from_ref(&root),
                &filter,
                &[],
                limits,
                Arc::new(|| false),
            )
            .await;
        assert_eq!(walked, Err(WalkError::Filter), "{filter:?}");
    }
}

/// rustic reads a glob file whole as the walk is built, before any limit:
/// one that isn't a regular file, or holds more than 1 MiB, is refused by
/// its metadata before anything is read.
#[tokio::test]
async fn a_glob_file_that_is_not_a_regular_file_or_is_over_a_mebibyte_is_refused_unread() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let root = std::fs::canonicalize(scratch.path()).expect("resolved");
    std::fs::write(root.join("kept.txt"), b"kept").expect("a file");
    std::fs::write(root.join("left-out.log"), b"left out").expect("a file");
    let small = root.join("small-globs");
    std::fs::write(&small, b"!*.log\n").expect("a glob file");
    let large = root.join("large-globs");
    let mut lines = b"!*.log\n".to_vec();
    lines.resize(1024 * 1024 + 1, b'\n');
    std::fs::write(&large, lines).expect("a glob file");
    let folder = root.join("a-folder");
    std::fs::create_dir(&folder).expect("a folder");
    let source = Source::parse(root.to_str().expect("UTF-8")).expect("a source");
    let limits = WalkLimits {
        entries: 100,
        time: SignedDuration::from_secs(10),
    };
    let glob_file = |path: &std::path::Path| GlobFile::parse(path.to_str().expect("UTF-8"));
    let walk = |filter: SourceFilter| {
        let source = source.clone();
        async move {
            RusticWalk
                .walk(&[source], &filter, &[], limits, Arc::new(|| false))
                .await
        }
    };
    let counted = walk(SourceFilter {
        iglob_files: vec![glob_file(&small).expect("a path")],
        ..SourceFilter::default()
    })
    .await
    .expect("a walk");
    // `kept.txt` and the two glob files; the folder isn't a file.
    assert_eq!(counted.files, 3, "{counted:?}");
    for refused in [&large, &folder, &root.join("no-such-file")] {
        for filter in [
            SourceFilter {
                glob_files: vec![glob_file(refused).expect("a path")],
                ..SourceFilter::default()
            },
            SourceFilter {
                iglob_files: vec![glob_file(refused).expect("a path")],
                ..SourceFilter::default()
            },
        ] {
            assert_eq!(walk(filter).await, Err(WalkError::Filter), "{refused:?}");
        }
    }
}
