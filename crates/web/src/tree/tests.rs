use api::{EntryDto, EntryKindDto, FolderDto};

use super::*;

fn entry(name: &str, kind: EntryKindDto) -> EntryDto {
    EntryDto {
        name: name.to_owned(),
        display: name.to_owned(),
        kind,
        size: (kind == EntryKindDto::File).then_some(5),
        modified: None,
    }
}

fn folder(name: &str) -> EntryDto {
    entry(name, EntryKindDto::Folder)
}

fn file(name: &str) -> EntryDto {
    entry(name, EntryKindDto::File)
}

/// An entry whose key, the name rustic records, isn't the name it shows.
fn shown_as(key: &str, display: &str, kind: EntryKindDto) -> EntryDto {
    EntryDto {
        display: display.to_owned(),
        ..entry(key, kind)
    }
}

fn page(entries: Vec<EntryDto>, total: u64, next: Option<u64>) -> FolderDto {
    FolderDto {
        path: String::new(),
        entries,
        total,
        next,
    }
}

fn whole(entries: Vec<EntryDto>) -> FolderDto {
    let total = entries.len() as u64;
    page(entries, total, None)
}

fn refusal(text: &str) -> Sentence {
    Sentence {
        before: text.to_owned(),
        detail: None,
    }
}

/// A tree whose top folder holds `docs` (a folder), `empty` (a folder) and
/// `readme.txt`, answered.
fn opened() -> Tree {
    let mut tree = Tree::new();
    let request = tree.start().expect("the top folder is asked for");
    tree.answered(
        &request,
        Ok(whole(vec![
            folder("docs"),
            folder("empty"),
            file("readme.txt"),
        ])),
    );
    tree
}

