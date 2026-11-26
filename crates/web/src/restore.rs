//! What restoring an entry of a snapshot from its page decides (technical
//! design §10.4), host-tested: the addresses it asks, the requests it sends,
//! what the step-up field asks for, when the dialog can be confirmed, and
//! what a refusal and each ending of a restore are said as.
//! `snapshot_page.rs` is its glue.

use api::{
    ErrorCode, MsgArg, RestoreDestinationDto, RestorePreviewDto, RestorePreviewRequestDto,
    RestoreRequestDto,
};
use l10n::msg;

use crate::announcer::Politeness;
use crate::components::tree::rules::encoded;
use crate::maintenance::Awaited;
use crate::session::Standing;
use crate::wording::Words;

/// How many characters of a snapshot's ID its short ID shows, as the page
/// heading and the step-up field show it.
pub(crate) const SHORT_ID_LENGTH: u64 = 8;

/// The route that starts a restore of an entry of snapshot `snapshot` of the
/// repository `alias`, each percent-encoded as a folder's address is.
pub(crate) fn start_path(alias: &str, snapshot: &str) -> String {
    format!(
        "/api/v1/repositories/{}/snapshots/{}/restore",
        encoded(alias),
        encoded(snapshot)
    )
}

/// The route of the dry run that says what restoring an entry of that
/// snapshot in place would do.
pub(crate) fn preview_path(alias: &str, snapshot: &str) -> String {
    format!("{}/preview", start_path(alias, snapshot))
}

/// What the step-up field asks for (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asks {
    /// The admin password, with sign-in on.
    Password,
    /// The snapshot's short ID, with sign-in off.
    ShortId,
}

impl Asks {
    /// What the field asks for where the page stands with sign-in: the short
    /// ID only with sign-in off, the password otherwise. A page that could
    /// not read where it stands asks for the password, which a daemon with
    /// sign-in off refuses as a wrong confirmation, and nothing runs.
    pub(crate) fn of(standing: Option<Standing>) -> Asks {
        match standing {
            Some(Standing::Off) => Asks::ShortId,
            _ => Asks::Password,
        }
    }
}

/// Where the dry run of a restore in place stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Preview {
    /// Not asked for: in place was never chosen.
    #[default]
    Unasked,
    /// On its way.
    Reading,
    /// What it found.
    Read(RestorePreviewDto),
    /// It got a refusal or no answer, which the dialog says; choosing in
    /// place again asks again.
    Failed,
}

impl Preview {
    /// What the dialog says of it: that it reads, or what it found.
    pub(crate) fn words(self) -> Option<Words> {
        match self {
            Preview::Reading => Some(Words {
                id: msg::UI_RESTORE_PREVIEWING,
                args: Vec::new(),
            }),
            Preview::Read(found) => Some(Words {
                id: msg::UI_RESTORE_PREVIEW,
                args: vec![
                    ("added", MsgArg::Count(found.added)),
                    ("overwritten", MsgArg::Count(found.overwritten)),
                    ("unchanged", MsgArg::Count(found.unchanged)),
                ],
            }),
            Preview::Unasked | Preview::Failed => None,
        }
    }
}

/// The restore dialog of one entry: where it restores to, the dry run of a
/// restore in place, and whether its start is out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dialog {
    /// The entry's path in the snapshot, the names rustic records joined by
    /// `/`.
    path: String,
    pub(crate) destination: RestoreDestinationDto,
    pub(crate) preview: Preview,
    pub(crate) starting: bool,
}

impl Dialog {
    /// The dialog of the entry at `path`, as it opens: into a new folder,
    /// which touches no file that is there.
    pub(crate) fn new(path: String) -> Dialog {
        Dialog {
            path,
            destination: RestoreDestinationDto::NewFolder,
            preview: Preview::Unasked,
            starting: false,
        }
    }

    /// Chooses where to restore. Choosing in place asks for its dry run,
    /// unless it is read or on its way: the request returned is to be sent.
    pub(crate) fn choose(
        &mut self,
        destination: RestoreDestinationDto,
    ) -> Option<RestorePreviewRequestDto> {
        self.destination = destination;
        let ask = destination == RestoreDestinationDto::InPlace
            && matches!(self.preview, Preview::Unasked | Preview::Failed);
        ask.then(|| {
            self.preview = Preview::Reading;
            RestorePreviewRequestDto {
                path: self.path.clone(),
            }
        })
    }

