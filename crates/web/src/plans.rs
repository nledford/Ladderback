//! The decisions behind the Plans page (§4.4, decision 96): what each field of
//! the plan form shows of a plan, the plan a form sends, which field a refusal
//! is about and the words the form has for it, whether a saved plan still
//! waits for the scheduler to plan its next run, and the words of each button
//! and dialog. Host-tested here; the DOM glue that holds the signals and sends
//! the requests is `plans_page.rs`.
//!
//! The form edits the plan's table as the daemon sent it, in the
//! configuration file's keys: a field left as it was shown leaves its setting
//! as it came, so what the form has no field for (a host, a schedule's time
//! zone, a hook written as a table) is sent back untouched. The daemon reads
//! what is sent as it reads the file, so every refusal is its own, at the
//! setting it names: the form parses nothing.

use std::collections::BTreeMap;
use std::time::Duration;

use api::{ErrorCode, MsgArg, PlanDto, PlanSummaryDto, RepositoryDto};
use l10n::{MessageId, msg};
use serde_json::{Map, Value};

use crate::components::SummaryItem;
use crate::repositories::Sentence;

/// A message and its arguments, as each decision here names one.
pub(crate) type Said = (MessageId, Vec<(&'static str, MsgArg)>);

/// The route that lists and creates plans, from the API's root.
pub(crate) const PLANS_PATH: &str = "/api/v1/plans";

/// The route that lists the repositories, whose first a new plan starts with.
pub(crate) const REPOSITORIES_PATH: &str = "/api/v1/repositories";

/// The `id` of the button that opens the form for a new plan, where focus
/// returns once that form is saved or closed.
pub(crate) const ADD_ID: &str = "plan-add";

/// The `id` of the plan form's heading, which names the form and takes focus
/// when it opens.
pub(crate) const FORM_HEADING_ID: &str = "plan-form-heading";

/// The `id` of what the delete dialog says, which describes it.
pub(crate) const DELETE_BODY_ID: &str = "plan-delete-body";

/// How often the plans list is read again while a saved plan waits for the
/// scheduler to plan its next run, which it does a moment after the save.
pub(crate) const NEXT_RUN_EVERY: Duration = Duration::from_millis(500);

/// How many times it is read so, at most: five seconds, after which the row
/// says the run is not planned yet, as the start page does.
pub(crate) const NEXT_RUN_TRIES: u32 = 10;

/// How long each of those reads may take before it counts as unanswered,
/// after which the list is left as it was last read.
pub(crate) const READ_PATIENCE: Duration = Duration::from_secs(5);

/// What a field takes, and so how its text is read and written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// One line of text.
    Text,
    /// A list, one entry a line.
    Lines,
    /// A whole number.
    Count,
    /// On or off: the checkbox.
    Flag,
}

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

/// The tables of a plan the form has fields in, and the first field of each,
/// which a refusal of the whole table is about.
const TABLES: [(&str, Field); 3] = [
    ("schedule", Field::Cron),
    ("retention", Field::KeepLast),
    ("hooks", Field::RunBefore),
];

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

    /// The fields of what a plan backs up, of when, of what forget keeps and
    /// of the commands around a backup: the form's groups, in its order.
    pub(crate) const SOURCES: [Self; 5] = [
        Self::Sources,
        Self::Globs,
        Self::Iglobs,
        Self::ExcludeIfPresent,
        Self::GitIgnore,
    ];
    pub(crate) const RETENTION: [Self; 6] = [
        Self::KeepLast,
        Self::KeepHourly,
        Self::KeepDaily,
        Self::KeepWeekly,
        Self::KeepMonthly,
        Self::KeepYearly,
    ];
    pub(crate) const HOOKS: [Self; 4] = [
        Self::RunBefore,
        Self::RunAfter,
        Self::RunFailed,
        Self::RunFinally,
    ];

    /// The field's setting: the table it is in, if any, and its key there.
    fn setting(self) -> (Option<&'static str>, &'static str) {
        match self {
            Self::Name => (None, "id"),
            Self::Repository => (None, "repository"),
            Self::Sources => (None, "sources"),
            Self::Globs => (None, "globs"),
            Self::Iglobs => (None, "iglobs"),
            Self::ExcludeIfPresent => (None, "exclude-if-present"),
            Self::GitIgnore => (None, "git-ignore"),
            Self::Cron => (Some("schedule"), "cron"),
            Self::KeepLast => (Some("retention"), "keep-last"),
            Self::KeepHourly => (Some("retention"), "keep-hourly"),
            Self::KeepDaily => (Some("retention"), "keep-daily"),
            Self::KeepWeekly => (Some("retention"), "keep-weekly"),
            Self::KeepMonthly => (Some("retention"), "keep-monthly"),
            Self::KeepYearly => (Some("retention"), "keep-yearly"),
            Self::RunBefore => (Some("hooks"), "run-before"),
            Self::RunAfter => (Some("hooks"), "run-after"),
            Self::RunFailed => (Some("hooks"), "run-failed"),
            Self::RunFinally => (Some("hooks"), "run-finally"),
        }
    }

    /// The field's setting as a refusal names it: `schedule.cron`.
    fn path(self) -> String {
        match self.setting() {
            (Some(table), key) => format!("{table}.{key}"),
            (None, key) => key.to_owned(),
        }
    }

    /// The field's name as the catalog's selects take it.
    fn code(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Cron => "cron",
            other => other.setting().1,
        }
    }

    /// The `id` of the field's input, which an error summary's link points at.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Name => "plan-name",
            Self::Repository => "plan-repository",
            Self::Sources => "plan-sources",
            Self::Globs => "plan-globs",
            Self::Iglobs => "plan-iglobs",
            Self::ExcludeIfPresent => "plan-exclude-if-present",
            Self::GitIgnore => "plan-git-ignore",
            Self::Cron => "plan-cron",
            Self::KeepLast => "plan-keep-last",
            Self::KeepHourly => "plan-keep-hourly",
            Self::KeepDaily => "plan-keep-daily",
            Self::KeepWeekly => "plan-keep-weekly",
            Self::KeepMonthly => "plan-keep-monthly",
            Self::KeepYearly => "plan-keep-yearly",
            Self::RunBefore => "plan-run-before",
            Self::RunAfter => "plan-run-after",
            Self::RunFailed => "plan-run-failed",
            Self::RunFinally => "plan-run-finally",
        }
    }

    /// What the field takes.
    pub(crate) fn kind(self) -> Kind {
        match self {
            Self::Name | Self::Repository | Self::Cron => Kind::Text,
            Self::GitIgnore => Kind::Flag,
            Self::KeepLast
            | Self::KeepHourly
            | Self::KeepDaily
            | Self::KeepWeekly
            | Self::KeepMonthly
            | Self::KeepYearly => Kind::Count,
            Self::Sources
            | Self::Globs
            | Self::Iglobs
            | Self::ExcludeIfPresent
            | Self::RunBefore
            | Self::RunAfter
            | Self::RunFailed
            | Self::RunFinally => Kind::Lines,
        }
    }

    /// The field's label.
    pub(crate) fn label(self) -> Said {
        (msg::UI_PLAN_FIELD_LABEL, self.selected())
    }

    /// The hint under the field.
    pub(crate) fn hint(self) -> Said {
        (msg::UI_PLAN_FIELD_HINT, self.selected())
    }

    fn selected(self) -> Vec<(&'static str, MsgArg)> {
        vec![("field", MsgArg::Text(self.code().to_owned()))]
    }

    /// The field's value in `plan`, if it has one.
    fn value(self, plan: &Map<String, Value>) -> Option<&Value> {
        match self.setting() {
            (Some(table), key) => plan.get(table)?.as_object()?.get(key),
            (None, key) => plan.get(key),
        }
    }
}

