//! One page of a snapshot's folder on its way to the daemon (§16, decision
//! 102): the folder's entries in the order its tree records them, from the
//! position asked for, at most [`PAGE_ENTRIES`] of them and at most
//! [`LISTING_BATCH_BYTES`] serialized, so the page is one message under the
//! protocol's cap whatever the folder holds (§4.1).

use domain::browse::{DisplayName, EntryKind, FolderPage, MAX_ENTRY_NAME_BYTES, PAGE_ENTRIES};
use rustic_core::repofile::{Node, NodeType};

use crate::job::{EntryResult, FolderResult, LISTING_BATCH_BYTES};

/// The page of `nodes`, a folder's tree, that starts at position `from`.
/// An entry whose name is longer than [`MAX_ENTRY_NAME_BYTES`] (no file
/// system writes one) is left out of the page but keeps its position, so the
/// total still counts it and the next page starts after it.
pub(crate) fn page(nodes: &[Node], from: u64) -> FolderResult {
    let total = u64::try_from(nodes.len()).unwrap_or(u64::MAX);
    let start = usize::try_from(from).unwrap_or(usize::MAX);
    let mut entries = Vec::new();
    let mut bytes = 0_usize;
    let mut consumed = 0_u64;
    for node in nodes.iter().skip(start) {
        if entries.len() == PAGE_ENTRIES {
            break;
        }
        if node.name.len() <= MAX_ENTRY_NAME_BYTES {
            let entry = entry(node);
            // The entry as the page's JSON carries it, and the comma after
            // it: an entry that would take the page past the budget starts
            // the next one instead.
            let size = serde_json::to_vec(&entry)
                .map_or(usize::MAX, |json| json.len())
                .saturating_add(1);
            bytes = bytes.saturating_add(size);
            if bytes > LISTING_BATCH_BYTES {
                break;
            }
            entries.push(entry);
        }
        consumed = consumed.saturating_add(1);
    }
    let FolderPage { total, next, .. } = FolderPage::continuing(Vec::new(), from, consumed, total);
    FolderResult::Page {
        entries,
        total,
        next,
    }
}

/// What `node` is, as a page says it.
fn kind(node: &Node) -> EntryKind {
    match node.node_type {
        NodeType::Dir => EntryKind::Folder,
        NodeType::File => EntryKind::File,
        NodeType::Symlink { .. } => EntryKind::Link,
        NodeType::Dev { .. } | NodeType::Chardev { .. } | NodeType::Fifo | NodeType::Socket => {
            EntryKind::Other
        }
    }
}