/// The node of the row named `name`.
fn named(tree: &Tree, name: &str) -> usize {
    tree.rows()
        .into_iter()
        .find_map(|row| match row {
            Row::Entry(node) if tree.node(node).map(|n| n.entry.name.as_str()) == Some(name) => {
                Some(node)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

/// The names of the rows, `+` for the row that reads a next page.
fn names(tree: &Tree) -> Vec<String> {
    tree.rows()
        .into_iter()
        .map(|row| match row {
            Row::Entry(node) => tree.node(node).expect("a node").entry.name.clone(),
            Row::More(_) => "+".to_owned(),
        })
        .collect()
}

/// The name of the row holding the tab stop.
fn focused(tree: &Tree) -> String {
    match tree.tab_stop() {
        Some(Row::Entry(node)) => tree.node(node).expect("a node").entry.name.clone(),
        Some(Row::More(_)) => "+".to_owned(),
        None => "-".to_owned(),
    }
}

fn press(tree: &mut Tree, key: &str) -> Pressed {
    tree.key(key, false, 0.0)
}

/// Presses the right arrow on `name`'s row and answers the page it asks for
/// with `answer`.
fn open_with(tree: &mut Tree, name: &str, answer: FolderDto) {
    focus_on(tree, name);
    let request = press(tree, "ArrowRight")
        .request
        .expect("a page is asked for");
    tree.answered(&request, Ok(answer));
}

fn focus_on(tree: &mut Tree, name: &str) {
    let node = named(tree, name);
    tree.focus = Some(Row::Entry(node));
}

#[test]
fn starting_asks_for_the_top_folder_from_its_first_entry() {
    let mut tree = Tree::new();
    assert_eq!(
        tree.start(),
        Some(Request {
            folder: 0,
            path: String::new(),
            from: 0
        })
    );
    assert_eq!(tree.rows(), []);
    assert_eq!(tree.tab_stop(), None);
    assert_eq!(tree.start(), None, "asked once while on its way");
}

#[test]
fn the_top_folder_s_entries_are_the_rows_at_level_one_and_the_first_holds_the_tab_stop() {
    let tree = opened();
    assert_eq!(names(&tree), ["docs", "empty", "readme.txt"]);
    assert_eq!(focused(&tree), "docs");
    let readme = tree.node(named(&tree, "readme.txt")).expect("a node");
    assert_eq!((readme.level, readme.position), (1, 3));
    assert_eq!(tree.node(0).expect("the top").total, 3);
    assert!(!tree.is_empty());
}

#[test]
fn a_snapshot_holding_nothing_is_empty_once_read() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    assert!(!tree.is_empty(), "not before it is read");
    tree.answered(&request, Ok(whole(Vec::new())));
    assert!(tree.is_empty());
    assert_eq!(tree.tab_stop(), None);
}

#[test]
fn the_arrows_move_between_rows_and_stop_at_either_end() {
    let mut tree = opened();
    assert!(press(&mut tree, "ArrowUp").handled);
    assert_eq!(focused(&tree), "docs");
    press(&mut tree, "ArrowDown");
    assert_eq!(focused(&tree), "empty");
    press(&mut tree, "ArrowDown");
    assert_eq!(focused(&tree), "readme.txt");
    press(&mut tree, "ArrowDown");
    assert_eq!(focused(&tree), "readme.txt");
    press(&mut tree, "ArrowUp");
    assert_eq!(focused(&tree), "empty");
}

#[test]
fn home_and_end_move_to_the_first_and_last_rows() {
    let mut tree = opened();
    assert!(press(&mut tree, "End").handled);
    assert_eq!(focused(&tree), "readme.txt");
    assert!(press(&mut tree, "Home").handled);
    assert_eq!(focused(&tree), "docs");
}

#[test]
fn right_opens_a_folder_by_asking_for_its_first_page_then_moves_into_it() {
    let mut tree = opened();
    let pressed = press(&mut tree, "ArrowRight");
    let request = pressed.request.clone().expect("asked");
    assert_eq!(
        pressed,
        Pressed {
            handled: true,
            request: Some(Request {
                folder: named(&tree, "docs"),
                path: "docs".to_owned(),
                from: 0
            })
        }
    );
    let docs = named(&tree, "docs");
    assert!(tree.node(docs).expect("docs").open);
    assert_eq!(tree.state(docs), Some(State::Loading));
    assert_eq!(press(&mut tree, "ArrowRight").request, None, "on its way");
    assert_eq!(focused(&tree), "docs", "nothing to move into yet");
    tree.answered(&request, Ok(whole(vec![file("a.md"), folder("drafts")])));
    assert_eq!(
        names(&tree),
        ["docs", "a.md", "drafts", "empty", "readme.txt"]
    );
    let drafts = tree.node(named(&tree, "drafts")).expect("drafts");
    assert_eq!((drafts.level, drafts.position), (2, 2));
    assert_eq!(press(&mut tree, "ArrowRight").request, None);
    assert_eq!(focused(&tree), "a.md");
}

#[test]
fn a_folder_s_path_joins_the_names_above_it() {
    let mut tree = opened();
    open_with(&mut tree, "docs", whole(vec![folder("drafts")]));
    focus_on(&mut tree, "drafts");
    let request = press(&mut tree, "ArrowRight").request.expect("asked");
    assert_eq!(request.path, "docs/drafts");
}

#[test]
fn a_folder_s_path_joins_the_names_rustic_records_never_those_shown() {
    let mut tree = opened();
    open_with(
        &mut tree,
        "docs",
        whole(vec![shown_as(
            "say \\\"hi\\\"",
            "say \"hi\"",
            EntryKindDto::Folder,
        )]),
    );
    focus_on(&mut tree, "say \\\"hi\\\"");
    let request = press(&mut tree, "ArrowRight").request.expect("asked");
    assert_eq!(request.path, "docs/say \\\"hi\\\"");
}

#[test]
fn right_on_an_open_folder_that_holds_nothing_stays() {
    let mut tree = opened();
    open_with(&mut tree, "empty", whole(Vec::new()));
    press(&mut tree, "ArrowRight");
    assert_eq!(focused(&tree), "empty");
    assert_eq!(tree.state(named(&tree, "empty")), Some(State::Empty));
}

#[test]
fn right_on_the_last_open_folder_with_nothing_below_stays() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(&request, Ok(whole(vec![folder("only")])));
    open_with(&mut tree, "only", whole(Vec::new()));
    press(&mut tree, "ArrowRight");
    assert_eq!(focused(&tree), "only");
}

#[test]
fn right_on_an_open_folder_that_holds_nothing_stays_above_its_parent_s_row_that_reads_more() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(&request, Ok(page(vec![folder("only")], 2, Some(1))));
    open_with(&mut tree, "only", whole(Vec::new()));
    assert_eq!(names(&tree), ["only", "+"]);
    press(&mut tree, "ArrowRight");
    assert_eq!(focused(&tree), "only", "the row below is not in it");
}

#[test]
fn right_on_a_file_does_nothing_but_is_taken() {
    let mut tree = opened();
    focus_on(&mut tree, "readme.txt");
    assert_eq!(
        press(&mut tree, "ArrowRight"),
        Pressed {
            handled: true,
            request: None
        }
    );
    assert_eq!(focused(&tree), "readme.txt");
}

#[test]
fn left_closes_an_open_folder_and_from_inside_moves_to_it() {
    let mut tree = opened();
    open_with(&mut tree, "docs", whole(vec![file("a.md")]));
    focus_on(&mut tree, "a.md");
    press(&mut tree, "ArrowLeft");
    assert_eq!(focused(&tree), "docs");
    press(&mut tree, "ArrowLeft");
    assert!(!tree.node(named(&tree, "docs")).expect("docs").open);
    assert_eq!(names(&tree), ["docs", "empty", "readme.txt"]);
    press(&mut tree, "ArrowLeft");
    assert_eq!(focused(&tree), "docs", "the top folder is no row");
    assert_eq!(press(&mut tree, "ArrowRight").request, None, "read once");
    assert_eq!(names(&tree), ["docs", "a.md", "empty", "readme.txt"]);
}

#[test]
fn left_on_the_row_that_reads_more_moves_to_its_folder() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a.md")], 2, Some(1)));
    press(&mut tree, "ArrowDown");
    press(&mut tree, "ArrowDown");
    assert_eq!(focused(&tree), "+");
    press(&mut tree, "ArrowLeft");
    assert_eq!(focused(&tree), "docs");
}

