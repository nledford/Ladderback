//! The gallery's tree demo (technical design §4.5, "Tests").

use std::rc::Rc;
use std::sync::Arc;

use api::{EntryDto, EntryKindDto, FolderDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::{FolderLoader, FolderPage, Tree};
use crate::repositories::Sentence;
use crate::text;

/// The tree's demo (`#tree-demo`).
#[component]
pub(super) fn TreeDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="tree-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TREE_HEADING) />
            <Tree
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_TREE)
                id="gallery-tree".to_owned()
                loader=demo_tree(Arc::clone(&localizer))
                empty=t(msg::UI_SNAPSHOT_EMPTY)
            />
        </section>
    }
}

/// The demo tree's folders, read at once: `documents` holds two files,
/// `empty` nothing, `many` five files in pages of two, `slow` is never
/// answered (loading) and `unreadable` is refused (error). One file's key
/// isn't the name it shows, and one name is written right to left.
fn demo_tree(localizer: Arc<Localizer>) -> FolderLoader {
    Rc::new(move |path: String, from: u64| -> FolderPage {
        let name = |id| text(&localizer, id);
        let entry = |id, kind, size: Option<u64>, modified: Option<&str>| EntryDto {
            name: name(id),
            display: name(id),
            kind,
            size,
            modified: modified.map(str::to_owned),
        };
        let folder = |id| entry(id, EntryKindDto::Folder, None, None);
        let file = |id, size| entry(id, EntryKindDto::File, Some(size), None);
        let entries = if path.is_empty() {
            vec![
                folder(msg::UI_GALLERY_TREE_DOCUMENTS),
                folder(msg::UI_GALLERY_TREE_EMPTY),
                folder(msg::UI_GALLERY_TREE_MANY),
                folder(msg::UI_GALLERY_TREE_SLOW),
                folder(msg::UI_GALLERY_TREE_UNREADABLE),
                entry(msg::UI_GALLERY_TREE_LATEST, EntryKindDto::Link, None, None),
                entry(msg::UI_GALLERY_TREE_PIPE, EntryKindDto::Other, None, None),
                // rustic keeps U+202E as it is in the key; the daemon shows
                // it visibly.
                EntryDto {
                    name: "abc\u{202e}txt.exe".to_owned(),
                    ..file(msg::UI_GALLERY_TREE_SHOWN, 300)
                },
                file(msg::UI_GALLERY_TREE_WRITTEN_RTL, 64),
                file(msg::UI_GALLERY_TREE_README, 1_200),
            ]
        } else if path == name(msg::UI_GALLERY_TREE_DOCUMENTS) {
            vec![
                entry(
                    msg::UI_GALLERY_TREE_NOTES,
                    EntryKindDto::File,
                    Some(5),
                    Some("2026-11-01T12:00:00Z"),
                ),
                file(msg::UI_GALLERY_TREE_REPORT, 2_048),
            ]
        } else if path == name(msg::UI_GALLERY_TREE_EMPTY) {
            Vec::new()
        } else if path == name(msg::UI_GALLERY_TREE_MANY) {
            [
                msg::UI_GALLERY_TREE_ALPHA,
                msg::UI_GALLERY_TREE_BRAVO,
                msg::UI_GALLERY_TREE_CHARLIE,
                msg::UI_GALLERY_TREE_DELTA,
                msg::UI_GALLERY_TREE_ECHO,
            ]
            .into_iter()
            .map(|id| file(id, 100))
            .collect()
        } else if path == name(msg::UI_GALLERY_TREE_SLOW) {
            return Box::pin(std::future::pending());
        } else {
            let refused = Sentence::new(&name(msg::UI_GALLERY_TREE_UNREADABLE_SAID), None);
            return Box::pin(std::future::ready(Err(refused)));
        };
        // A page of two of `many`; every other folder in one page.
        let (total, size) = (
            entries.len() as u64,
            if entries.len() == 5 { 2 } else { 1_000 },
        );
        let page: Vec<EntryDto> = entries
            .into_iter()
            .skip(usize::try_from(from).unwrap_or(usize::MAX))
            .take(size)
            .collect();
        let next = from.saturating_add(page.len() as u64);
        Box::pin(std::future::ready(Ok(FolderDto {
            path,
            entries: page,
            total,
            next: (next < total).then_some(next),
        })))
    })
}