/// A list entry as its line shows it: text as it is, a hook written as a
/// table by its command, and anything else as the file's JSON would say it.
fn line_of(entry: &Value) -> String {
    match entry {
        Value::String(text) => text.clone(),
        Value::Object(table) => match table.get("command") {
            Some(Value::String(command)) => command.clone(),
            _ => entry.to_string(),
        },
        other => other.to_string(),
    }
}

/// A value as its field's text shows it.
fn text_of(kind: Kind, value: &Value) -> String {
    match (kind, value) {
        (_, Value::String(text)) => text.clone(),
        (Kind::Lines, Value::Array(entries)) => {
            entries.iter().map(line_of).collect::<Vec<_>>().join("\n")
        }
        (_, other) => other.to_string(),
    }
}

/// What a person has typed and chosen: the text of each field but the
/// checkbox, and the checkbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlanForm {
    texts: BTreeMap<Field, String>,
    pub(crate) git_ignore: bool,
}

impl PlanForm {
    /// The form of `plan`, as the daemon sent it: each setting the form has
    /// a field for in that field.
    pub(crate) fn of(plan: &PlanDto) -> Self {
        let texts = Field::ALL
            .into_iter()
            .filter(|field| field.kind() != Kind::Flag)
            .filter_map(|field| {
                let value = field.value(&plan.0)?;
                Some((field, text_of(field.kind(), value)))
            })
            .collect();
        Self {
            texts,
            git_ignore: Field::GitIgnore.value(&plan.0) == Some(&Value::Bool(true)),
        }
    }