#[test]
fn left_on_the_top_folder_s_row_that_reads_more_stays() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(&request, Ok(page(vec![file("a.md")], 2, Some(1))));
    press(&mut tree, "End");
    press(&mut tree, "ArrowLeft");
    assert_eq!(focused(&tree), "+");
}

#[test]
fn right_to_left_swaps_the_arrows_that_open_and_close() {
    let mut tree = opened();
    assert_eq!(tree.key("ArrowRight", true, 0.0).request, None);
    let docs = named(&tree, "docs");
    assert!(!tree.node(docs).expect("docs").open, "right closes");
    let request = tree
        .key("ArrowLeft", true, 0.0)
        .request
        .expect("left opens");
    tree.answered(&request, Ok(whole(vec![file("a.md")])));
    tree.key("ArrowLeft", true, 0.0);
    assert_eq!(focused(&tree), "a.md");
    tree.key("ArrowRight", true, 0.0);
    assert_eq!(focused(&tree), "docs");
}

#[test]
fn enter_and_space_open_and_close_a_folder() {
    for key in ["Enter", " "] {
        let mut tree = opened();
        let request = press(&mut tree, key).request.expect("asked");
        assert_eq!(request.path, "docs", "{key:?}");
        tree.answered(&request, Ok(whole(vec![file("a.md")])));
        assert!(press(&mut tree, key).handled);
        assert!(
            !tree.node(named(&tree, "docs")).expect("docs").open,
            "{key:?}"
        );
        assert_eq!(press(&mut tree, key).request, None, "{key:?}: read once");
        assert!(
            tree.node(named(&tree, "docs")).expect("docs").open,
            "{key:?}"
        );
    }
}

#[test]
fn a_key_the_tree_has_no_use_for_is_left_to_the_browser() {
    let mut tree = opened();
    for key in ["Tab", "Escape", "PageDown", "\u{7}"] {
        assert_eq!(press(&mut tree, key), Pressed::default(), "{key:?}");
    }
    assert_eq!(
        press(&mut Tree::new(), "ArrowDown"),
        Pressed::default(),
        "no rows yet"
    );
}

