use l10n::Localizer;

use super::*;
use crate::routes::text_with;

fn list(folders: &[&str]) -> Vec<String> {
    folders.iter().map(|folder| (*folder).to_owned()).collect()
}

/// What `said` reads in the page's default language, with the marks that
/// isolate each value taken out, as a screen reader says it.
fn shown((id, args): Said) -> String {
    text_with(&Localizer::new(&[]), id, &args).replace(['\u{2068}', '\u{2069}'], "")
}

/// What the message `id` reads, without values.
fn plain(id: MessageId) -> String {
    shown((id, Vec::new()))
}

fn dto(folders: &[&str], home: Option<&str>) -> DryRunAllowlistDto {
    DryRunAllowlistDto {
        folders: list(folders),
        home: home.map(str::to_owned),
    }
}

fn entry(position: u64) -> Vec<(String, MsgArg)> {
    vec![("entry".to_owned(), MsgArg::Count(position))]
}

#[test]
fn each_folder_is_one_row_in_the_order_the_list_holds_it_even_when_the_file_lists_it_twice() {
    let folders = list(&[
        "/home/demo/Pictures",
        "/home/demo/Documents",
        "/home/demo/Pictures",
    ]);
    assert_eq!(
        rows(&folders),
        list(&["/home/demo/Pictures", "/home/demo/Documents"])
    );
    assert_eq!(rows(&[]), Vec::<String>::new());
}

#[test]
fn a_folder_typed_is_added_at_the_end_without_the_spaces_around_it() {
    let folders = list(&["/home/demo/Documents"]);
    assert_eq!(
        with_added(&folders, "  /home/demo/Pictures\n"),
        Ok(list(&["/home/demo/Documents", "/home/demo/Pictures"]))
    );
}

#[test]
fn a_folder_typed_keeps_the_spaces_inside_it() {
    assert_eq!(
        with_added(&[], "/home/demo/Family photos"),
        Ok(list(&["/home/demo/Family photos"]))
    );
}

#[test]
fn nothing_typed_or_only_spaces_asks_for_the_folder_s_full_path() {
    for typed in ["", "   ", "\t\n"] {
        let lack = with_added(&[], typed).unwrap_err();
        assert_eq!(
            plain(lack.said()),
            "Give the folder's full path.",
            "{typed:?}"
        );
    }
}

#[test]
fn a_folder_the_list_holds_already_is_not_added_again_and_the_field_says_so() {
    let folders = list(&["/home/demo/Documents"]);
    let lack = with_added(&folders, " /home/demo/Documents ").unwrap_err();
    assert_eq!(plain(lack.said()), "The list holds this folder already.");
}

#[test]
fn a_folder_that_only_starts_like_one_listed_is_added() {
    let folders = list(&["/home/demo/Documents"]);
    assert_eq!(
        with_added(&folders, "/home/demo/Documents-old"),
        Ok(list(&["/home/demo/Documents", "/home/demo/Documents-old"]))
    );
}

#[test]
fn a_folder_typed_that_is_the_home_folder_or_holds_it_warns_first() {
    let home = Some("/home/demo");
    for typed in ["/home/demo", " /home/demo/ ", "/home", "/home/", "/", "//"] {
        assert!(opens_home(typed, home), "{typed:?}");
    }
    assert!(opens_home("/home/demo", Some("/home/demo/")));
}

#[test]
fn a_folder_typed_inside_the_home_folder_or_beside_it_does_not_warn() {
    let home = Some("/home/demo");
    for typed in [
        "/home/demo/Documents",
        "/home/demo-old",
        "/home/dem",
        "/srv",
        "Documents",
        "home/demo",
    ] {
        assert!(!opens_home(typed, home), "{typed:?}");
    }
}

#[test]
fn no_folder_typed_warns_where_the_daemon_names_no_home_folder() {
    assert!(!opens_home("/", None));
    assert!(!opens_home("/home/demo", Some("")));
}

#[test]
fn removing_a_folder_removes_each_entry_of_it_and_keeps_the_others_in_order() {
    let folders = list(&[
        "/home/demo/Pictures",
        "/home/demo/Documents",
        "/home/demo/Pictures",
        "/home/demo/Music",
    ]);
    assert_eq!(
        without(&folders, "/home/demo/Pictures"),
        list(&["/home/demo/Documents", "/home/demo/Music"])
    );
    assert_eq!(without(&folders, "/home/demo"), folders);
}

#[test]
fn the_home_folder_is_offered_until_the_list_holds_it() {
    assert_eq!(
        home_offered(&list(&["/home/demo/Documents"]), Some("/home/demo")),
        Some("/home/demo".to_owned())
    );
    assert_eq!(
        home_offered(
            &list(&["/home/demo/Documents", "/home/demo"]),
            Some("/home/demo")
        ),
        None
    );
}