    /// The dry run's answer: what it found, or `None` for a refusal or no
    /// answer.
    pub(crate) fn previewed(&mut self, found: Option<RestorePreviewDto>) {
        self.preview = found.map_or(Preview::Failed, Preview::Read);
    }

    /// The request confirming sends, once it can be sent: never while a
    /// start is out, nor while [`Dialog::held`]. The start is then out until
    /// [`Dialog::answered`].
    pub(crate) fn confirm(&mut self, confirmation: String) -> Option<RestoreRequestDto> {
        if self.starting || self.held().is_some() {
            return None;
        }
        self.starting = true;
        Some(RestoreRequestDto {
            path: self.path.clone(),
            destination: self.destination,
            confirmation,
        })
    }

    /// Why the dialog can't be confirmed yet, said beside its confirm button:
    /// in place, while its dry run reads or after it couldn't be read. `None`
    /// once it can be.
    pub(crate) fn held(&self) -> Option<Words> {
        let id = match (self.destination, self.preview) {
            (RestoreDestinationDto::InPlace, Preview::Unasked | Preview::Reading) => {
                msg::UI_RESTORE_HELD_READING
            }
            (RestoreDestinationDto::InPlace, Preview::Failed) => msg::UI_RESTORE_HELD_UNREAD,
            (RestoreDestinationDto::InPlace, Preview::Read(_))
            | (RestoreDestinationDto::NewFolder, _) => return None,
        };
        Some(Words {
            id,
            args: Vec::new(),
        })
    }

    /// Whether the dry run can be read again: in place, once it answered,
    /// whether it was read or not. The dialog offers it in place throughout,
    /// held while the dry run reads, so focus never loses the button.
    pub(crate) fn can_read_again(&self) -> bool {
        self.destination == RestoreDestinationDto::InPlace
            && matches!(self.preview, Preview::Read(_) | Preview::Failed)
    }

    /// Reads the dry run again, when it can be: the request returned is to be
    /// sent, and a restore in place is held until it answers.
    pub(crate) fn read_again(&mut self) -> Option<RestorePreviewRequestDto> {
        self.can_read_again().then(|| {
            self.preview = Preview::Reading;
            RestorePreviewRequestDto {
                path: self.path.clone(),
            }
        })
    }

    /// The daemon answered the start, or didn't: the dialog takes input
    /// again.
    pub(crate) fn answered(&mut self) {
        self.starting = false;
    }
}

/// What a refusal of a start is said as in the step-up field, when it is
/// about what was typed there; any other refusal is the dialog's alert.
pub(crate) fn field_error(code: ErrorCode, asks: Asks) -> Option<Words> {
    (code == ErrorCode::StepUpFailed).then(|| match asks {
        Asks::ShortId => Words {
            id: msg::UI_RESTORE_WRONG_ID,
            args: vec![("length", MsgArg::Count(SHORT_ID_LENGTH))],
        },
        Asks::Password => Words {
            id: msg::UI_RESTORE_WRONG_PASSWORD,
            args: Vec::new(),
        },
    })
}

/// What is said once the restore of the entry shown as `name` started.
pub(crate) fn running(name: &str) -> Words {
    named(msg::UI_RESTORE_RUNNING, name)
}

/// What the wait for the restore of the entry shown as `name` came to is
/// said as, and how, by its operation's status as a maintenance run's end
/// is: a failure assertively, the rest politely. A restore into a new
/// folder that finished or was cancelled names `folder`, where its files
/// are. Nothing is said when the page that waited is gone.
pub(crate) fn ended(
    name: &str,
    folder: Option<&str>,
    awaited: &Awaited,
) -> Option<(Words, Politeness)> {
    let (id, politeness) = match awaited {
        Awaited::Ended(detail) => match (detail.operation.status.as_str(), folder) {
            ("failed", _) => (msg::UI_RESTORE_FAILED, Politeness::Assertive),
            ("cancelled", Some(folder)) => {
                return Some((
                    into(msg::UI_RESTORE_CANCELED_INTO, name, folder),
                    Politeness::Polite,
                ));
            }
            ("cancelled", None) => (msg::UI_RESTORE_CANCELED, Politeness::Polite),
            (_, Some(folder)) => {
                return Some((
                    into(msg::UI_RESTORE_FINISHED_INTO, name, folder),
                    Politeness::Polite,
                ));
            }
            (_, None) => (msg::UI_RESTORE_FINISHED, Politeness::Polite),
        },
        Awaited::GaveUp => (msg::UI_RESTORE_UNWATCHED, Politeness::Polite),
        Awaited::Gone => return None,
    };
    Some((named(id, name), politeness))
}