#[test]
fn a_typed_character_moves_to_the_next_name_it_starts_ignoring_case_and_wrapping() {
    let mut tree = opened();
    assert!(tree.key("R", false, 0.0).handled);
    assert_eq!(focused(&tree), "readme.txt");
    tree.key("d", false, 1_000.0);
    assert_eq!(focused(&tree), "docs", "wrapped");
    tree.key("x", false, 2_000.0);
    assert_eq!(focused(&tree), "docs", "nothing starts so");
}

#[test]
fn a_typed_character_searches_the_names_shown() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(
        &request,
        Ok(whole(vec![
            file("alpha"),
            shown_as("\\\"quoted\\\"", "\"quoted\"", EntryKindDto::File),
            shown_as("tab\\there", "tab\u{2409}here", EntryKindDto::File),
        ])),
    );
    tree.key("\"", false, 0.0);
    assert_eq!(focused(&tree), "\\\"quoted\\\"", "by its shown name");
    tree.key("\\", false, 2_000.0);
    assert_eq!(focused(&tree), "\\\"quoted\\\"", "no name shown starts so");
    for (at, typed) in ["t", "a", "b", "\u{2409}"].into_iter().enumerate() {
        tree.key(typed, false, 4_000.0 + at as f64);
    }
    assert_eq!(focused(&tree), "tab\\there", "the tab as it shows");
}

#[test]
fn characters_typed_close_together_add_up_to_one_search() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(
        &request,
        Ok(whole(vec![file("alpha"), file("beta"), file("bravo")])),
    );
    tree.key("b", false, 0.0);
    assert_eq!(focused(&tree), "beta");
    tree.key("r", false, TYPE_AHEAD_MS);
    assert_eq!(focused(&tree), "bravo", "\"br\", from the focused row");
    tree.key("b", false, TYPE_AHEAD_MS * 2.0 + 1.0);
    assert_eq!(
        focused(&tree),
        "beta",
        "a new search, after the focused row"
    );
}

#[test]
fn a_search_holds_at_most_its_cap_of_characters() {
    let mut tree = opened();
    for at in 0..100 {
        tree.key("r", false, f64::from(at));
    }
    assert_eq!(tree.typed.chars().count(), TYPED_MAX);
}

#[test]
fn a_page_of_a_folder_ends_in_a_row_that_reads_the_next() {
    let mut tree = opened();
    open_with(
        &mut tree,
        "docs",
        page(vec![file("a"), file("b")], 3, Some(2)),
    );
    assert_eq!(names(&tree), ["docs", "a", "b", "+", "empty", "readme.txt"]);
    let docs = named(&tree, "docs");
    assert_eq!(tree.node(docs).expect("docs").total, 3);
    tree.focus = Some(Row::More(docs));
    let request = press(&mut tree, "Enter").request.expect("the next page");
    assert_eq!(
        request,
        Request {
            folder: docs,
            path: "docs".to_owned(),
            from: 2
        }
    );
    assert_eq!(press(&mut tree, "Enter").request, None, "on its way");
    tree.answered(&request, Ok(page(vec![file("c")], 3, None)));
    assert_eq!(names(&tree), ["docs", "a", "b", "c", "empty", "readme.txt"]);
    assert_eq!(focused(&tree), "c", "from the row that went");
    let c = tree.node(named(&tree, "c")).expect("c");
    assert_eq!((c.level, c.position), (2, 3));
}

#[test]
fn clicking_a_row_focuses_it_and_does_what_enter_does() {
    let mut tree = opened();
    let readme = named(&tree, "readme.txt");
    assert_eq!(tree.activate(Row::Entry(readme)), None);
    assert_eq!(focused(&tree), "readme.txt");
    let docs = named(&tree, "docs");
    let request = tree.activate(Row::Entry(docs)).expect("opens");
    tree.answered(&request, Ok(page(vec![file("a")], 2, Some(1))));
    let more = tree.activate(Row::More(docs)).expect("the next page");
    assert_eq!(more.from, 1);
    assert_eq!(tree.activate(Row::Entry(docs)), None);
    assert!(!tree.node(docs).expect("docs").open, "closes");
}

