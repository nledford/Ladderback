//! What a snapshot's tree shows and does (technical design §4.5, "Tree"; §16,
//! decision 102), decided on the host: which rows show, which one holds the
//! tree's one tab stop, what a key does and which page of which folder it
//! asks for, and what an answer changes. `components/tree.rs`'s `Tree` is its
//! DOM glue, which the `@ui` scenarios run in a browser.
//!
//! The tree is an arena: node 0 is the snapshot's top folder, never a row of
//! its own, and every other node an entry some folder's answer listed. A
//! folder is read when it is first opened, a page at a time; a folder with
//! more after its last page read ends in a row that reads the next.

use std::collections::HashMap;

use api::{EntryDto, EntryKindDto, FolderDto};

use crate::repositories::Sentence;

/// How long after a typed character the next one still adds to the same
/// search (WAI-ARIA tree pattern, type-ahead).
pub(crate) const TYPE_AHEAD_MS: f64 = 500.0;

/// The most characters one type-ahead search holds: a person types a few.
const TYPED_MAX: usize = 64;

/// Where the reading of a folder stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load {
    /// Never asked for.
    Unread,
    /// The page starting at `from` is asked for.
    Loading { from: u64 },
    /// Every page asked for is in.
    Read,
    /// Its first page was refused: opening it asks again.
    Failed,
}

/// One entry of the snapshot, and, for a folder, what is read of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) entry: EntryDto,
    parent: usize,
    /// Its `aria-level`: 1 for an entry of the top folder.
    pub(crate) level: u32,
    /// Its `aria-posinset`: where its folder lists it, from 1.
    pub(crate) position: u64,
    pub(crate) open: bool,
    /// The entries read of it so far, in its order.
    pub(crate) children: Vec<usize>,
    /// How many entries it holds in all, as its last answer said.
    pub(crate) total: u64,
    /// Where its next page starts, while one is left.
    pub(crate) next: Option<u64>,
    pub(crate) load: Load,
}

impl Node {
    fn new(entry: EntryDto, parent: usize, level: u32, position: u64) -> Node {
        Node {
            entry,
            parent,
            level,
            position,
            open: false,
            children: Vec::new(),
            total: 0,
            next: None,
            load: Load::Unread,
        }
    }

    pub(crate) fn is_folder(&self) -> bool {
        self.entry.kind == EntryKindDto::Folder
    }
}

/// A row of the tree: an entry, or the row that reads the next page of the
/// folder it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Row {
    Entry(usize),
    More(usize),
}

/// A page of a folder to ask the daemon for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    /// The folder's node.
    pub(crate) folder: usize,
    /// Its path in the snapshot, its names joined by `/`.
    pub(crate) path: String,
    pub(crate) from: u64,
}

/// What a key did: whether the tree took it (so the browser does nothing
/// else with it), and the page it asks for, if any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pressed {
    pub(crate) handled: bool,
    pub(crate) request: Option<Request>,
}

/// What a row says after its name, for a screen reader and on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// A folder, closed or with entries.
    Folder,
    /// A folder whose page is on its way.
    Loading,
    /// A folder read, with nothing in it.
    Empty,
    /// A folder whose first page was refused.
    Failed,
    File,
    Link,
    Other,
}

impl State {
    /// The value of the `$kind` argument of `ui-tree-entry`.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            State::Folder => "folder",
            State::Loading => "loading",
            State::Empty => "empty",
            State::Failed => "failed",
            State::File => "file",
            State::Link => "link",
            State::Other => "other",
        }
    }
}

/// What a shown row says to the page besides its level and its entry's
/// name, which never change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowView {
    /// `aria-posinset` (`place`).
    pub(crate) position: u64,
    /// `aria-setsize` (`set_size`).
    pub(crate) set_size: u64,
    /// `aria-expanded`: whether a folder's row is open; nothing for any
    /// other row.
    pub(crate) expanded: Option<bool>,
    /// `aria-busy` (`busy`).
    pub(crate) busy: bool,
    /// Whether the row holds the tree's one tab stop (`tab_stop`).
    pub(crate) stop: bool,
    /// What an entry's row says after its name (`state`); nothing for the
    /// row that reads the next page.
    pub(crate) state: Option<State>,
    /// For the row that reads the next page: how many of its folder's
    /// entries are shown.
    pub(crate) shown: u64,
}

