//! The decisions behind Settings' dry-run allowlist (§16, decision 89, point
//! 13): the folders a plan dry run an AI agent asks for may read. What each
//! row shows, the list a change saves, why a folder typed can't be added,
//! when a folder typed warns as the home folder does, the home folder "Add
//! home folder" offers, which refusal is the field's error and how an
//! alert words the rest, and the words of each button and dialog.
//! Host-tested here; the DOM glue that holds the signals and sends the
//! requests is `settings/dry_run_allowlist.rs`.

use api::{DryRunAllowlistDto, ErrorCode, MsgArg};
use l10n::{MessageId, msg};

use crate::api_tokens::Said;

/// The route that reads and saves the allowlist, from the API's root.
pub(crate) const ALLOWLIST_PATH: &str = "/api/v1/settings/dry-run-allowlist";

/// The `id` of the section's heading, which names the table of folders and
/// takes focus when the control that held it goes.
pub(crate) const HEADING_ID: &str = "dry-run-allowlist-heading";

/// Why a folder typed can't be added, found before anything is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lack {
    /// Nothing but spaces was typed.
    Empty,
    /// The list holds the folder already.
    Listed,
}

impl Lack {
    /// The error the field shows.
    pub(crate) fn said(self) -> MessageId {
        match self {
            Self::Empty => msg::UI_DRY_RUN_FOLDER_EMPTY,
            Self::Listed => msg::UI_DRY_RUN_FOLDER_LISTED,
        }
    }
}

/// The list as the page holds it once the daemon has read it: its folders,
/// and the daemon's home folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) folders: Vec<String>,
    pub(crate) home: Option<String>,
}

impl Listed {
    /// The list the daemon's read answered.
    pub(crate) fn read(answer: DryRunAllowlistDto) -> Self {
        Self {
            folders: answer.folders,
            home: answer.home,
        }
    }

    /// Takes the daemon's answer to a save: its folders, and its home
    /// folder unless it names none, when the one read first stays.
    pub(crate) fn take(&mut self, answer: DryRunAllowlistDto) {
        if answer.home.is_some() {
            self.home = answer.home;
        }
        self.folders = answer.folders;
    }
}

/// The folders the table shows, a row each: every folder once, in the order
/// the list first holds it, so no two rows share a key.
pub(crate) fn rows(folders: &[String]) -> Vec<String> {
    let mut shown: Vec<String> = Vec::with_capacity(folders.len());
    for folder in folders {
        if !shown.contains(folder) {
            shown.push(folder.clone());
        }
    }
    shown
}

/// The list with the folder `typed` added at its end, without the spaces
/// around it, or why it can't be. The daemon decides whether it is a full
/// path.
pub(crate) fn with_added(folders: &[String], typed: &str) -> Result<Vec<String>, Lack> {
    let folder = typed.trim();
    if folder.is_empty() {
        return Err(Lack::Empty);
    }
    if folders.iter().any(|listed| listed == folder) {
        return Err(Lack::Listed);
    }
    let mut added = folders.to_vec();
    added.push(folder.to_owned());
    Ok(added)
}

/// Whether the folder `typed` is the home folder `home` or holds it, so
/// adding it warns first as "Add home folder" does. Compared as text, a
/// trailing `/` aside: `/` holds every home folder, `/home/demo-old` doesn't
/// hold `/home/demo`.
pub(crate) fn opens_home(typed: &str, home: Option<&str>) -> bool {
    let Some(home) = home.filter(|home| !home.is_empty()) else {
        return false;
    };
    let typed = typed.trim();
    let folder = typed.trim_end_matches('/');
    if folder.is_empty() {
        return typed.starts_with('/');
    }
    let home = home.trim_end_matches('/');
    home == folder
        || home
            .strip_prefix(folder)
            .is_some_and(|inside| inside.starts_with('/'))
}

/// The list without `folder`, each entry of it.
pub(crate) fn without(folders: &[String], folder: &str) -> Vec<String> {
    folders
        .iter()
        .filter(|listed| *listed != folder)
        .cloned()
        .collect()
}

/// The home folder "Add home folder" offers: the daemon's, unless it names
/// none or the list holds it already.
pub(crate) fn home_offered(folders: &[String], home: Option<&str>) -> Option<String> {
    home.filter(|home| !home.is_empty() && !folders.iter().any(|listed| listed == home))
        .map(str::to_owned)
}