/// The words `id` says of the restore of `name` into `folder`.
fn into(id: l10n::MessageId, name: &str, folder: &str) -> Words {
    let mut words = named(id, name);
    words.args.push(("folder", MsgArg::Text(folder.to_owned())));
    words
}

fn named(id: l10n::MessageId, name: &str) -> Words {
    Words {
        id,
        args: vec![("name", MsgArg::Text(name.to_owned()))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{OperationDetailDto, OperationSummaryDto};

    fn found(added: u64, overwritten: u64, unchanged: u64) -> RestorePreviewDto {
        RestorePreviewDto {
            added,
            overwritten,
            unchanged,
            kept: 4,
            bytes: 5,
        }
    }

    fn awaited(status: &str) -> Awaited {
        Awaited::Ended(Box::new(OperationDetailDto {
            operation: OperationSummaryDto {
                id: "01a1c74a-2d40-7288-86c8-04ba8db8aa1a".to_owned(),
                plan: String::new(),
                kind: "restore".to_owned(),
                status: status.to_owned(),
                queued_at: "2027-01-15T08:00:00Z".to_owned(),
                started_at: None,
                finished_at: None,
                warning_codes: Vec::new(),
                failure_code: None,
                failure: None,
                result: None,
                repository: Some("demo".to_owned()),
                locations: Vec::new(),
                entry: None,
                restored_to: None,
            },
            warnings: Vec::new(),
        }))
    }

    #[test]
    fn a_restore_is_started_and_previewed_under_its_snapshot_each_name_encoded() {
        assert_eq!(
            start_path("off site", "0a1b"),
            "/api/v1/repositories/off%20site/snapshots/0a1b/restore"
        );
        assert_eq!(
            preview_path("home/../x", "0a1b"),
            "/api/v1/repositories/home%2F..%2Fx/snapshots/0a1b/restore/preview"
        );
    }

    #[test]
    fn the_step_up_field_asks_for_the_short_id_only_with_sign_in_off() {
        assert_eq!(Asks::of(Some(Standing::Off)), Asks::ShortId);
        for standing in [
            Some(Standing::SignedIn),
            Some(Standing::Unread),
            Some(Standing::Asking),
            None,
        ] {
            assert_eq!(Asks::of(standing), Asks::Password, "{standing:?}");
        }
    }

    #[test]
    fn a_restore_into_a_new_folder_is_confirmed_at_once_with_what_was_typed() {
        let mut dialog = Dialog::new("docs/a.txt".to_owned());
        assert_eq!(dialog.destination, RestoreDestinationDto::NewFolder);
        assert_eq!(
            dialog.confirm("0a1b2c3d".to_owned()),
            Some(RestoreRequestDto {
                path: "docs/a.txt".to_owned(),
                destination: RestoreDestinationDto::NewFolder,
                confirmation: "0a1b2c3d".to_owned(),
            })
        );
        assert!(dialog.starting);
    }

    #[test]
    fn a_second_confirm_while_the_start_is_out_sends_nothing_until_it_is_answered() {
        let mut dialog = Dialog::new("a.txt".to_owned());
        assert!(dialog.confirm("x".to_owned()).is_some());
        assert_eq!(dialog.confirm("x".to_owned()), None);
        dialog.answered();
        assert!(!dialog.starting);
        assert!(dialog.confirm("x".to_owned()).is_some());
    }

    #[test]
    fn choosing_in_place_asks_for_its_dry_run_once() {
        let mut dialog = Dialog::new("docs".to_owned());
        assert_eq!(
            dialog.choose(RestoreDestinationDto::InPlace),
            Some(RestorePreviewRequestDto {
                path: "docs".to_owned()
            })
        );
        assert_eq!(dialog.preview, Preview::Reading);
        assert_eq!(dialog.choose(RestoreDestinationDto::NewFolder), None);
        assert_eq!(dialog.destination, RestoreDestinationDto::NewFolder);
        assert_eq!(
            dialog.choose(RestoreDestinationDto::InPlace),
            None,
            "on its way"
        );
        dialog.previewed(Some(found(0, 0, 1)));
        assert_eq!(dialog.choose(RestoreDestinationDto::NewFolder), None);
        assert_eq!(dialog.choose(RestoreDestinationDto::InPlace), None, "read");
    }

    #[test]
    fn a_dry_run_that_failed_is_asked_for_again_when_in_place_is_chosen_again() {
        let mut dialog = Dialog::new("docs".to_owned());
        dialog.choose(RestoreDestinationDto::InPlace);
        dialog.previewed(None);
        assert_eq!(dialog.preview, Preview::Failed);
        dialog.choose(RestoreDestinationDto::NewFolder);
        assert!(dialog.choose(RestoreDestinationDto::InPlace).is_some());
    }

    #[test]
    fn a_restore_in_place_is_confirmed_only_once_its_dry_run_said_what_it_would_change() {
        let mut dialog = Dialog::new("docs".to_owned());
        dialog.choose(RestoreDestinationDto::InPlace);
        assert_eq!(dialog.confirm("x".to_owned()), None, "reading");
        dialog.previewed(None);
        assert_eq!(dialog.confirm("x".to_owned()), None, "failed");
        assert!(!dialog.starting);
        dialog.choose(RestoreDestinationDto::InPlace);
        dialog.previewed(Some(found(1, 2, 3)));
        assert_eq!(
            dialog
                .confirm("x".to_owned())
                .map(|request| request.destination),
            Some(RestoreDestinationDto::InPlace)
        );
    }

    #[test]
    fn a_restore_in_place_is_held_with_its_reason_until_its_dry_run_is_read() {
        let mut dialog = Dialog::new("docs".to_owned());
        assert_eq!(dialog.held(), None, "into a new folder");
        dialog.choose(RestoreDestinationDto::InPlace);
        assert_eq!(
            dialog.held(),
            Some(Words {
                id: msg::UI_RESTORE_HELD_READING,
                args: Vec::new()
            }),
            "reading"
        );
        dialog.previewed(None);
        assert_eq!(
            dialog.held(),
            Some(Words {
                id: msg::UI_RESTORE_HELD_UNREAD,
                args: Vec::new()
            }),
            "failed"
        );
        dialog.choose(RestoreDestinationDto::NewFolder);
        assert_eq!(dialog.held(), None, "back to a new folder");
        dialog.choose(RestoreDestinationDto::InPlace);
        dialog.previewed(Some(found(1, 2, 3)));
        assert_eq!(dialog.held(), None, "read");
    }

    #[test]
    fn a_dry_run_that_answered_is_read_again_on_request_in_place() {
        let mut dialog = Dialog::new("docs".to_owned());
        assert!(!dialog.can_read_again(), "unasked");
        assert_eq!(dialog.read_again(), None, "unasked");
        dialog.choose(RestoreDestinationDto::InPlace);
        assert!(!dialog.can_read_again(), "reading");
        assert_eq!(dialog.read_again(), None, "reading");
        dialog.previewed(None);
        dialog.choose(RestoreDestinationDto::NewFolder);
        assert!(!dialog.can_read_again(), "failed, into a new folder");
        assert_eq!(dialog.read_again(), None, "failed, into a new folder");
        dialog.destination = RestoreDestinationDto::InPlace;
        assert!(dialog.can_read_again(), "failed, in place");
        assert_eq!(
            dialog.read_again(),
            Some(RestorePreviewRequestDto {
                path: "docs".to_owned()
            })
        );
        assert_eq!(dialog.preview, Preview::Reading);
        assert!(dialog.held().is_some(), "held while it reads again");
        dialog.previewed(Some(found(1, 2, 3)));
        assert!(dialog.can_read_again(), "read");
        assert!(dialog.read_again().is_some(), "read");
        assert_eq!(dialog.preview, Preview::Reading);
        assert!(dialog.confirm("x".to_owned()).is_none(), "reading again");
    }

    #[test]
    fn the_dry_run_is_said_while_it_reads_and_by_what_it_found() {
        assert_eq!(Preview::Unasked.words(), None);
        assert_eq!(Preview::Failed.words(), None);
        assert_eq!(
            Preview::Reading.words(),
            Some(Words {
                id: msg::UI_RESTORE_PREVIEWING,
                args: Vec::new()
            })
        );
        assert_eq!(
            Preview::Read(found(1, 2, 3)).words(),
            Some(Words {
                id: msg::UI_RESTORE_PREVIEW,
                args: vec![
                    ("added", MsgArg::Count(1)),
                    ("overwritten", MsgArg::Count(2)),
                    ("unchanged", MsgArg::Count(3)),
                ],
            })
        );
    }

    #[test]
    fn a_wrong_confirmation_is_said_in_the_field_by_what_it_asked_for() {
        assert_eq!(
            field_error(ErrorCode::StepUpFailed, Asks::ShortId),
            Some(Words {
                id: msg::UI_RESTORE_WRONG_ID,
                args: vec![("length", MsgArg::Count(8))],
            })
        );
        assert_eq!(
            field_error(ErrorCode::StepUpFailed, Asks::Password),
            Some(Words {
                id: msg::UI_RESTORE_WRONG_PASSWORD,
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn any_other_refusal_is_the_dialog_s_alert() {
        for code in [
            ErrorCode::RestoreRunning,
            ErrorCode::RestoreFolderUnset,
            ErrorCode::RestoreInPlaceNotHere,
        ] {
            assert_eq!(field_error(code, Asks::ShortId), None, "{code:?}");
        }
    }

    #[test]
    fn a_restore_that_started_is_said_by_the_entry_s_name() {
        assert_eq!(
            running("a\tb.txt"),
            Words {
                id: msg::UI_RESTORE_RUNNING,
                args: vec![("name", MsgArg::Text("a\tb.txt".to_owned()))],
            }
        );
    }

    #[test]
    fn each_ending_of_a_restore_is_said_and_only_a_failure_urgently() {
        let name = || vec![("name", MsgArg::Text("a.txt".to_owned()))];
        for (status, id, politeness) in [
            ("succeeded", msg::UI_RESTORE_FINISHED, Politeness::Polite),
            (
                "succeeded-with-warnings",
                msg::UI_RESTORE_FINISHED,
                Politeness::Polite,
            ),
            ("cancelled", msg::UI_RESTORE_CANCELED, Politeness::Polite),
            ("failed", msg::UI_RESTORE_FAILED, Politeness::Assertive),
        ] {
            assert_eq!(
                ended("a.txt", None, &awaited(status)),
                Some((Words { id, args: name() }, politeness)),
                "{status}"
            );
        }
        assert_eq!(
            ended("a.txt", None, &Awaited::GaveUp),
            Some((
                Words {
                    id: msg::UI_RESTORE_UNWATCHED,
                    args: name()
                },
                Politeness::Polite
            ))
        );
        assert_eq!(ended("a.txt", None, &Awaited::Gone), None);
    }

    #[test]
    fn a_restore_into_a_new_folder_names_it_once_it_finished_or_was_canceled() {
        const FOLDER: &str = "/home/demo/ladderback-restore/laptop-3f2a1b4c-20270115-080000";
        let into = || {
            vec![
                ("name", MsgArg::Text("a.txt".to_owned())),
                ("folder", MsgArg::Text(FOLDER.to_owned())),
            ]
        };
        let name = || vec![("name", MsgArg::Text("a.txt".to_owned()))];
        for (status, said) in [
            (
                "succeeded",
                Words {
                    id: msg::UI_RESTORE_FINISHED_INTO,
                    args: into(),
                },
            ),
            (
                "succeeded-with-warnings",
                Words {
                    id: msg::UI_RESTORE_FINISHED_INTO,
                    args: into(),
                },
            ),
            (
                "cancelled",
                Words {
                    id: msg::UI_RESTORE_CANCELED_INTO,
                    args: into(),
                },
            ),
            // A failure sends the person to the Operations page; the folder
            // may hold nothing.
            (
                "failed",
                Words {
                    id: msg::UI_RESTORE_FAILED,
                    args: name(),
                },
            ),
        ] {
            assert_eq!(
                ended("a.txt", Some(FOLDER), &awaited(status)).map(|(said, _)| said),
                Some(said),
                "{status}"
            );
        }
        assert_eq!(
            ended("a.txt", Some(FOLDER), &Awaited::GaveUp).map(|(said, _)| said),
            Some(Words {
                id: msg::UI_RESTORE_UNWATCHED,
                args: name(),
            })
        );
    }
}