#[test]
fn an_empty_next_page_moves_focus_from_its_row_to_its_folder() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a")], 1, Some(1)));
    let docs = named(&tree, "docs");
    let request = tree.activate(Row::More(docs)).expect("asked");
    tree.answered(&request, Ok(page(Vec::new(), 1, None)));
    assert_eq!(focused(&tree), "docs");
    assert_eq!(
        tree.state(docs),
        Some(State::Folder),
        "not empty: it holds a"
    );
}

#[test]
fn an_empty_next_page_of_the_top_folder_leaves_the_stop_on_the_first_row() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(&request, Ok(page(vec![file("a")], 1, Some(1))));
    let request = tree.activate(Row::More(0)).expect("asked");
    tree.answered(&request, Ok(page(Vec::new(), 1, None)));
    assert_eq!(tree.focus, None);
    assert_eq!(focused(&tree), "a");
}

#[test]
fn the_top_folder_s_next_page_is_asked_for_at_its_empty_path_with_folders_below_read() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(
        &request,
        Ok(page(vec![folder("docs"), file("b")], 3, Some(2))),
    );
    open_with(&mut tree, "docs", whole(vec![file("a.md")]));
    let request = tree.activate(Row::More(0)).expect("asked");
    assert_eq!(request.path, "");
}

#[test]
fn a_next_page_that_starts_no_later_ends_the_folder() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a")], 9, Some(0)));
    assert_eq!(tree.node(named(&tree, "docs")).expect("docs").next, None);
    assert_eq!(names(&tree), ["docs", "a", "empty", "readme.txt"]);
}

#[test]
fn a_total_below_what_was_read_counts_what_was_read() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a"), file("b")], 1, None));
    assert_eq!(tree.node(named(&tree, "docs")).expect("docs").total, 2);
}

#[test]
fn an_answer_the_tree_no_longer_waits_for_is_left_alone() {
    let mut tree = opened();
    let docs = named(&tree, "docs");
    let stale = Request {
        folder: docs,
        path: "docs".to_owned(),
        from: 0,
    };
    tree.answered(&stale, Ok(whole(vec![file("a")])));
    assert_eq!(names(&tree), ["docs", "empty", "readme.txt"], "never asked");
    let request = press(&mut tree, "ArrowRight").request.expect("asked");
    let other_page = Request {
        from: 7,
        ..request.clone()
    };
    tree.answered(&other_page, Ok(whole(vec![file("x")])));
    tree.answered(&request, Err(refusal("no")));
    assert_eq!(tree.state(docs), Some(State::Failed));
    let missing = Request {
        folder: 99,
        path: String::new(),
        from: 0,
    };
    tree.answered(&missing, Ok(whole(vec![file("y")])));
    assert_eq!(names(&tree), ["docs", "empty", "readme.txt"]);
}

#[test]
fn a_refused_folder_closes_says_why_and_is_asked_again_on_opening() {
    let mut tree = opened();
    let request = press(&mut tree, "ArrowRight").request.expect("asked");
    tree.answered(&request, Err(refusal("The snapshot has no folder docs.")));
    let docs = named(&tree, "docs");
    assert!(!tree.node(docs).expect("docs").open);
    assert_eq!(tree.state(docs), Some(State::Failed));
    assert_eq!(
        tree.failure(),
        Some(&refusal("The snapshot has no folder docs."))
    );
    let again = press(&mut tree, "ArrowRight").request.expect("asked again");
    assert_eq!(again.from, 0);
    assert_eq!(tree.failure(), None, "until the next answer");
}

