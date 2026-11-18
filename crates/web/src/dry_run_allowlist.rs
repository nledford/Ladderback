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
        let _ = msg::UI_DRY_RUN_FOLDER_EMPTY;
        todo!()
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
    pub(crate) fn read(_answer: DryRunAllowlistDto) -> Self {
        todo!()
    }

    /// Takes the daemon's answer to a save: its folders, and its home
    /// folder unless it names none, when the one read first stays.
    pub(crate) fn take(&mut self, _answer: DryRunAllowlistDto) {
        todo!()
    }
}

/// The folders the table shows, a row each: every folder once, in the order
/// the list first holds it, so no two rows share a key.
pub(crate) fn rows(_folders: &[String]) -> Vec<String> {
    todo!()
}

/// The list with the folder `typed` added at its end, without the spaces
/// around it, or why it can't be. The daemon decides whether it is a full
/// path.
pub(crate) fn with_added(_folders: &[String], _typed: &str) -> Result<Vec<String>, Lack> {
    todo!()
}

/// Whether the folder `typed` is the home folder `home` or holds it, so
/// adding it warns first as "Add home folder" does. Compared as text, a
/// trailing `/` aside: `/` holds every home folder, `/home/demo-old` doesn't
/// hold `/home/demo`.
pub(crate) fn opens_home(_typed: &str, _home: Option<&str>) -> bool {
    todo!()
}

/// The list without `folder`, each entry of it.
pub(crate) fn without(_folders: &[String], _folder: &str) -> Vec<String> {
    todo!()
}

/// The home folder "Add home folder" offers: the daemon's, unless it names
/// none or the list holds it already.
pub(crate) fn home_offered(_folders: &[String], _home: Option<&str>) -> Option<String> {
    todo!()
}

/// What a change saves: the whole list. The daemon names the home folder;
/// the page never sends one.
pub(crate) fn request(_folders: Vec<String>) -> DryRunAllowlistDto {
    todo!()
}

/// The Folder field's error for the daemon's refusal of `saved`, the list
/// with the folder typed at its end, when the refusal is about that folder:
/// it isn't a full path (`dry-run-folder-invalid` of the last entry). It
/// names the folder typed and suggests a full path, the home folder's
/// Documents when the daemon names a home folder. `None` for any other
/// refusal, which an alert says.
pub(crate) fn field_refusal(
    _code: ErrorCode,
    _args: &[(String, MsgArg)],
    _saved: &[String],
    _home: Option<&str>,
) -> Option<Said> {
    todo!()
}

/// The page's own words for the daemon's refusal `code` of a save, where
/// the daemon's would name a control the page hasn't ("save again"): the
/// list holding too many folders. `None` for the daemon's own words.
pub(crate) fn alert_words(_code: ErrorCode) -> Option<MessageId> {
    todo!()
}

/// The `id` of the Remove button of `folder`, where focus returns when its
/// dialog is left without removing it: the folder's bytes in hexadecimal,
/// so any path gives an `id` of its own with no space in it.
pub(crate) fn remove_id(_folder: &str) -> String {
    todo!()
}

/// The Remove button of `folder`, named for it.
pub(crate) fn remove_label(_folder: &str) -> Said {
    todo!()
}

/// The title of the dialog that asks before `folder` is removed.
pub(crate) fn remove_title(_folder: &str) -> Said {
    todo!()
}

/// What is announced once `folder` is removed.
pub(crate) fn removed(_folder: &str) -> Said {
    todo!()
}

/// What is announced once `folder` is added.
pub(crate) fn added(_folder: &str) -> Said {
    todo!()
}

/// What the dialog that asks before the home folder `home` is added warns
/// of. `~/.ssh` is passed as a value, so the catalog isolates it as it does
/// a path.
pub(crate) fn home_body(_home: &str) -> Said {
    todo!()
}

#[cfg(test)]
mod tests;
