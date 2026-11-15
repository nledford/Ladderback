//! The decisions behind the Plans page (§4.4, decision 96): what each field of
//! the plan form shows of a plan, the plan a form sends, which field a refusal
//! is about and the words the form has for it, whether a saved plan still
//! waits for the scheduler to plan its next run, and the words of each button
//! and dialog. Host-tested here; the DOM glue that holds the signals and sends
//! the requests is `plans_page.rs`.

use api::{ErrorCode, MsgArg, PlanDto, PlanSummaryDto, RepositoryDto};
use l10n::MessageId;

use crate::components::SummaryItem;
use crate::repositories::Sentence;

/// A message and its arguments, as each decision here names one.
pub(crate) type Said = (MessageId, Vec<(&'static str, MsgArg)>);

/// A part of the plan form, each a setting of the plan's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    Name,
    Repository,
    Sources,
    Globs,
    Iglobs,
    ExcludeIfPresent,
    GitIgnore,
    Cron,
    KeepLast,
    KeepHourly,
    KeepDaily,
    KeepWeekly,
    KeepMonthly,
    KeepYearly,
    RunBefore,
    RunAfter,
    RunFailed,
    RunFinally,
}

impl Field {
    /// Every field, in the form's order.
    pub(crate) const ALL: [Self; 18] = [
        Self::Name,
        Self::Repository,
        Self::Sources,
        Self::Globs,
        Self::Iglobs,
        Self::ExcludeIfPresent,
        Self::GitIgnore,
        Self::Cron,
        Self::KeepLast,
        Self::KeepHourly,
        Self::KeepDaily,
        Self::KeepWeekly,
        Self::KeepMonthly,
        Self::KeepYearly,
        Self::RunBefore,
        Self::RunAfter,
        Self::RunFailed,
        Self::RunFinally,
    ];

    /// The `id` of the field's input, which an error summary's link points at.
    pub(crate) fn id(self) -> &'static str {
        todo!()
    }

    /// The field's label.
    pub(crate) fn label(self) -> Said {
        todo!()
    }

    /// The hint under the field.
    pub(crate) fn hint(self) -> Said {
        todo!()
    }
}

/// What a person has typed and chosen: the text of each field but the
/// checkbox, and the checkbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlanForm {
    pub(crate) git_ignore: bool,
}

impl PlanForm {
    /// The form of `plan`, as the daemon sent it.
    pub(crate) fn of(_plan: &PlanDto) -> Self {
        todo!()
    }

    /// The text of `field`.
    pub(crate) fn text(&self, _field: Field) -> &str {
        todo!()
    }

    /// Sets the text of `field`.
    pub(crate) fn set_text(&mut self, _field: Field, _text: String) {
        todo!()
    }

    /// The plan the form sends: `base`, the plan as it came, with each field
    /// changed since written in.
    pub(crate) fn request(&self, _base: &PlanDto) -> PlanDto {
        todo!()
    }
}

/// The field a refusal of a save is about, when it is about one.
pub(crate) fn field_of(_code: ErrorCode, _args: &[(String, MsgArg)]) -> Option<Field> {
    todo!()
}

/// The form's own words for a refusal of a save, when it has any.
pub(crate) fn own_words(_code: ErrorCode, _args: &[(String, MsgArg)]) -> Option<MessageId> {
    todo!()
}

/// What the summary says of a refusal of a save.
pub(crate) fn refusal_of(
    _code: ErrorCode,
    _args: &[(String, MsgArg)],
    _say: impl Fn(MessageId) -> String,
    _sentence: Sentence,
) -> SummaryItem {
    todo!()
}

/// The error `field` shows, by the summary's `items`.
pub(crate) fn error_of(_items: &[SummaryItem], _field: Field) -> Option<String> {
    todo!()
}

/// The ID of the plan a save's `answer` names.
pub(crate) fn saved_id(_answer: &str) -> Option<String> {
    todo!()
}

/// The plan a new plan's form starts from.
pub(crate) fn new_plan(_repositories: &[RepositoryDto]) -> PlanDto {
    todo!()
}

/// Whether the saved plan `id` is on a schedule the scheduler has not yet
/// planned a run of, by the plans list `plans`.
pub(crate) fn awaits_next_run(_plans: &[PlanSummaryDto], _id: &str) -> bool {
    todo!()
}

/// The route of the plan `id`.
pub(crate) fn plan_path(_id: &str) -> String {
    todo!()
}

/// The `id` of the Change button of the plan `id`.
pub(crate) fn change_id(_id: &str) -> String {
    todo!()
}

/// The `id` of the Delete button of the plan `id`.
pub(crate) fn delete_id(_id: &str) -> String {
    todo!()
}

/// What is announced once the plan `id` is saved.
pub(crate) fn saved(_id: &str, _base: &PlanDto, _sent: &PlanDto) -> Said {
    todo!()
}

/// What is announced once the plan `id` is deleted.
pub(crate) fn deleted(_id: &str) -> Said {
    todo!()
}

/// The Change button of the plan `id`.
pub(crate) fn change_label(_id: &str) -> Said {
    todo!()
}

/// The Delete button of the plan `id`.
pub(crate) fn delete_label(_id: &str) -> Said {
    todo!()
}

/// The title of the dialog that asks before the plan `id` is deleted.
pub(crate) fn delete_title(_id: &str) -> Said {
    todo!()
}

/// What that dialog says a new plan with the deleted plan's ID would do.
pub(crate) fn delete_successor(_id: &str) -> Said {
    todo!()
}

/// The heading of the form that changes the plan `id`.
pub(crate) fn form_change_heading(_id: &str) -> Said {
    todo!()
}

#[cfg(test)]
mod tests;