#[test]
fn no_home_folder_is_offered_where_the_daemon_names_none() {
    assert_eq!(home_offered(&[], None), None);
    assert_eq!(home_offered(&[], Some("")), None);
}

#[test]
fn a_save_s_answer_that_names_no_home_folder_keeps_the_one_read_first() {
    let mut held = Listed::read(dto(&[], Some("/home/demo")));
    held.take(dto(&["/home/demo/Documents"], None));
    assert_eq!(
        held,
        Listed::read(dto(&["/home/demo/Documents"], Some("/home/demo")))
    );
}

#[test]
fn a_save_s_answer_that_names_a_home_folder_replaces_the_list_and_the_home_folder() {
    let mut held = Listed::read(dto(&["/home/demo/Pictures"], Some("/home/demo")));
    held.take(dto(&["/home/demo/Documents"], Some("/home/other")));
    assert_eq!(
        held,
        Listed::read(dto(&["/home/demo/Documents"], Some("/home/other")))
    );
}

#[test]
fn a_folder_typed_that_is_not_a_full_path_is_the_field_s_error_naming_it_with_a_full_path_to_follow()
 {
    let saved = list(&["/home/demo/Pictures", "/home/demo/Music", "Documents"]);
    let error = field_refusal(
        ErrorCode::DryRunFolderInvalid,
        &entry(3),
        &saved,
        Some("/home/demo"),
    );
    assert_eq!(
        error.map(shown).as_deref(),
        Some(
            "Documents is not a full path. Give the folder's whole path, such as /home/demo/Documents."
        )
    );
}

#[test]
fn the_full_path_suggested_follows_the_home_folder_s_own_separator() {
    let saved = list(&["Documents"]);
    let shown_for = |home| {
        field_refusal(
            ErrorCode::DryRunFolderInvalid,
            &entry(1),
            &saved,
            Some(home),
        )
        .map(shown)
    };
    assert!(
        shown_for("/home/demo/")
            .unwrap()
            .ends_with("such as /home/demo/Documents.")
    );
    assert!(
        shown_for(r"C:\Users\demo")
            .unwrap()
            .ends_with(r"such as C:\Users\demo\Documents.")
    );
}

#[test]
fn a_home_folder_that_uses_slashes_keeps_them_though_a_name_in_it_has_a_backslash() {
    let saved = list(&["Documents"]);
    let error = field_refusal(
        ErrorCode::DryRunFolderInvalid,
        &entry(1),
        &saved,
        Some(r"/home/demo\old"),
    );
    assert!(
        error
            .map(shown)
            .unwrap()
            .ends_with(r"such as /home/demo\old/Documents.")
    );
}

#[test]
fn without_a_home_folder_the_field_s_error_still_names_the_folder_typed() {
    let saved = list(&["Documents"]);
    let error = field_refusal(ErrorCode::DryRunFolderInvalid, &entry(1), &saved, None);
    assert_eq!(
        error.map(shown).as_deref(),
        Some(
            "Documents is not a full path. Give the folder's whole path, starting at the top of its drive."
        )
    );
}

#[test]
fn a_refusal_about_another_folder_or_of_another_kind_is_not_the_field_s() {
    let saved = list(&["notes", "/home/demo/Documents"]);
    let home = Some("/home/demo");
    assert_eq!(
        field_refusal(ErrorCode::DryRunFolderInvalid, &entry(1), &saved, home),
        None
    );
    assert_eq!(
        field_refusal(ErrorCode::DryRunFolderInvalid, &[], &saved, home),
        None
    );
    assert_eq!(
        field_refusal(ErrorCode::DryRunAllowlistTooLong, &entry(2), &saved, home),
        None
    );
}

#[test]
fn a_list_too_long_is_said_in_the_page_s_words_which_name_no_save_control() {
    let words = alert_words(ErrorCode::DryRunAllowlistTooLong).unwrap();
    let said = shown((words, vec![("limit", MsgArg::Count(64))]));
    assert_eq!(
        said,
        "The dry-run allowlist may hold at most 64 folders, so the change was not saved. Remove a folder you no longer need first."
    );
    assert_eq!(alert_words(ErrorCode::ConfigUnavailable), None);
}

#[test]
fn the_list_is_saved_whole_and_names_no_home_folder() {
    let saved = request(list(&["/home/demo/Documents"]));
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::json!({ "folders": ["/home/demo/Documents"] })
    );
}