/// What a change saves: the whole list. The daemon names the home folder;
/// the page never sends one.
pub(crate) fn request(folders: Vec<String>) -> DryRunAllowlistDto {
    DryRunAllowlistDto {
        folders,
        home: None,
    }
}

/// The Folder field's error for the daemon's refusal of `saved`, the list
/// with the folder typed at its end, when the refusal is about that folder:
/// it isn't a full path (`dry-run-folder-invalid` of the last entry). It
/// names the folder typed and suggests a full path, the home folder's
/// Documents when the daemon names a home folder. `None` for any other
/// refusal, which an alert says.
pub(crate) fn field_refusal(
    code: ErrorCode,
    args: &[(String, MsgArg)],
    saved: &[String],
    home: Option<&str>,
) -> Option<Said> {
    if code != ErrorCode::DryRunFolderInvalid {
        return None;
    }
    let last = u64::try_from(saved.len()).ok()?;
    let about_typed = args
        .iter()
        .any(|(name, arg)| name == "entry" && *arg == MsgArg::Count(last));
    let folder = saved.last().filter(|_| about_typed)?;
    let folder = ("folder", MsgArg::Text(folder.clone()));
    Some(match home.filter(|home| !home.is_empty()) {
        Some(home) => (
            msg::UI_DRY_RUN_FOLDER_NOT_FULL,
            vec![folder, ("example", MsgArg::Text(documents_of(home)))],
        ),
        None => (msg::UI_DRY_RUN_FOLDER_NOT_FULL_PLAIN, vec![folder]),
    })
}

/// The Documents folder inside `home`, joined with the separator `home`
/// uses: a backslash for a Windows path that has no `/`.
fn documents_of(home: &str) -> String {
    let separator = if home.contains('\\') && !home.contains('/') {
        '\\'
    } else {
        '/'
    };
    let mut documents = home.trim_end_matches(separator).to_owned();
    documents.push(separator);
    documents.push_str("Documents");
    documents
}

/// The page's own words for the daemon's refusal `code` of a save, where
/// the daemon's would name a control the page hasn't ("save again"): the
/// list holding too many folders. `None` for the daemon's own words.
pub(crate) fn alert_words(code: ErrorCode) -> Option<MessageId> {
    (code == ErrorCode::DryRunAllowlistTooLong).then_some(msg::UI_DRY_RUN_ALLOWLIST_TOO_LONG)
}

/// The `id` of the Remove button of `folder`, where focus returns when its
/// dialog is left without removing it: the folder's bytes in hexadecimal,
/// so any path gives an `id` of its own with no space in it.
pub(crate) fn remove_id(folder: &str) -> String {
    const PREFIX: &str = "dry-run-folder-remove-";
    let mut id = String::with_capacity(PREFIX.len().saturating_add(folder.len().saturating_mul(2)));
    id.push_str(PREFIX);
    for byte in folder.bytes() {
        for digit in [byte >> 4, byte & 0x0f] {
            id.push(char::from_digit(u32::from(digit), 16).unwrap_or('0'));
        }
    }
    id
}

/// A message about `folder`.
fn about(id: MessageId, folder: &str) -> Said {
    (id, vec![("folder", MsgArg::Text(folder.to_owned()))])
}

/// The Remove button of `folder`, named for it.
pub(crate) fn remove_label(folder: &str) -> Said {
    about(msg::UI_DRY_RUN_FOLDER_REMOVE, folder)
}

/// The title of the dialog that asks before `folder` is removed.
pub(crate) fn remove_title(folder: &str) -> Said {
    about(msg::UI_DRY_RUN_FOLDER_REMOVE_TITLE, folder)
}

/// What is announced once `folder` is removed.
pub(crate) fn removed(folder: &str) -> Said {
    about(msg::UI_DRY_RUN_FOLDER_REMOVED, folder)
}

/// What is announced once `folder` is added.
pub(crate) fn added(folder: &str) -> Said {
    about(msg::UI_DRY_RUN_FOLDER_ADDED, folder)
}

/// What the dialog that asks before the home folder `home` is added warns
/// of. `~/.ssh` is passed as a value, so the catalog isolates it as it does
/// a path.
pub(crate) fn home_body(home: &str) -> Said {
    (
        msg::UI_DRY_RUN_HOME_BODY,
        vec![
            ("home", MsgArg::Text(home.to_owned())),
            ("keys", MsgArg::Text("~/.ssh".to_owned())),
        ],
    )
}

#[cfg(test)]
mod tests;