/// Every shown row and what it says, worked out in one walk of the tree, so
/// a change costs the page one walk rather than one for each row.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Views {
    /// The rows shown, top to bottom (`rows`).
    pub(crate) rows: Vec<Row>,
    by_row: HashMap<Row, RowView>,
}

impl Views {
    /// What a shown row says; nothing for a row not shown.
    pub(crate) fn get(&self, row: Row) -> Option<RowView> {
        self.by_row.get(&row).copied()
    }
}

/// How many steps of indentation a row at `level` takes: one for each level
/// below the first, up to `INDENT_CAP`, so a deep folder's rows still fit a
/// narrow screen (1.4.10, Reflow). `aria-level` says the exact level.
pub(crate) fn indent_steps(level: u32) -> u32 {
    level.saturating_sub(1).min(INDENT_CAP)
}

/// The most steps a row is indented by.
pub(crate) const INDENT_CAP: u32 = 6;

#[cfg(test)]
thread_local! {
    /// How many times the shown rows were walked, for the test that a
    /// change costs the page one walk.
    static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A snapshot's tree as the page holds it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tree {
    nodes: Vec<Node>,
    focus: Option<Row>,
    typed: String,
    typed_at: f64,
    failure: Option<Sentence>,
}

impl Default for Tree {
    fn default() -> Tree {
        Tree::new()
    }
}

impl Tree {
    /// A tree whose top folder is not read yet.
    pub(crate) fn new() -> Tree {
        let top = EntryDto {
            name: String::new(),
            display: String::new(),
            kind: EntryKindDto::Folder,
            size: None,
            modified: None,
        };
        Tree {
            nodes: vec![Node::new(top, 0, 0, 0)],
            focus: None,
            typed: String::new(),
            typed_at: f64::NEG_INFINITY,
            failure: None,
        }
    }

    /// Opens the top folder: the first page of it to ask for.
    pub(crate) fn start(&mut self) -> Option<Request> {
        self.open(0)
    }