#[test]
fn each_folder_s_remove_button_has_an_id_of_its_own_whatever_its_path_holds() {
    let ids: Vec<String> = [
        "/home/demo/Documents",
        "/home/demo/Family photos",
        "/home/demo/Family-photos",
        "/home/demo/Fotos-ü",
    ]
    .iter()
    .map(|folder| remove_id(folder))
    .collect();
    for (at, id) in ids.iter().enumerate() {
        assert!(id.starts_with("dry-run-folder-remove-"), "{id}");
        assert!(!id.chars().any(char::is_whitespace), "{id}");
        assert!(ids.iter().skip(at + 1).all(|other| other != id), "{ids:?}");
    }
    assert_eq!(remove_id("/a"), remove_id("/a"));
}

#[test]
fn a_remove_button_s_id_spells_each_byte_of_the_folder_high_digit_first() {
    assert_eq!(remove_id("/a"), "dry-run-folder-remove-2f61");
    // `a` and `q` differ only in their high digit.
    assert_ne!(remove_id("/a"), remove_id("/q"));
}

#[test]
fn a_folder_s_button_dialog_and_announcements_name_it() {
    let folder = "/home/demo/Documents";
    assert_eq!(shown(remove_label(folder)), "Remove /home/demo/Documents");
    assert_eq!(
        shown(remove_title(folder)),
        "Remove /home/demo/Documents from the dry-run allowlist"
    );
    assert_eq!(
        shown(removed(folder)),
        "Folder /home/demo/Documents removed from the dry-run allowlist."
    );
    assert_eq!(
        shown(added(folder)),
        "Folder /home/demo/Documents added to the dry-run allowlist."
    );
}

#[test]
fn the_home_folder_s_warning_names_it_and_isolates_the_keys_folder_as_a_path() {
    let said = home_body("/home/demo");
    assert!(
        shown(said.clone()).starts_with(
            "This lets an AI agent count the files under your whole home folder, /home/demo, including ~/.ssh and browser profiles."
        )
    );
    // The page turns isolation on, as an interface that mixes in
    // right-to-left text does.
    let isolated = text_with(&Localizer::new(&[]).with_isolation(true), said.0, &said.1);
    assert!(isolated.contains("\u{2068}~/.ssh\u{2069}"), "{isolated}");
}

/// The section once the daemon's read answered `folders` and `home`.
fn read_as(folders: &[&str], home: Option<&str>) -> Allowlist {
    let section = Allowlist::new("http://127.0.0.1:4100".to_owned());
    section.held.set(Some(Listed::read(dto(folders, home))));
    section
}

#[test]
fn the_list_is_read_and_saved_at_the_daemon_s_own_origin() {
    assert_eq!(
        Allowlist::new("http://127.0.0.1:4100".to_owned()).url(),
        "http://127.0.0.1:4100/api/v1/settings/dry-run-allowlist"
    );
}

#[test]
fn a_list_not_read_yet_holds_no_folder_and_names_no_home_folder() {
    let section = Allowlist::new(String::new());
    assert_eq!(section.current(), Vec::<String>::new());
    assert_eq!(section.home(), None);
}

#[test]
fn a_list_read_holds_its_folders_in_order_and_the_daemon_s_home_folder() {
    let section = read_as(
        &["/home/demo/Documents", "/home/demo/Music"],
        Some("/home/demo"),
    );
    assert_eq!(
        section.current(),
        list(&["/home/demo/Documents", "/home/demo/Music"])
    );
    assert_eq!(section.home().as_deref(), Some("/home/demo"));
}

#[test]
fn a_save_s_answer_is_taken_whether_or_not_the_list_was_read() {
    let unread = Allowlist::new(String::new());
    unread.take(dto(&["/home/demo/Documents"], Some("/home/demo")));
    assert_eq!(
        unread.held.get_untracked(),
        Some(Listed::read(dto(
            &["/home/demo/Documents"],
            Some("/home/demo")
        )))
    );
    let read = read_as(&["/home/demo/Documents"], Some("/home/demo"));
    read.take(dto(&["/home/demo/Music"], None));
    assert_eq!(read.current(), list(&["/home/demo/Music"]));
    assert_eq!(read.home().as_deref(), Some("/home/demo"));
}

#[test]
fn one_save_goes_at_a_time_and_the_next_starts_once_it_ends() {
    let section = Allowlist::new(String::new());
    assert!(section.start());
    assert!(section.saving.get_untracked());
    assert!(!section.start());
    section.end();
    assert!(!section.saving.get_untracked());
    assert!(section.start());
}

#[test]
fn the_folder_field_empties_once_the_folder_it_holds_is_added() {
    let typed = RwSignal::new(" /home/demo/Music ".to_owned());
    empty_if_still(typed, "/home/demo/Music");
    assert_eq!(typed.get_untracked(), "");
}

#[test]
fn a_folder_typed_while_the_last_was_added_stays_in_the_field() {
    let typed = RwSignal::new("/home/demo/Pic".to_owned());
    empty_if_still(typed, "/home/demo/Music");
    assert_eq!(typed.get_untracked(), "/home/demo/Pic");
}
