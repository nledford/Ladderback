//! The decisions behind the admin password's section of Settings (security
//! §10.1, decision 70): what the form lacks before it is sent, the request it
//! sends and which field a refusal is about. Host-tested here; the DOM glue
//! that holds the signals and sends the request is `settings.rs`. The rules a
//! new password must meet are the daemon's, which says them in its refusal;
//! the page only checks that something was typed and that the two new
//! passwords agree, as `ladderback set-password` does at a terminal.

use api::{ErrorCode, MsgArg, SessionDto, SetAdminPasswordDto};
use l10n::{MessageId, msg};

use crate::components::SummaryItem;
use crate::repositories::Sentence;

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
        todo!("{self:?}")
    }

    /// What the field says when it lacks what it needs.
    fn problem(self) -> MessageId {
        todo!("{self:?}{:?}", msg::UI_SETTINGS_HEADING)
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
        todo!("{f:p}")
    }
}

impl PasswordForm {
    /// The form as typed while sign-in is `on` or off: the current password
    /// only while it is on, when the form asks for it.
    pub(crate) fn typed(on: bool, current: String, new: String, again: String) -> Self {
        todo!("{on}{current}{new}{again}")
    }

    /// What the form lacks, in the page's order: a current password typed
    /// (when it is asked for), a new one typed, and the new one typed again
    /// the same.
    pub(crate) fn missing(&self) -> Vec<Field> {
        todo!("{}", self.new.len())
    }

    /// The body of the request, which takes the passwords out of the form.
    pub(crate) fn request(self) -> SetAdminPasswordDto {
        todo!("{}", self.new.len())
    }
}

/// What the form lacks, as the error summary lists it, worded by `say`.
/// `None` when nothing is lacking.
pub(crate) fn problems_of(
    form: &PasswordForm,
    say: impl Fn(MessageId, &[(&'static str, MsgArg)]) -> String,
) -> Option<Vec<SummaryItem>> {
    todo!(
        "{}{:?}",
        form.new.len(),
        say(msg::UI_SETTINGS_HEADING, &[]).len()
    )
}

/// The field a refusal of the request is about, when it is about one.
pub(crate) fn field_of(code: ErrorCode) -> Option<Field> {
    todo!("{code:?}")
}

/// The error `field` shows under the summary `items`.
pub(crate) fn error_of(items: &[SummaryItem], field: Field) -> Option<String> {
    todo!("{}{field:?}", items.len())
}

/// What the section says of itself while sign-in is `on` or off.
pub(crate) fn body(on: bool, waiting: Option<MessageId>) -> Option<MessageId> {
    todo!("{on}{waiting:?}")
}

/// The form's button while sign-in is `on` or off.
pub(crate) fn button(on: bool) -> MessageId {
    todo!("{on}")
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
/// sign-in was `on` or off, with the section still `shown` or not.
pub(crate) fn steps(
    on: bool,
    answer: Answer,
    shown: bool,
    say: impl Fn(MessageId) -> String,
) -> Vec<Step> {
    todo!("{on}{answer:?}{shown}{}", say(msg::UI_SETTINGS_HEADING))
}

/// What Settings says once a password is set with sign-in off.
pub(crate) fn said_once_set(read: Option<&SessionDto>) -> MessageId {
    todo!("{read:?}")
}

#[cfg(test)]
mod tests;