#[test]
fn a_refused_next_page_keeps_what_was_read_and_can_be_asked_again() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a")], 2, Some(1)));
    let docs = named(&tree, "docs");
    let request = tree.activate(Row::More(docs)).expect("asked");
    tree.answered(&request, Err(refusal("busy")));
    assert!(tree.node(docs).expect("docs").open);
    assert_eq!(names(&tree), ["docs", "a", "+", "empty", "readme.txt"]);
    assert_eq!(tree.failure(), Some(&refusal("busy")));
    assert!(tree.activate(Row::More(docs)).is_some());
    assert_eq!(tree.failure(), None);
}

#[test]
fn a_refused_top_folder_shows_nothing_and_is_not_empty() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(&request, Err(refusal("gone")));
    assert_eq!(tree.rows(), []);
    assert!(!tree.is_empty());
    assert_eq!(tree.failure(), Some(&refusal("gone")));
}

#[test]
fn each_kind_of_entry_says_what_it_is() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    tree.answered(
        &request,
        Ok(whole(vec![
            folder("f"),
            file("x"),
            entry("l", EntryKindDto::Link),
            entry("o", EntryKindDto::Other),
        ])),
    );
    let states = ["f", "x", "l", "o"].map(|name| tree.state(named(&tree, name)));
    assert_eq!(
        states,
        [
            Some(State::Folder),
            Some(State::File),
            Some(State::Link),
            Some(State::Other)
        ]
    );
    assert_eq!(tree.state(99), None);
}

#[test]
fn a_folder_read_with_more_left_is_not_empty() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(Vec::new(), 4, Some(1)));
    assert_eq!(tree.state(named(&tree, "docs")), Some(State::Folder));
}

#[test]
fn a_folder_loading_its_next_page_is_still_a_folder() {
    let mut tree = opened();
    open_with(&mut tree, "docs", page(vec![file("a")], 2, Some(1)));
    let docs = named(&tree, "docs");
    tree.activate(Row::More(docs));
    assert_eq!(tree.state(docs), Some(State::Folder));
}

#[test]
fn each_state_has_its_word_for_the_message() {
    let words = [
        State::Folder,
        State::Loading,
        State::Empty,
        State::Failed,
        State::File,
        State::Link,
        State::Other,
    ]
    .map(State::kind);
    assert_eq!(
        words,
        [
            "folder", "loading", "empty", "failed", "file", "link", "other"
        ]
    );
}

#[test]
fn a_file_is_not_a_folder_to_open() {
    let mut tree = opened();
    let readme = named(&tree, "readme.txt");
    assert_eq!(tree.open(readme), None);
    assert!(!tree.node(readme).expect("readme").open);
    assert!(tree.node(named(&tree, "docs")).expect("docs").is_folder());
}

#[test]
fn a_folder_page_s_address_encodes_every_name() {
    assert_eq!(
        folder_url("http://host", "my repo", "ab", "docs/a b&c=%", 0),
        "http://host/api/v1/repositories/my%20repo/snapshots/ab/folder?path=docs%2Fa%20b%26c%3D%25"
    );
    assert_eq!(
        folder_url("", "demo", "ab", "", 1000),
        "/api/v1/repositories/demo/snapshots/ab/folder?path=&from=1000"
    );
    assert_eq!(encoded("Az09-._~é"), "Az09-._~%C3%A9");
}

#[test]
fn a_snapshot_s_address_encodes_its_repository() {
    assert_eq!(snapshot_href("a/b", "ff"), "/snapshots/a%2Fb/ff");
}

#[test]
fn each_row_has_its_level_position_and_its_folder_s_size() {
    let mut tree = opened();
    open_with(
        &mut tree,
        "docs",
        page(vec![file("a"), file("b")], 5, Some(2)),
    );
    let docs = named(&tree, "docs");
    let b = named(&tree, "b");
    assert_eq!(tree.place(Row::Entry(docs)), (1, 1));
    assert_eq!(tree.place(Row::Entry(b)), (2, 2));
    assert_eq!(tree.place(Row::More(docs)), (2, 3));
    assert_eq!(tree.set_size(Row::Entry(docs)), 3);
    assert_eq!(tree.set_size(Row::Entry(b)), 5);
    assert_eq!(tree.set_size(Row::More(docs)), 5);
    assert_eq!(tree.place(Row::Entry(99)), (0, 0));
    assert_eq!(tree.place(Row::More(99)), (0, 0));
    assert_eq!(tree.set_size(Row::Entry(99)), 3, "the top folder's");
    assert_eq!(tree.set_size(Row::More(99)), 0);
}