    /// The text of `field`; empty for one with nothing in it.
    pub(crate) fn text(&self, field: Field) -> &str {
        self.texts.get(&field).map_or("", String::as_str)
    }

    /// Sets the text of `field`.
    pub(crate) fn set_text(&mut self, field: Field, text: String) {
        self.texts.insert(field, text);
    }

    /// The plan the form sends: `base`, the plan as it came, with each field
    /// changed since written in. A field emptied removes its setting, and a
    /// table left empty goes too; an emptied schedule removes the whole
    /// schedule, which can't hold only a time zone or a jitter. A count that
    /// is not a whole number is sent as it was typed, for the daemon to
    /// refuse at its field.
    pub(crate) fn request(&self, base: &PlanDto) -> PlanDto {
        let shown = Self::of(base);
        let mut plan = base.0.clone();
        for field in Field::ALL {
            match field.kind() {
                Kind::Flag => {
                    if self.git_ignore != shown.git_ignore {
                        let value = self.git_ignore.then_some(Value::Bool(true));
                        write(&mut plan, field, value);
                    }
                }
                kind => {
                    let typed = self.text(field);
                    if typed != shown.text(field) {
                        let old = field.value(&base.0);
                        let value = value_of(kind, typed, old);
                        if field == Field::Cron {
                            write_cron(&mut plan, value);
                        } else {
                            write(&mut plan, field, value);
                        }
                    }
                }
            }
        }
        PlanDto(plan)
    }
}

/// The value `typed` into a field of `kind` stands for; `None` for nothing.
/// `old` is the setting as it came: a line that is the command of a hook
/// written as a table keeps that table.
fn value_of(kind: Kind, typed: &str, old: Option<&Value>) -> Option<Value> {
    match kind {
        Kind::Lines => {
            let mut tables: Vec<&Value> = old
                .and_then(Value::as_array)
                .map(|entries| entries.iter().filter(|entry| entry.is_object()).collect())
                .unwrap_or_default();
            let entries: Vec<Value> = typed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(
                    |line| match tables.iter().position(|table| line_of(table) == line) {
                        Some(at) => tables.remove(at).clone(),
                        None => Value::String(line.to_owned()),
                    },
                )
                .collect();
            (!entries.is_empty()).then_some(Value::Array(entries))
        }
        Kind::Count => {
            let typed = typed.trim();
            (!typed.is_empty()).then(|| match typed.parse::<u64>() {
                Ok(count) => Value::from(count),
                Err(_) => Value::String(typed.to_owned()),
            })
        }
        Kind::Text | Kind::Flag => {
            let typed = typed.trim();
            (!typed.is_empty()).then(|| Value::String(typed.to_owned()))
        }
    }
}

