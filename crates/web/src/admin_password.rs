//! The decisions behind the admin password's section of Settings (security
//! §10.1, decision 70): what the form lacks before it is sent, the request it
//! sends, which field a refusal is about, the words the section shows and
//! what the page does with the daemon's answer. Host-tested here; the DOM
//! glue that holds the signals, sends the request and takes the steps is
//! `settings/password.rs`. The rules a new password must meet are the
//! daemon's, which says them in its refusal; the page only checks that
//! something was typed and that the two new passwords agree, as
//! `ladderback set-password` does at a terminal.

use api::{ErrorCode, MsgArg, SessionDto, SetAdminPasswordDto};
use l10n::{MessageId, msg};

use crate::components::SummaryItem;
use crate::repositories::Sentence;
use crate::session::waiting_password;

/// The route that sets and changes the admin password, from the API's root.
pub(crate) const ADMIN_PASSWORD_PATH: &str = "/api/v1/admin-password";

/// The fewest characters an admin password may have, as the daemon counts
/// them (`domain::access::PASSWORD_MIN_CHARS`): the hint says it, and the
/// daemon decides.
pub(crate) const PASSWORD_MIN_CHARS: u64 = 8;

/// The `id` of the section's heading.
pub(crate) const HEADING_ID: &str = "admin-password-heading";

/// A field of the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    /// The current password, asked for only while sign-in is on.
    Current,
    /// The new password.
    New,
    /// The new password typed again.
    Again,
}

impl Field {
    /// The `id` of the field's input, which an error summary's link points at.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Current => "admin-password-current",
            Self::New => "admin-password-new",
            Self::Again => "admin-password-again",
        }
    }

    /// What the field says when it lacks what it needs.
    fn problem(self) -> MessageId {
        match self {
            Self::Current => msg::UI_ADMIN_PASSWORD_CURRENT_REQUIRED,
            Self::New => msg::UI_ADMIN_PASSWORD_NEW_REQUIRED,
            Self::Again => msg::UI_ADMIN_PASSWORD_MISMATCH,
        }
    }
}

/// What a person has typed: the current password while sign-in is on
/// (`None` while it is off, when the form doesn't ask for it), the new one,
/// and the new one again. A password is never trimmed.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct PasswordForm {
    pub(crate) current: Option<String>,
    pub(crate) new: String,
    pub(crate) again: String,
}

impl std::fmt::Debug for PasswordForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = if self.current.is_some() {
            "Some([hidden])"
        } else {
            "None"
        };
        write!(
            f,
            "PasswordForm {{ current: {current}, new: [hidden], again: [hidden] }}"
        )
    }
}

impl PasswordForm {
    /// The form as typed while sign-in is `on` or off: the current password
    /// only while it is on, when the form asks for it.
    pub(crate) fn typed(on: bool, current: String, new: String, again: String) -> Self {
        Self {
            current: on.then_some(current),
            new,
            again,
        }
    }

    /// What the form lacks, in the page's order: a current password typed
    /// (when it is asked for), a new one typed, and the new one typed again
    /// the same.
    pub(crate) fn missing(&self) -> Vec<Field> {
        let wanted = [
            (
                Field::Current,
                self.current.as_ref().is_some_and(String::is_empty),
            ),
            (Field::New, self.new.is_empty()),
            (Field::Again, self.again != self.new),
        ];
        wanted
            .into_iter()
            .filter_map(|(field, missing)| missing.then_some(field))
            .collect()
    }

    /// The body of the request, which takes the passwords out of the form.
    pub(crate) fn request(self) -> SetAdminPasswordDto {
        SetAdminPasswordDto {
            current_password: self.current,
            new_password: self.new,
        }
    }
}