    pub(crate) fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    /// Whether the top folder is read and holds nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.nodes
            .first()
            .is_some_and(|top| top.load == Load::Read && top.children.is_empty())
    }

    /// The refusal of the page asked for last, until another is asked for.
    pub(crate) fn failure(&self) -> Option<&Sentence> {
        self.failure.as_ref()
    }

    /// The rows shown, top to bottom: each open folder's entries under it,
    /// then the row that reads its next page while one is left.
    pub(crate) fn rows(&self) -> Vec<Row> {
        #[cfg(test)]
        WALKS.with(|walks| walks.set(walks.get() + 1));
        let mut rows = Vec::new();
        self.push_rows(0, &mut rows);
        rows
    }

    /// Every shown row and what it says (`Views`).
    pub(crate) fn views(&self) -> Views {
        let rows = self.rows();
        let stop = Self::stop_in(self.focus, &rows);
        let by_row = rows
            .iter()
            .map(|&row| {
                let (expanded, state, shown) = match row {
                    Row::Entry(index) => {
                        let expanded = self
                            .nodes
                            .get(index)
                            .filter(|node| node.is_folder())
                            .map(|node| node.open);
                        (expanded, self.state(index), 0)
                    }
                    Row::More(folder) => {
                        let shown = self
                            .nodes
                            .get(folder)
                            .map_or(0, |node| node.children.len() as u64);
                        (None, None, shown)
                    }
                };
                let view = RowView {
                    position: self.place(row).1,
                    set_size: self.set_size(row),
                    expanded,
                    busy: self.busy(row),
                    stop: stop == Some(row),
                    state,
                    shown,
                };
                (row, view)
            })
            .collect();
        Views { rows, by_row }
    }

    fn push_rows(&self, folder: usize, rows: &mut Vec<Row>) {
        let Some(node) = self.nodes.get(folder) else {
            return;
        };
        for &child in &node.children {
            rows.push(Row::Entry(child));
            if self.nodes.get(child).is_some_and(|child| child.open) {
                self.push_rows(child, rows);
            }
        }
        if node.next.is_some() {
            rows.push(Row::More(folder));
        }
    }

    /// The row that holds the tree's one tab stop: the one focus was last
    /// on while it is shown, else the first.
    pub(crate) fn tab_stop(&self) -> Option<Row> {
        Self::stop_in(self.focus, &self.rows())
    }

    /// The entry the tab stop is on: nothing before the top folder is read,
    /// nor on a row that reads more of a folder. Selection follows focus, as
    /// a single-select tree's does.
    pub(crate) fn selection(&self) -> Option<Selection> {
        let Row::Entry(index) = self.tab_stop()? else {
            return None;
        };
        let node = self.node(index)?;
        Some(Selection {
            path: self.path(index),
            name: node.entry.display.clone(),
        })
    }

    fn stop_in(focus: Option<Row>, rows: &[Row]) -> Option<Row> {
        focus
            .filter(|focus| rows.contains(focus))
            .or_else(|| rows.first().copied())
    }

    /// A row's `aria-level` and `aria-posinset`: the row that reads a
    /// folder's next page stands where that page's first entry will.
    pub(crate) fn place(&self, row: Row) -> (u32, u64) {
        match row {
            Row::Entry(index) => self
                .nodes
                .get(index)
                .map_or((0, 0), |node| (node.level, node.position)),
            Row::More(folder) => self.nodes.get(folder).map_or((0, 0), |node| {
                (
                    node.level.saturating_add(1),
                    (node.children.len() as u64).saturating_add(1),
                )
            }),
        }
    }

    /// A row's `aria-setsize`: how many entries its folder holds in all.
    pub(crate) fn set_size(&self, row: Row) -> u64 {
        let folder = match row {
            Row::Entry(index) => self.nodes.get(index).map_or(0, |node| node.parent),
            Row::More(folder) => folder,
        };
        self.nodes.get(folder).map_or(0, |node| node.total)
    }

    /// Whether a row waits for a page: a folder for its first, the row that
    /// reads the next page for that one.
    pub(crate) fn busy(&self, row: Row) -> bool {
        match row {
            Row::Entry(index) => self
                .nodes
                .get(index)
                .is_some_and(|node| node.load == Load::Loading { from: 0 }),
            Row::More(folder) => self
                .nodes
                .get(folder)
                .is_some_and(|node| matches!(node.load, Load::Loading { .. })),
        }
    }

    /// Whether the top folder's first page is on its way.
    pub(crate) fn starting(&self) -> bool {
        self.busy(Row::Entry(0))
    }

    /// What a row says after its name.
    pub(crate) fn state(&self, index: usize) -> Option<State> {
        let node = self.nodes.get(index)?;
        Some(match node.entry.kind {
            EntryKindDto::Folder => match node.load {
                Load::Loading { .. } if node.children.is_empty() => State::Loading,
                Load::Read if node.children.is_empty() && node.next.is_none() => State::Empty,
                Load::Failed => State::Failed,
                _ => State::Folder,
            },
            EntryKindDto::File => State::File,
            EntryKindDto::Link => State::Link,
            EntryKindDto::Other => State::Other,
        })
    }

    /// The path of a node in the snapshot, its names joined by `/`; empty
    /// for the top folder. The top folder is its own parent, so the walk up
    /// takes at most a step per node: no parent link can hold it.
    pub(crate) fn path(&self, index: usize) -> String {
        let mut names = Vec::new();
        let mut at = index;
        for _ in 0..self.nodes.len() {
            if at == 0 {
                break;
            }
            let Some(node) = self.nodes.get(at) else {
                break;
            };
            names.push(node.entry.name.as_str());
            at = node.parent;
        }
        names.reverse();
        names.join("/")
    }

    /// Opens a folder; its first page to ask for, unless it is read or on
    /// its way.
    fn open(&mut self, index: usize) -> Option<Request> {
        let node = self.nodes.get_mut(index)?;
        if index != 0 && !node.is_folder() {
            return None;
        }
        node.open = true;
        match node.load {
            Load::Unread | Load::Failed => {
                node.load = Load::Loading { from: 0 };
                self.failure = None;
                Some(Request {
                    folder: index,
                    path: self.path(index),
                    from: 0,
                })
            }
            Load::Loading { .. } | Load::Read => None,
        }
    }

    /// Asks for the next page of a folder, unless a page of it is on its way.
    fn more(&mut self, folder: usize) -> Option<Request> {
        let node = self.nodes.get_mut(folder)?;
        let from = node.next.filter(|_| node.load == Load::Read)?;
        node.load = Load::Loading { from };
        self.failure = None;
        Some(Request {
            folder,
            path: self.path(folder),
            from,
        })
    }

    /// A row was clicked: focus goes to it, a folder opens or closes, and
    /// the row that reads the next page reads it.
    pub(crate) fn activate(&mut self, row: Row) -> Option<Request> {
        self.focus = Some(row);
        match row {
            Row::Entry(index) => self.toggle(index),
            Row::More(folder) => self.more(folder),
        }
    }

    fn toggle(&mut self, index: usize) -> Option<Request> {
        let node = self.nodes.get_mut(index)?;
        if node.open {
            node.open = false;
            None
        } else {
            self.open(index)
        }
    }

    /// Does what `key` (a `KeyboardEvent.key` pressed without Ctrl, Alt or
    /// Meta) does on the focused row, `now_ms` being when. In a page that runs
    /// right to left the left arrow opens and the right arrow closes.
    pub(crate) fn key(&mut self, key: &str, rtl: bool, now_ms: f64) -> Pressed {
        let rows = self.rows();
        let Some(at) = Self::stop_in(self.focus, &rows) else {
            return Pressed::default();
        };
        let index = rows.iter().position(|row| *row == at).unwrap_or_default();
        let (forward, back) = if rtl {
            ("ArrowLeft", "ArrowRight")
        } else {
            ("ArrowRight", "ArrowLeft")
        };
        let mut request = None;
        match key {
            "ArrowDown" => self.focus_on(rows.get(index.saturating_add(1)).copied().or(Some(at))),
            "ArrowUp" => {
                self.focus_on(
                    index
                        .checked_sub(1)
                        .and_then(|before| rows.get(before))
                        .copied()
                        .or(Some(at)),
                );
            }
            "Home" => self.focus_on(rows.first().copied()),
            "End" => self.focus_on(rows.last().copied()),
            "Enter" | " " => {
                self.focus = Some(at);
                request = match at {
                    Row::Entry(node) => self.toggle(node),
                    Row::More(folder) => self.more(folder),
                };
            }
            _ if key == forward => {
                self.focus = Some(at);
                if let Row::Entry(node) = at {
                    request = self.forward(node, rows.get(index.saturating_add(1)).copied());
                }
            }
            _ if key == back => self.back(at),
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(typed), None) if !typed.is_control() => {
                        self.type_ahead(typed, now_ms, &rows, index);
                    }
                    _ => return Pressed::default(),
                }
            }
        }
        Pressed {
            handled: true,
            request,
        }
    }

    fn focus_on(&mut self, row: Option<Row>) {
        self.focus = row;
    }

    /// Right in a left-to-right page: opens a closed folder, and moves into
    /// an open one, to `after`, the row below it, when that is in it.
    fn forward(&mut self, index: usize, after: Option<Row>) -> Option<Request> {
        let node = self.nodes.get(index)?;
        if !node.is_folder() {
            return None;
        }
        if !node.open {
            return self.open(index);
        }
        let inside = match after {
            Some(Row::Entry(child)) => self
                .nodes
                .get(child)
                .is_some_and(|child| child.parent == index),
            Some(Row::More(folder)) => folder == index,
            None => false,
        };
        if inside {
            self.focus = after;
        }
        None
    }

    /// Left in a left-to-right page: closes an open folder, and otherwise
    /// moves to the folder the row is in.
    fn back(&mut self, at: Row) {
        let parent = match at {
            Row::Entry(index) => {
                let Some(node) = self.nodes.get_mut(index) else {
                    return;
                };
                if node.open {
                    node.open = false;
                    self.focus = Some(at);
                    return;
                }
                node.parent
            }
            Row::More(folder) => folder,
        };
        self.focus = Some(if parent == 0 { at } else { Row::Entry(parent) });
    }

    /// Moves focus to the next entry whose name starts with what was typed:
    /// a character typed within [`TYPE_AHEAD_MS`] of the last adds to the
    /// search, which then starts at the focused row; one typed later starts
    /// a new search after it. The search wraps, and ignores case.
    fn type_ahead(&mut self, typed: char, now_ms: f64, rows: &[Row], index: usize) {
        let elapsed = now_ms - self.typed_at;
        let fresh = elapsed.is_nan() || elapsed > TYPE_AHEAD_MS;
        if fresh {
            self.typed.clear();
        }
        self.typed_at = now_ms;
        if self.typed.chars().count() < TYPED_MAX {
            self.typed.extend(typed.to_lowercase());
        }
        let start = if fresh {
            index.saturating_add(1)
        } else {
            index
        };
        let found = (0..rows.len())
            .filter_map(|step| rows.get(start.saturating_add(step) % rows.len()))
            .find(|row| match row {
                Row::Entry(node) => self
                    .nodes
                    .get(*node)
                    .is_some_and(|node| node.entry.display.to_lowercase().starts_with(&self.typed)),
                Row::More(_) => false,
            });
        if let Some(found) = found {
            self.focus = Some(*found);
        }
    }

    /// Takes the daemon's answer to `request`. An answer to a request the
    /// tree no longer waits for is left alone. A page's entries join their
    /// folder; focus on the row that read it moves to the first of them. A
    /// refused first page closes its folder, to be asked again on opening;
    /// either refusal is kept for the page to say.
    pub(crate) fn answered(&mut self, request: &Request, answer: Result<FolderDto, Sentence>) {
        let Some(node) = self.nodes.get(request.folder) else {
            return;
        };
        if node.load != (Load::Loading { from: request.from }) {
            return;
        }
        let level = node.level.saturating_add(1);
        match answer {
            Ok(folder) => {
                let first = self.nodes.len();
                for (offset, entry) in (1_u64..).zip(folder.entries) {
                    let position = request.from.saturating_add(offset);
                    self.nodes
                        .push(Node::new(entry, request.folder, level, position));
                }
                let added = first..self.nodes.len();
                let Some(node) = self.nodes.get_mut(request.folder) else {
                    return;
                };
                node.children.extend(added.clone());
                node.total = folder.total.max(node.children.len() as u64);
                node.next = folder.next.filter(|next| *next > request.from);
                node.load = Load::Read;
                if self.focus == Some(Row::More(request.folder)) {
                    self.focus = if added.is_empty() {
                        (request.folder != 0).then_some(Row::Entry(request.folder))
                    } else {
                        Some(Row::Entry(first))
                    };
                }
            }
            Err(refusal) => {
                let Some(node) = self.nodes.get_mut(request.folder) else {
                    return;
                };
                if request.from == 0 {
                    node.load = Load::Failed;
                    node.open = false;
                } else {
                    node.load = Load::Read;
                }
                self.failure = Some(refusal);
            }
        }
    }
}

/// What the tree selects, which a pane beside it acts on: the entry the tab
/// stop is on, by its path in the snapshot, the names rustic records, and by
/// the name it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) path: String,
    pub(crate) name: String,
}

/// The address of a page of a folder of snapshot `snapshot` of repository
/// `alias`, under `base`: each name percent-encoded, so no name a snapshot
/// records can change which route or which folder is asked for.
pub(crate) fn folder_url(base: &str, alias: &str, snapshot: &str, path: &str, from: u64) -> String {
    let mut url = format!(
        "{base}/api/v1/repositories/{}/snapshots/{}/folder?path={}",
        encoded(alias),
        encoded(snapshot),
        encoded(path)
    );
    if from != 0 {
        url.push_str(&format!("&from={from}"));
    }
    url
}

/// `text` with every byte but an unreserved one (RFC 3986) percent-encoded.
pub(crate) fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The address of the page of snapshot `snapshot` of repository `alias`.
pub(crate) fn snapshot_href(alias: &str, snapshot: &str) -> String {
    format!("/snapshots/{}/{}", encoded(alias), encoded(snapshot))
}

#[cfg(test)]
mod tests;