/// `node` as a page carries it: its name as rustic records it, the key, and
/// as `rustic ls` prints it, to show.
fn entry(node: &Node) -> EntryResult {
    let kind = kind(node);
    EntryResult {
        name: node.name.clone(),
        display: DisplayName::new(&node.name().to_string_lossy()),
        kind,
        size: (kind == EntryKind::File).then_some(node.meta.size),
        modified: node.meta.mtime,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use rustic_core::repofile::Metadata;

    use super::*;

    fn file(name: &str, size: u64) -> Node {
        Node::new_node(
            OsStr::new(name),
            NodeType::File,
            Metadata {
                size,
                ..Metadata::default()
            },
        )
    }

    fn folder(name: &str) -> Node {
        Node::new_node(OsStr::new(name), NodeType::Dir, Metadata::default())
    }

    fn files(n: usize) -> Vec<Node> {
        (0..n)
            .map(|at| file(&format!("file-{at:04}.txt"), 1))
            .collect()
    }

    fn names(result: &FolderResult) -> Vec<&str> {
        match result {
            FolderResult::Page { entries, .. } => {
                entries.iter().map(|entry| entry.name.as_str()).collect()
            }
            other => panic!("a page, not {other:?}"),
        }
    }

    fn paging(result: &FolderResult) -> (u64, Option<u64>) {
        match result {
            FolderResult::Page { total, next, .. } => (*total, *next),
            other => panic!("a page, not {other:?}"),
        }
    }

    #[test]
    fn a_page_holds_each_entry_with_its_kind_size_and_time() {
        let nodes = vec![folder("docs"), file("readme.txt", 5)];
        let FolderResult::Page {
            entries,
            total,
            next,
        } = page(&nodes, 0)
        else {
            panic!("a page");
        };
        assert_eq!(total, 2);
        assert_eq!(next, None);
        assert_eq!(entries[0].name, "docs");
        assert_eq!(entries[0].kind, EntryKind::Folder);
        assert_eq!(entries[0].size, None, "a folder has no size of its own");
        assert_eq!(entries[1].name, "readme.txt");
        assert_eq!(entries[1].kind, EntryKind::File);
        assert_eq!(entries[1].size, Some(5));
    }

    #[test]
    fn an_empty_folder_is_one_empty_page() {
        assert_eq!(
            page(&[], 0),
            FolderResult::Page {
                entries: Vec::new(),
                total: 0,
                next: None
            }
        );
    }

    #[test]
    fn a_folder_past_the_cap_is_read_in_pages_of_the_cap() {
        let nodes = files(PAGE_ENTRIES + 1);
        let first = page(&nodes, 0);
        assert_eq!(names(&first).len(), PAGE_ENTRIES);
        assert_eq!(
            paging(&first),
            (PAGE_ENTRIES as u64 + 1, Some(PAGE_ENTRIES as u64))
        );
        let second = page(&nodes, PAGE_ENTRIES as u64);
        assert_eq!(
            names(&second),
            [format!("file-{PAGE_ENTRIES:04}.txt").as_str()]
        );
        assert_eq!(paging(&second), (PAGE_ENTRIES as u64 + 1, None));
    }

    #[test]
    fn a_page_asked_past_the_end_is_empty_and_the_last() {
        let nodes = files(3);
        assert!(names(&page(&nodes, 3)).is_empty());
        assert_eq!(paging(&page(&nodes, u64::MAX)), (3, None));
    }

    #[test]
    fn a_page_stops_before_it_outgrows_one_message() {
        // Names of the longest kept, each escaped to six bytes a character:
        // far fewer than the cap's worth fit in a page.
        let long = "\u{1}".repeat(MAX_ENTRY_NAME_BYTES);
        let nodes: Vec<Node> = (0..PAGE_ENTRIES)
            .map(|at| file(&format!("{at:04}{}", &long[4..]), 1))
            .collect();
        let first = page(&nodes, 0);
        let kept = names(&first).len();
        assert!(kept > 0 && kept < PAGE_ENTRIES, "{kept}");
        let bytes = serde_json::to_vec(&first).expect("a page serializes").len();
        assert!(bytes <= LISTING_BATCH_BYTES + 1024, "{bytes}");
        assert_eq!(
            paging(&first).1,
            Some(kept as u64),
            "the next page starts after it"
        );
    }

    #[test]
    fn an_entry_with_a_name_no_file_system_writes_is_left_out_but_keeps_its_place() {
        let nodes = vec![
            file("a.txt", 1),
            file(&"x".repeat(MAX_ENTRY_NAME_BYTES + 1), 1),
            file("c.txt", 1),
        ];
        let result = page(&nodes, 0);
        assert_eq!(names(&result), ["a.txt", "c.txt"]);
        assert_eq!(paging(&result), (3, None));
    }

    /// Each entry of `result` as its key and the name it shows.
    fn keys_and_names(result: &FolderResult) -> Vec<(&str, &str)> {
        match result {
            FolderResult::Page { entries, .. } => entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.display.as_str()))
                .collect(),
            other => panic!("a page, not {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn an_entry_shows_its_name_as_written_and_keeps_rustic_s_form_as_its_key() {
        use std::os::unix::ffi::OsStrExt;

        let raw: [&[u8]; 7] = [
            b"say \"hi\".txt",
            b"back\\slash.txt",
            "café.txt".as_bytes(),
            b"new\nline.txt",
            b"tab\there.txt",
            b"bad\xffbyte.txt",
            "abc\u{202e}txt.exe".as_bytes(),
        ];
        let nodes: Vec<Node> = raw
            .iter()
            .map(|name| {
                Node::new_node(OsStr::from_bytes(name), NodeType::File, Metadata::default())
            })
            .collect();
        assert_eq!(
            keys_and_names(&page(&nodes, 0)),
            [
                ("say \\\"hi\\\".txt", "say \"hi\".txt"),
                ("back\\\\slash.txt", "back\\slash.txt"),
                ("café.txt", "café.txt"),
                ("new\\nline.txt", "new\u{240a}line.txt"),
                ("tab\\there.txt", "tab\u{2409}here.txt"),
                ("bad\\xffbyte.txt", "bad\u{fffd}byte.txt"),
                ("abc\u{202e}txt.exe", "abc[U+202E]txt.exe"),
            ]
        );
    }
}