/// What the form lacks, as the error summary lists it, worded by `say`.
/// `None` when nothing is lacking.
pub(crate) fn problems_of(
    form: &PasswordForm,
    say: impl Fn(MessageId, &[(&'static str, MsgArg)]) -> String,
) -> Option<Vec<SummaryItem>> {
    let items: Vec<_> = form
        .missing()
        .into_iter()
        .map(|field| SummaryItem {
            field: Some(field.id()),
            sentence: Sentence::new(&say(field.problem(), &[]), None),
        })
        .collect();
    (!items.is_empty()).then_some(items)
}

/// The field a refusal of the request is about, when it is about one.
pub(crate) fn field_of(code: ErrorCode) -> Option<Field> {
    match code {
        ErrorCode::CurrentPasswordWrong => Some(Field::Current),
        ErrorCode::AdminPasswordTooShort | ErrorCode::AdminPasswordTooLong => Some(Field::New),
        _ => None,
    }
}

/// The error `field` shows under the summary `items`.
pub(crate) fn error_of(items: &[SummaryItem], field: Field) -> Option<String> {
    items
        .iter()
        .find(|item| item.field == Some(field.id()))
        .map(|item| item.sentence.plain())
}

/// What the section says of itself while sign-in is `on` or off: with it
/// on, that a change ends every session; with it off, that no password is
/// set, until Settings says what waits for a restart (`waiting`) in its
/// place.
pub(crate) fn body(on: bool, waiting: Option<MessageId>) -> Option<MessageId> {
    if on {
        Some(msg::UI_ADMIN_PASSWORD_ON_BODY)
    } else {
        waiting.is_none().then_some(msg::UI_ADMIN_PASSWORD_OFF_BODY)
    }
}

/// The form's button while sign-in is `on` or off.
pub(crate) fn button(on: bool) -> MessageId {
    if on {
        msg::UI_ADMIN_PASSWORD_CHANGE
    } else {
        msg::UI_ADMIN_PASSWORD_SET
    }
}

/// What the daemon answered a sent form with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The password is set or changed.
    Done,
    /// The daemon refused it with `code`, which `sentence` says.
    Refused(ErrorCode, Sentence),
    /// No answer came.
    Unreachable,
}

/// A step the page takes on an [`Answer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Forget the page's session, which the daemon ended with the change,
    /// and show the sign-in page saying the password changed.
    SignInWithTheNew,
    /// Clear the form's passwords and its problems.
    Clear,
    /// Read again what waits for a restart, and say it.
    SayWaiting,
    /// Show these problems.
    Show(Vec<SummaryItem>),
}

/// The steps the page takes, in order, on `answer` to a form sent while
/// sign-in was `on` or off, worded by `say`: the section's own first, while
/// it is still drawn, then the page's, which shows the sign-in page in its
/// place. A reader may leave Settings before the answer comes: then the
/// section is no longer `shown`, and only the page's step is taken, so a
/// changed password still forgets the session the daemon ended and the next
/// view is the sign-in page saying why.
pub(crate) fn steps(
    on: bool,
    answer: Answer,
    shown: bool,
    say: impl Fn(MessageId) -> String,
) -> Vec<Step> {
    let changed = on && answer == Answer::Done;
    let problem = |field: Option<Field>, sentence| {
        Step::Show(vec![SummaryItem {
            field: field.map(Field::id),
            sentence,
        }])
    };
    let mut steps = match (shown, answer) {
        (false, _) => Vec::new(),
        (true, Answer::Done) if on => vec![Step::Clear],
        (true, Answer::Done) => vec![Step::Clear, Step::SayWaiting],
        (true, Answer::Refused(code, sentence)) => vec![problem(field_of(code), sentence)],
        (true, Answer::Unreachable) => vec![problem(
            None,
            Sentence::new(&say(msg::UI_SESSION_UNREACHABLE), None),
        )],
    };
    if changed {
        steps.push(Step::SignInWithTheNew);
    }
    steps
}

/// What Settings says once a password is set with sign-in off, from the
/// session read again after it (`read`): what waits for a restart, and that
/// a restart turns sign-in on when the read says nothing or fails, since
/// the daemon has just said the password is set.
pub(crate) fn said_once_set(read: Option<&SessionDto>) -> MessageId {
    waiting_password(read).unwrap_or(msg::UI_ADMIN_PASSWORD_WAITING)
}

#[cfg(test)]
mod tests;