/// Writes `value` as the setting of `field` into `plan`, or removes the
/// setting for `None`, with its table once that is empty.
fn write(plan: &mut Map<String, Value>, field: Field, value: Option<Value>) {
    match (field.setting(), value) {
        ((None, key), Some(value)) => {
            plan.insert(key.to_owned(), value);
        }
        ((None, key), None) => {
            plan.remove(key);
        }
        ((Some(table), key), Some(value)) => {
            let entry = plan
                .entry(table.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            if let Value::Object(settings) = entry {
                settings.insert(key.to_owned(), value);
            }
        }
        ((Some(table), key), None) => {
            if let Some(Value::Object(settings)) = plan.get_mut(table) {
                settings.remove(key);
                if settings.is_empty() {
                    plan.remove(table);
                }
            }
        }
    }
}

/// Writes a schedule typed into the cron field: it replaces an interval
/// (`every`), as a schedule holds one or the other, and nothing removes the
/// whole schedule.
fn write_cron(plan: &mut Map<String, Value>, value: Option<Value>) {
    match value {
        Some(value) => {
            if let Some(Value::Object(schedule)) = plan.get_mut("schedule") {
                schedule.remove("every");
            }
            write(plan, Field::Cron, Some(value));
        }
        None => {
            plan.remove("schedule");
        }
    }
}

/// The setting a refusal of a save names, and what is wrong with it, when
/// it names one.
fn invalid_setting(code: ErrorCode, args: &[(String, MsgArg)]) -> Option<(&str, &str)> {
    if code != ErrorCode::PlanInvalid {
        return None;
    }
    let text = |name: &str| {
        args.iter().find_map(|(arg, value)| match value {
            MsgArg::Text(text) if arg == name => Some(text.as_str()),
            _ => None,
        })
    };
    Some((text("field")?, text("problem")?))
}

/// The field a refusal of a save is about, when it is about one: the field
/// of the setting it names, or of a list's entry in it (`sources[1]`), or
/// the first field of a table it names whole (`schedule`). A setting the
/// form has no field for is about none.
pub(crate) fn field_of(code: ErrorCode, args: &[(String, MsgArg)]) -> Option<Field> {
    let (setting, _) = invalid_setting(code, args)?;
    let named = Field::ALL.into_iter().find(|field| {
        let path = field.path();
        setting
            .strip_prefix(path.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.') || rest.starts_with('['))
    });
    named.or_else(|| {
        TABLES
            .into_iter()
            .find_map(|(table, first)| (setting == table).then_some(first))
    })
}

/// The form's own words for a refusal of a save, when it has any: what to
/// enter in the field, where the daemon's sentence would name a setting of
/// the file.
pub(crate) fn own_words(code: ErrorCode, args: &[(String, MsgArg)]) -> Option<MessageId> {
    let (_, problem) = invalid_setting(code, args)?;
    let field = field_of(code, args)?;
    let malformed = matches!(problem, "invalid" | "wrong-type");
    match (field, problem) {
        (Field::Cron, _) if malformed => Some(msg::UI_PLAN_CRON_INVALID),
        (Field::Name, "missing") => Some(msg::UI_PLAN_NAME_MISSING),
        (Field::Name, "taken") => Some(msg::UI_PLAN_NAME_TAKEN),
        (Field::Name, _) if malformed => Some(msg::UI_PLAN_NAME_INVALID),
        (Field::Repository, "missing") => Some(msg::UI_PLAN_REPOSITORY_MISSING),
        (Field::Repository, "unknown-repository") => Some(msg::UI_PLAN_REPOSITORY_UNKNOWN),
        (Field::Sources, "missing" | "no-sources") => Some(msg::UI_PLAN_SOURCES_MISSING),
        (field, _) if malformed && field.kind() == Kind::Count => Some(msg::UI_PLAN_COUNT_INVALID),
        _ => None,
    }
}

/// What the summary says of a refusal of a save: the form's own words when
/// it has any, else the daemon's `sentence`, at the field the refusal is
/// about. `say` puts a message of the form's in the page's language.
pub(crate) fn refusal_of(
    code: ErrorCode,
    args: &[(String, MsgArg)],
    say: impl Fn(MessageId) -> String,
    sentence: Sentence,
) -> SummaryItem {
    SummaryItem {
        field: field_of(code, args).map(Field::id),
        sentence: match own_words(code, args) {
            Some(id) => Sentence::new(&say(id), None),
            None => sentence,
        },
    }
}

/// The error `field` shows, by the summary's `items`: the problem about it,
/// if one is.
pub(crate) fn error_of(items: &[SummaryItem], field: Field) -> Option<String> {
    items
        .iter()
        .find(|item| item.field == Some(field.id()))
        .map(|item| item.sentence.plain())
}

/// The ID of the plan a save's `answer`, the plan as the daemon saved it,
/// names; `None` for an answer that is not the one the route gives.
pub(crate) fn saved_id(answer: &str) -> Option<String> {
    let plan: PlanDto = serde_json::from_str(answer).ok()?;
    match plan.0.get("id") {
        Some(Value::String(id)) => Some(id.clone()),
        _ => None,
    }
}

/// The plan a new plan's form starts from: the first of the configuration's
/// `repositories` as its repository, which most configurations have only
/// one of, and nothing else.
pub(crate) fn new_plan(repositories: &[RepositoryDto]) -> PlanDto {
    let mut plan = Map::new();
    if let Some(first) = repositories.first() {
        plan.insert("repository".to_owned(), Value::String(first.alias.clone()));
    }
    PlanDto(plan)
}

/// Whether the saved plan `id` is on a schedule the scheduler has not yet
/// planned a run of, by the plans list `plans`.
pub(crate) fn awaits_next_run(plans: &[PlanSummaryDto], id: &str) -> bool {
    plans
        .iter()
        .any(|plan| plan.id == id && plan.scheduled && plan.next_run.is_none())
}

/// The route of the plan `id`.
pub(crate) fn plan_path(id: &str) -> String {
    format!("{PLANS_PATH}/{id}")
}

/// The `id` of the Change button of the plan `id`, where focus returns once
/// its form is saved or closed.
pub(crate) fn change_id(id: &str) -> String {
    format!("plan-change-{id}")
}

/// The `id` of the Delete button of the plan `id`, where focus returns when
/// its dialog is left without deleting.
pub(crate) fn delete_id(id: &str) -> String {
    format!("plan-delete-{id}")
}

/// A message about the plan `id`.
fn about(message: MessageId, id: &str) -> Said {
    (message, vec![("plan", MsgArg::Text(id.to_owned()))])
}

/// What is announced once the plan `id` is saved, by the plan as it came,
/// `base`, and the plan sent, `sent`: a plan that had a schedule and is
/// saved without one runs only when it is started from then on, and the
/// announcement says so, as the cron field's hint does.
pub(crate) fn saved(id: &str, base: &PlanDto, sent: &PlanDto) -> Said {
    let unscheduled = base.0.contains_key("schedule") && !sent.0.contains_key("schedule");
    let message = if unscheduled {
        msg::UI_PLAN_SAVED_UNSCHEDULED
    } else {
        msg::UI_PLAN_SAVED
    };
    about(message, id)
}

/// What is announced once the plan `id` is deleted.
pub(crate) fn deleted(id: &str) -> Said {
    about(msg::UI_PLAN_DELETED, id)
}

/// The Change button of the plan `id`, named for it.
pub(crate) fn change_label(id: &str) -> Said {
    about(msg::UI_PLAN_CHANGE, id)
}

/// The Delete button of the plan `id`, named for it.
pub(crate) fn delete_label(id: &str) -> Said {
    about(msg::UI_PLAN_DELETE, id)
}

/// The title of the dialog that asks before the plan `id` is deleted.
pub(crate) fn delete_title(id: &str) -> Said {
    about(msg::UI_PLAN_DELETE_TITLE, id)
}

/// What that dialog says a new plan with the deleted plan's ID would do.
pub(crate) fn delete_successor(id: &str) -> Said {
    about(msg::UI_PLAN_DELETE_SUCCESSOR, id)
}

/// The heading of the form that changes the plan `id`.
pub(crate) fn form_change_heading(id: &str) -> Said {
    about(msg::UI_PLAN_FORM_CHANGE, id)
}

#[cfg(test)]
mod tests;
