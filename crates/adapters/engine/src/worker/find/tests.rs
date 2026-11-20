use std::cell::RefCell;
use std::collections::HashMap;

use domain::browse::EntryKind;

use super::*;

/// An entry of a tree in memory: its name, its tree for a folder, and its
/// size.
type Entry = (&'static str, Option<u32>, u64);

/// Trees in memory, by number, counting each read.
#[derive(Default)]
struct Memory {
    trees: HashMap<u32, Vec<Entry>>,
    reads: RefCell<Vec<u32>>,
}

impl Memory {
    fn with(mut self, id: u32, children: Vec<(&'static str, Option<u32>, u64)>) -> Self {
        self.trees.insert(id, children);
        self
    }

    fn reads(&self) -> usize {
        self.reads.borrow().len()
    }
}

impl Trees for Memory {
    type Id = u32;

    fn children(&self, id: &u32) -> Result<Vec<Child<u32>>, JobError> {
        self.reads.borrow_mut().push(*id);
        let children = self
            .trees
            .get(id)
            .ok_or_else(|| JobError::failed(format!("no tree {id}")))?;
        Ok(children
            .iter()
            .map(|&(name, subtree, size)| {
                let kind = if subtree.is_some() {
                    EntryKind::Folder
                } else {
                    EntryKind::File
                };
                Child {
                    entry: EntryResult {
                        name: name.to_owned(),
                        display: DisplayName::new(name),
                        kind,
                        size: (kind == EntryKind::File).then_some(size),
                        modified: None,
                    },
                    name: name.to_owned(),
                    subtree,
                }
            })
            .collect())
    }
}

fn file(name: &'static str, size: u64) -> (&'static str, Option<u32>, u64) {
    (name, None, size)
}

fn folder(name: &'static str, tree: u32) -> (&'static str, Option<u32>, u64) {
    (name, Some(tree), 0)
}

fn snapshot(n: u8) -> SnapshotId {
    SnapshotId::parse(&format!("{n:02x}").repeat(32)).expect("a snapshot ID")
}

fn root(n: u8, tree: u32) -> Root<u32> {
    Root {
        snapshot: snapshot(n),
        time: Timestamp::from_second(1_800_000_000 - i64::from(n)).expect("a time"),
        tree,
    }
}

fn pattern(text: &str) -> NamePattern {
    NamePattern::parse(text).expect("a pattern")
}

fn find(trees: &Memory, roots: Vec<Root<u32>>, text: &str, most: u32) -> FindResult {
    search(trees, roots, 0, &pattern(text), most, || false).expect("a search")
}

/// Each match as its path shown and the snapshots holding it, by number.
fn matches(result: &FindResult) -> Vec<(&str, Vec<SnapshotId>)> {
    result
        .matches
        .iter()
        .map(|found| {
            (
                found.display.as_str(),
                found
                    .seen
                    .iter()
                    .map(|seen| seen.snapshot.clone())
                    .collect(),
            )
        })
        .collect()
}

/// Two snapshots: the newer (1) holds `docs/taxes.pdf`, of 7 bytes, and the
/// older (2) holds `docs` without it. Both hold the unchanged `photos`.
fn two_snapshots() -> Memory {
    Memory::default()
        .with(10, vec![folder("docs", 11), folder("photos", 30)])
        .with(11, vec![file("taxes.pdf", 7), file("notes.txt", 1)])
        .with(20, vec![folder("docs", 21), folder("photos", 30)])
        .with(21, vec![file("notes.txt", 1)])
        .with(30, vec![file("beach.jpg", 9), folder("old", 31)])
        .with(31, vec![file("TAXES-2019.PDF", 3)])
}

#[test]
fn a_match_says_which_snapshots_hold_it_and_its_size_in_each() {
    let trees = two_snapshots();
    let result = find(&trees, vec![root(1, 10), root(2, 20)], "taxes*", 100);
    assert_eq!(
        matches(&result),
        [
            ("docs/taxes.pdf", vec![snapshot(1)]),
            ("photos/old/TAXES-2019.PDF", vec![snapshot(1), snapshot(2)]),
        ]
    );
    let first = result.matches.first().expect("a match");
    assert_eq!(first.path, ["docs", "taxes.pdf"]);
    assert_eq!(first.kind, EntryKind::File);
    assert_eq!(first.seen.first().and_then(|seen| seen.size), Some(7));
    assert_eq!(result.stopped, None);
    assert_eq!(result.left_out, 0);
    let searched: Vec<SnapshotId> = result
        .searched
        .iter()
        .map(|searched| searched.snapshot.clone())
        .collect();
    assert_eq!(searched, [snapshot(1), snapshot(2)]);
}

#[test]
fn a_folder_that_did_not_change_between_snapshots_is_read_once() {
    let trees = two_snapshots();
    find(&trees, vec![root(1, 10), root(2, 20)], "*.pdf", 100);
    // 10, 11, 30, 31, then 20, 21: `photos` (30) and `old` (31) once.
    assert_eq!(trees.reads(), 6);
}

#[test]
fn a_folder_s_own_name_matches_as_a_file_s_does() {
    let trees = two_snapshots();
    let result = find(&trees, vec![root(1, 10)], "PHOTOS", 100);
    assert_eq!(matches(&result), [("photos", vec![snapshot(1)])]);
    assert_eq!(
        result.matches.first().map(|found| found.kind),
        Some(EntryKind::Folder)
    );
    assert_eq!(
        result.matches.first().map(|found| found.seen[0].size),
        Some(None)
    );
}

#[test]
fn a_search_stops_at_its_matches_and_says_so() {
    let trees = two_snapshots();
    let result = find(&trees, vec![root(1, 10), root(2, 20)], "*", 3);
    assert_eq!(
        matches(&result),
        [
            ("docs", vec![snapshot(1)]),
            ("docs/taxes.pdf", vec![snapshot(1)]),
            ("docs/notes.txt", vec![snapshot(1)]),
        ]
    );
    assert_eq!(result.stopped, Some(SearchStop::Matches));
    assert!(
        result.searched.is_empty(),
        "no snapshot was read to its end"
    );
    assert_eq!(result.left_out, 2);
}

#[test]
fn a_search_that_finds_exactly_its_matches_is_not_cut_short() {
    let trees = two_snapshots();
    let result = find(&trees, vec![root(1, 10), root(2, 20)], "taxes*", 3);
    assert_eq!(result.stopped, None);
    assert_eq!(result.searched.len(), 2);
}

#[test]
fn a_search_of_fewer_snapshots_than_there_are_says_so() {
    let trees = two_snapshots();
    let result = search(&trees, vec![root(1, 10)], 1, &pattern("*.pdf"), 100, || {
        false
    })
    .expect("a search");
    assert_eq!(result.stopped, Some(SearchStop::Snapshots));
    assert_eq!(result.left_out, 1);
    assert_eq!(result.searched.len(), 1);
}

#[test]
fn a_search_whose_time_is_up_keeps_what_it_found_and_says_so() {
    let trees = two_snapshots();
    let mut asked = 0;
    let result = search(
        &trees,
        vec![root(1, 10), root(2, 20)],
        0,
        &pattern("*.pdf"),
        100,
        || {
            asked += 1;
            asked > 4
        },
    )
    .expect("a search");
    assert_eq!(result.stopped, Some(SearchStop::Time));
    assert_eq!(
        result.searched.len(),
        1,
        "the first snapshot was read to its end"
    );
    assert_eq!(result.left_out, 1);
    assert_eq!(result.matches.len(), 2);
}

#[test]
fn a_tree_that_cannot_be_read_fails_the_search() {
    let trees = Memory::default().with(10, vec![folder("gone", 99)]);
    assert!(search(&trees, vec![root(1, 10)], 0, &pattern("*"), 100, || false).is_err());
}

#[test]
fn the_matches_stop_before_they_outgrow_one_message() {
    let long: &'static str = "x".repeat(MAX_ENTRY_NAME_BYTES).leak();
    let children = (0..200).map(|_| file(long, 1)).collect();
    let folders = (0..200_u32)
        .map(|at| -> (&'static str, Option<u32>, u64) { (format!("{at:03}").leak(), Some(1), 0) });
    let trees = Memory::default()
        .with(1, children)
        .with(0, folders.collect());
    let result = find(&trees, vec![root(1, 0)], "x*", 500);
    assert_eq!(result.stopped, Some(SearchStop::Matches));
    let bytes = serde_json::to_vec(&result)
        .expect("a result serializes")
        .len();
    assert!(bytes <= MATCH_BYTES, "{bytes}");
    assert!(!result.matches.is_empty());
}

#[test]
fn a_name_no_file_system_writes_is_left_out() {
    let long: &'static str = "x".repeat(MAX_ENTRY_NAME_BYTES + 1).leak();
    let trees = Memory::default().with(1, vec![file(long, 1), file("x.txt", 1)]);
    let result = find(&trees, vec![root(1, 1)], "x*", 100);
    assert_eq!(matches(&result), [("x.txt", vec![snapshot(1)])]);
}