#[test]
fn a_row_is_busy_while_the_page_it_waits_for_is_on_its_way() {
    let mut tree = Tree::new();
    let request = tree.start().expect("asked");
    assert!(tree.starting());
    tree.answered(&request, Ok(whole(vec![folder("docs")])));
    assert!(!tree.starting());
    let docs = named(&tree, "docs");
    let request = press(&mut tree, "ArrowRight").request.expect("asked");
    assert!(tree.busy(Row::Entry(docs)));
    tree.answered(&request, Ok(page(vec![file("a")], 2, Some(1))));
    assert!(!tree.busy(Row::Entry(docs)));
    assert!(!tree.busy(Row::More(docs)));
    tree.activate(Row::More(docs));
    assert!(tree.busy(Row::More(docs)));
    assert!(!tree.busy(Row::Entry(docs)), "its first page is in");
    assert!(!tree.busy(Row::Entry(99)));
    assert!(!tree.busy(Row::More(99)));
}

#[test]
fn every_shown_row_s_view_comes_from_one_walk_of_the_tree() {
    let mut tree = opened();
    let files = (0..300).map(|i| file(&format!("f{i}"))).collect();
    open_with(&mut tree, "docs", page(files, 400, Some(300)));
    focus_on(&mut tree, "empty");
    assert!(press(&mut tree, "ArrowRight").request.is_some());
    focus_on(&mut tree, "f150");
    let docs = named(&tree, "docs");
    let walks = || WALKS.with(std::cell::Cell::get);
    let before = walks();
    let views = tree.views();
    assert_eq!(
        walks() - before,
        1,
        "one walk for all {} rows, not one a row",
        views.rows.len()
    );
    let rows = tree.rows();
    assert_eq!(views.rows, rows);
    let stop = tree.tab_stop();
    for &row in &rows {
        let (expanded, state, shown) = match row {
            Row::Entry(index) => {
                let node = tree.node(index).expect("a node");
                (node.is_folder().then_some(node.open), tree.state(index), 0)
            }
            Row::More(folder) => {
                let node = tree.node(folder).expect("a folder");
                (None, None, node.children.len() as u64)
            }
        };
        let expected = RowView {
            position: tree.place(row).1,
            set_size: tree.set_size(row),
            expanded,
            busy: tree.busy(row),
            stop: stop == Some(row),
            state,
            shown,
        };
        assert_eq!(views.get(row), Some(expected), "{row:?}");
    }
    let f150 = Row::Entry(named(&tree, "f150"));
    assert!(views.get(f150).is_some_and(|view| view.stop));
    let empty = views.get(Row::Entry(named(&tree, "empty")));
    assert!(empty.is_some_and(|view| view.busy && view.expanded == Some(true)));
    assert_eq!(empty.and_then(|view| view.state), Some(State::Loading));
    let more = views.get(Row::More(docs)).expect("docs reads more");
    assert_eq!((more.position, more.set_size, more.shown), (301, 400, 300));
    let readme = views.get(Row::Entry(named(&tree, "readme.txt")));
    assert_eq!(readme.map(|view| view.expanded), Some(None));
    assert_eq!(views.get(Row::More(named(&tree, "readme.txt"))), None);
}

#[test]
fn a_row_is_indented_a_step_for_each_level_below_the_first_up_to_a_cap() {
    assert_eq!(indent_steps(0), 0);
    assert_eq!(indent_steps(1), 0);
    assert_eq!(indent_steps(2), 1);
    assert_eq!(indent_steps(INDENT_CAP), INDENT_CAP - 1);
    assert_eq!(indent_steps(INDENT_CAP + 1), INDENT_CAP);
    assert_eq!(indent_steps(INDENT_CAP + 2), INDENT_CAP);
    assert_eq!(indent_steps(u32::MAX), INDENT_CAP);
}
