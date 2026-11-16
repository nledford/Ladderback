//! What downloading an entry of a snapshot from its page decides (technical
//! design §10.4), host-tested: the addresses it asks, the request it sends,
//! when the dialog asks for an archive format, and what a refusal and a
//! download handed to the browser are said as. `snapshot_page.rs` is its
//! glue; what the step-up field asks for is the restore's (`restore::Asks`).

use api::{ArchiveFormatDto, DownloadRequestDto, ErrorCode, MsgArg};
use l10n::msg;

use crate::components::tree::rules::encoded;
use crate::restore::{Asks, SHORT_ID_LENGTH};
use crate::wording::Words;

/// The values of the choice of archive format.
pub(crate) const TAR: &str = "tar";
pub(crate) const ZIP: &str = "zip";

/// The route that confirms a download of an entry of snapshot `snapshot` of
/// the repository `alias`, each percent-encoded as a folder's address is.
pub(crate) fn start_path(alias: &str, snapshot: &str) -> String {
    format!(
        "/api/v1/repositories/{}/snapshots/{}/download",
        encoded(alias),
        encoded(snapshot)
    )
}

/// The route the browser saves the download waiting under `ticket` from.
pub(crate) fn fetch_path(ticket: &str) -> String {
    format!("/api/v1/downloads/{}", encoded(ticket))
}

/// The download dialog of one entry: whether it asks for an archive format,
/// and whether a confirmation is out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dialog {
    path: String,
    folder: bool,
    pub(crate) starting: bool,
}

impl Dialog {
    /// The dialog of the entry at `path`, a folder when `folder`.
    pub(crate) fn new(path: String, folder: bool) -> Dialog {
        Dialog {
            path,
            folder,
            starting: false,
        }
    }

    /// Whether the dialog asks which archive to download as: only for a
    /// folder, since a file is handed over as itself.
    pub(crate) fn asks_format(&self) -> bool {
        self.folder
    }

    /// The request confirming sends, the archive format `chosen` (one of
    /// [`TAR`] and [`ZIP`]): never while one is out, which it then is until
    /// [`Dialog::answered`].
    pub(crate) fn confirm(
        &mut self,
        chosen: &str,
        confirmation: String,
    ) -> Option<DownloadRequestDto> {
        if self.starting {
            return None;
        }
        self.starting = true;
        Some(DownloadRequestDto {
            path: self.path.clone(),
            format: if chosen == ZIP {
                ArchiveFormatDto::Zip
            } else {
                ArchiveFormatDto::Tar
            },
            confirmation,
        })
    }

    /// The daemon answered the confirmation, or didn't: the dialog takes
    /// input again.
    pub(crate) fn answered(&mut self) {
        self.starting = false;
    }
}

/// What a refusal of a download is said as in the step-up field, when it is
/// about what was typed there; any other refusal is the dialog's alert.
pub(crate) fn field_error(code: ErrorCode, asks: Asks) -> Option<Words> {
    (code == ErrorCode::StepUpFailed).then(|| match asks {
        Asks::ShortId => Words {
            id: msg::UI_DOWNLOAD_WRONG_ID,
            args: vec![("length", MsgArg::Count(SHORT_ID_LENGTH))],
        },
        Asks::Password => Words {
            id: msg::UI_DOWNLOAD_WRONG_PASSWORD,
            args: Vec::new(),
        },
    })
}

/// What is said once the download of the entry shown as `name` is handed to
/// the browser to save.
pub(crate) fn handed(name: &str) -> Words {
    Words {
        id: msg::UI_DOWNLOAD_HANDED,
        args: vec![("name", MsgArg::Text(name.to_owned()))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_is_confirmed_under_its_snapshot_and_saved_by_its_ticket_each_name_encoded() {
        assert_eq!(
            start_path("off site", "0a1b"),
            "/api/v1/repositories/off%20site/snapshots/0a1b/download"
        );
        assert_eq!(
            start_path("home/../x", "0a1b"),
            "/api/v1/repositories/home%2F..%2Fx/snapshots/0a1b/download"
        );
        assert_eq!(fetch_path("ab12"), "/api/v1/downloads/ab12");
        assert_eq!(fetch_path("../x"), "/api/v1/downloads/..%2Fx");
    }

    #[test]
    fn only_a_folder_asks_for_an_archive_format() {
        assert!(Dialog::new("home/docs".to_owned(), true).asks_format());
        assert!(!Dialog::new("home/docs/notes.md".to_owned(), false).asks_format());
    }

    #[test]
    fn confirming_sends_the_entry_the_format_chosen_and_what_was_typed_once() {
        let mut dialog = Dialog::new("home/docs".to_owned(), true);
        assert_eq!(
            dialog.confirm(ZIP, "3f2a1b4c".to_owned()),
            Some(DownloadRequestDto {
                path: "home/docs".to_owned(),
                format: ArchiveFormatDto::Zip,
                confirmation: "3f2a1b4c".to_owned(),
            })
        );
        assert!(dialog.starting);
        assert_eq!(dialog.confirm(ZIP, "3f2a1b4c".to_owned()), None);
        dialog.answered();
        assert!(!dialog.starting);
        assert_eq!(
            dialog
                .confirm(TAR, "again".to_owned())
                .map(|request| request.format),
            Some(ArchiveFormatDto::Tar)
        );
    }

    #[test]
    fn a_wrong_step_up_is_the_fields_error_by_what_it_asks_and_any_other_refusal_is_not() {
        assert_eq!(
            field_error(ErrorCode::StepUpFailed, Asks::ShortId),
            Some(Words {
                id: msg::UI_DOWNLOAD_WRONG_ID,
                args: vec![("length", MsgArg::Count(8))],
            })
        );
        assert_eq!(
            field_error(ErrorCode::StepUpFailed, Asks::Password),
            Some(Words {
                id: msg::UI_DOWNLOAD_WRONG_PASSWORD,
                args: Vec::new(),
            })
        );
        for code in [
            ErrorCode::DownloadsBusy,
            ErrorCode::DownloadsUnavailable,
            ErrorCode::EntryUnknown,
            ErrorCode::RestartPending,
        ] {
            assert_eq!(field_error(code, Asks::Password), None, "{code:?}");
        }
    }

    #[test]
    fn a_download_handed_to_the_browser_is_said_by_its_name() {
        assert_eq!(
            handed("notes.md"),
            Words {
                id: msg::UI_DOWNLOAD_HANDED,
                args: vec![("name", MsgArg::Text("notes.md".to_owned()))],
            }
        );
    }
}
