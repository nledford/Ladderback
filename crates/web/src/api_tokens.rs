//! The decisions behind the API tokens of Settings (security §10.1,
//! decisions 93 and 127) and its AI agents tab: what the form lacks, the
//! request it sends, which field a refusal is about, each permission's
//! group of choices, how a token's permissions and last use read, and the
//! words of each button and dialog. Host-tested here; the DOM glue that
//! holds the signals and sends the requests is `settings.rs`.

use api::{
    ChangeApiTokenPermissionsDto, ChangeWithoutTokenDto, CreateApiTokenDto, CreatedApiTokenDto,
    ErrorCode, MsgArg, PermissionLevelDto, PermissionsDto,
};
use l10n::{MessageId, msg};

use crate::components::SummaryItem;
use crate::repositories::Sentence;

/// A message and its arguments, as each decision here names one.
pub(crate) type Said = (MessageId, Vec<(&'static str, MsgArg)>);

/// The route that lists and creates tokens, from the API's root.
pub(crate) const TOKENS_PATH: &str = "/api/v1/tokens";

/// The most characters a label may have, as the daemon counts them
/// (`domain::access::LABEL_MAX_CHARS`): the hint and the form's own check say
/// it, and the daemon decides.
pub(crate) const LABEL_MAX_CHARS: u64 = 64;

/// Each permission's level, in the API's order.
fn levels(permissions: PermissionsDto) -> [PermissionLevelDto; 7] {
    let PermissionsDto {
        backups,
        backup_runs,
        previews,
        configuration,
        snapshot_contents,
        daemon_log,
        repository_check,
    } = permissions;
    [
        backups,
        backup_runs,
        previews,
        configuration,
        snapshot_contents,
        daemon_log,
        repository_check,
    ]
}

/// The permissions of `levels`, in the API's order.
fn permissions_of(levels: [PermissionLevelDto; 7]) -> PermissionsDto {
    let [
        backups,
        backup_runs,
        previews,
        configuration,
        snapshot_contents,
        daemon_log,
        repository_check,
    ] = levels;
    PermissionsDto {
        backups,
        backup_runs,
        previews,
        configuration,
        snapshot_contents,
        daemon_log,
        repository_check,
    }
}

/// The `id` of the heading of the tokens' section, which names their table
/// and takes focus when the control that held it goes.
pub(crate) const HEADING_ID: &str = "api-tokens-heading";

/// The `id` of the button that creates a token, where focus returns once the
/// new token's dialog is left.
pub(crate) const CREATE_ID: &str = "api-token-create";

/// The `id` of what the new token's dialog says, which describes it.
pub(crate) const CREATED_BODY_ID: &str = "api-token-created-body";

/// The `id` of what the revoke dialog says, which describes it.
pub(crate) const REVOKE_BODY_ID: &str = "api-token-revoke-body";

/// The route that revokes the token `id`.
pub(crate) fn revoke_path(id: &str) -> String {
    format!("{TOKENS_PATH}/{id}")
}

/// The `id` of the Revoke button of the token `id`, where focus returns when
/// its dialog is left without revoking.
pub(crate) fn revoke_id(id: &str) -> String {
    format!("api-token-revoke-{id}")
}

/// A part of the form a problem can be about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Label,
    Permissions,
}

impl Field {
    /// The `id` an error summary's link points at: the label's input, or the
    /// first choice of the first permission's group.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Label => "api-token-label",
            Self::Permissions => "api-token-backups-none",
        }
    }

    /// What the problem with the field says.
    fn problem(self) -> Said {
        match self {
            Self::Label => (
                msg::ERROR_API_TOKEN_LABEL,
                vec![("max", MsgArg::Count(LABEL_MAX_CHARS))],
            ),
            Self::Permissions => (msg::ERROR_API_TOKEN_NO_PERMISSION, Vec::new()),
        }
    }
}

/// What a person has typed and chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TokenForm {
    pub(crate) label: String,
    pub(crate) permissions: PermissionsDto,
}

impl TokenForm {
    /// What the form leaves out, in the page's order: a label of only spaces
    /// is none, and a token needs a permission. The daemon judges the rest of
    /// the label (its length and characters).
    pub(crate) fn missing(&self) -> Vec<Field> {
        let wanted = [
            (Field::Label, self.label.trim().is_empty()),
            (
                Field::Permissions,
                self.permissions == PermissionsDto::default(),
            ),
        ];
        wanted
            .into_iter()
            .filter_map(|(field, missing)| missing.then_some(field))
            .collect()
    }

    /// The body of the create request: the label without the spaces around
    /// it, and the permissions chosen.
    pub(crate) fn request(&self) -> CreateApiTokenDto {
        CreateApiTokenDto {
            label: self.label.trim().to_owned(),
            permissions: self.permissions,
        }
    }
}

/// What the form lacks, as the error summary lists it, worded by `say`.
/// `None` when nothing is lacking.
pub(crate) fn problems_of(
    form: &TokenForm,
    say: impl Fn(MessageId, &[(&'static str, MsgArg)]) -> String,
) -> Option<Vec<SummaryItem>> {
    let items: Vec<_> = form
        .missing()
        .into_iter()
        .map(|field| {
            let (id, args) = field.problem();
            SummaryItem {
                field: Some(field.id()),
                sentence: Sentence::new(&say(id, &args), None),
            }
        })
        .collect();
    (!items.is_empty()).then_some(items)
}

/// The field a refusal of the create request is about, when it is about one.
pub(crate) fn field_of(code: ErrorCode) -> Option<Field> {
    match code {
        ErrorCode::ApiTokenLabel => Some(Field::Label),
        ErrorCode::ApiTokenNoPermission => Some(Field::Permissions),
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

/// The token a create request's answer holds, or `None` for an answer that
/// is not the one the route gives.
pub(crate) fn created(answer: &str) -> Option<CreatedApiTokenDto> {
    serde_json::from_str(answer).ok()
}

/// When a program last used a token, as its row says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LastUsed {
    Never,
    /// How long ago, counted back from `now`: `unit` is the catalog's select
    /// (`now`, `minutes`, `hours`, `days`) and `count` how many, rounded down.
    Ago {
        unit: &'static str,
        count: u64,
    },
    /// An instant that doesn't parse, shown as it came: never a guess.
    Unread(String),
}

impl LastUsed {
    const MINUTE_MS: i64 = 60_000;
    const HOUR_MS: i64 = 60 * Self::MINUTE_MS;
    const DAY_MS: i64 = 24 * Self::HOUR_MS;

    /// The last use the daemon sent (`None`: never used), read from `now_ms`,
    /// the page's clock. An instant after `now_ms`, as from a daemon whose
    /// clock is ahead of the browser's, is less than a minute ago.
    pub(crate) fn of(raw: Option<&str>, now_ms: i64) -> Self {
        let Some(raw) = raw else {
            return Self::Never;
        };
        let Ok(then) = raw.parse::<jiff::Timestamp>() else {
            return Self::Unread(raw.to_owned());
        };
        let ago = now_ms.saturating_sub(then.as_millisecond()).max(0);
        if ago < Self::MINUTE_MS {
            return Self::Ago {
                unit: "now",
                count: 0,
            };
        }
        let (unit, per) = if ago < Self::HOUR_MS {
            ("minutes", Self::MINUTE_MS)
        } else if ago < Self::DAY_MS {
            ("hours", Self::HOUR_MS)
        } else {
            ("days", Self::DAY_MS)
        };
        Self::Ago {
            unit,
            count: u64::try_from(ago / per).unwrap_or(0),
        }
    }

    /// The message that says it, or the text as it came.
    pub(crate) fn said(&self) -> Result<Said, String> {
        match self {
            Self::Never => Ok((msg::UI_API_TOKEN_NEVER_USED, Vec::new())),
            Self::Ago { unit, count } => Ok((
                msg::UI_API_TOKEN_LAST_USED,
                vec![
                    ("unit", MsgArg::Text((*unit).to_owned())),
                    ("count", MsgArg::Count(*count)),
                ],
            )),
            Self::Unread(raw) => Err(raw.clone()),
        }
    }
}

/// A message about the token `label`.
fn about(id: MessageId, label: &str) -> Said {
    (id, vec![("label", MsgArg::Text(label.to_owned()))])
}

/// The Revoke button of the token `label`, named for it.
pub(crate) fn revoke_label(label: &str) -> Said {
    about(msg::UI_API_TOKEN_REVOKE, label)
}

/// The title of the dialog that asks before the token `label` is revoked.
pub(crate) fn revoke_title(label: &str) -> Said {
    about(msg::UI_API_TOKEN_REVOKE_TITLE, label)
}

/// What is announced once the token `label` is revoked.
pub(crate) fn revoked(label: &str) -> Said {
    about(msg::UI_API_TOKEN_REVOKED, label)
}

/// What the dialog that shows the new token `label` says.
pub(crate) fn created_body(label: &str) -> Said {
    about(msg::UI_API_TOKEN_CREATED_BODY, label)
}

/// The `id` of the button that changes the permissions of the token `id`,
/// where focus returns when its dialog is left.
pub(crate) fn change_id(id: &str) -> String {
    format!("api-token-edit-{id}")
}

/// The button that changes the permissions of the token `label`, named
/// for it.
pub(crate) fn change_label(label: &str) -> Said {
    about(msg::UI_API_TOKEN_CHANGE, label)
}

/// The title of the dialog that changes the permissions of the token
/// `label`.
pub(crate) fn change_title(label: &str) -> Said {
    about(msg::UI_API_TOKEN_CHANGE_TITLE, label)
}

/// What is announced once the permissions of the token `label` are
/// changed.
pub(crate) fn changed(label: &str) -> Said {
    about(msg::UI_API_TOKEN_CHANGED, label)
}

/// A permission a form offers as a group of its own (decision 127): no
/// access, or the permission's one level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    Backups,
    BackupRuns,
    Previews,
    Configuration,
    SnapshotContents,
    DaemonLog,
    RepositoryCheck,
}

/// Every permission, in the API's order, which the forms and a token's row
/// keep.
pub(crate) const ROWS: [Row; 7] = [
    Row::Backups,
    Row::BackupRuns,
    Row::Previews,
    Row::Configuration,
    Row::SnapshotContents,
    Row::DaemonLog,
    Row::RepositoryCheck,
];

/// The value of the radio that grants no access.
pub(crate) const NO_ACCESS: &str = "none";

/// A form of permissions: a new token's, an existing token's change, or
/// what AI agents may do without a token. Each names its groups apart, as
/// a page can hold two of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    Create,
    Change,
    WithoutToken,
}

impl Row {
    /// The permission's code, as the API and the catalog's selects name it.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Backups => "backups",
            Self::BackupRuns => "backup-runs",
            Self::Previews => "previews",
            Self::Configuration => "configuration",
            Self::SnapshotContents => "snapshot-contents",
            Self::DaemonLog => "daemon-log",
            Self::RepositoryCheck => "repository-check",
        }
    }

    /// The row's one level, which its group's second choice grants.
    const fn level(self) -> PermissionLevelDto {
        match self {
            Self::Backups | Self::SnapshotContents | Self::DaemonLog => PermissionLevelDto::Read,
            Self::BackupRuns => PermissionLevelDto::StartAndCancel,
            Self::Previews => PermissionLevelDto::Preview,
            Self::Configuration => PermissionLevelDto::Propose,
            Self::RepositoryCheck => PermissionLevelDto::Run,
        }
    }

    /// The code of the row's one level: the value of the radio that grants
    /// it.
    pub(crate) const fn level_code(self) -> &'static str {
        match self.level() {
            PermissionLevelDto::None => NO_ACCESS,
            PermissionLevelDto::Read => "read",
            PermissionLevelDto::StartAndCancel => "start-and-cancel",
            PermissionLevelDto::Preview => "preview",
            PermissionLevelDto::Propose => "propose",
            PermissionLevelDto::Run => "run",
        }
    }

    /// The `name` of the row's group in `form`; its radios' ids are this
    /// and their values.
    pub(crate) const fn group(self, form: Form) -> &'static str {
        match form {
            Form::Create => match self {
                Self::Backups => "api-token-backups",
                Self::BackupRuns => "api-token-backup-runs",
                Self::Previews => "api-token-previews",
                Self::Configuration => "api-token-configuration",
                Self::SnapshotContents => "api-token-snapshot-contents",
                Self::DaemonLog => "api-token-daemon-log",
                Self::RepositoryCheck => "api-token-repository-check",
            },
            Form::Change => match self {
                Self::Backups => "api-token-change-backups",
                Self::BackupRuns => "api-token-change-backup-runs",
                Self::Previews => "api-token-change-previews",
                Self::Configuration => "api-token-change-configuration",
                Self::SnapshotContents => "api-token-change-snapshot-contents",
                Self::DaemonLog => "api-token-change-daemon-log",
                Self::RepositoryCheck => "api-token-change-repository-check",
            },
            Form::WithoutToken => match self {
                Self::Backups => "without-token-backups",
                Self::BackupRuns => "without-token-backup-runs",
                Self::Previews => "without-token-previews",
                Self::Configuration => "without-token-configuration",
                Self::SnapshotContents => "without-token-snapshot-contents",
                Self::DaemonLog => "without-token-daemon-log",
                Self::RepositoryCheck => "without-token-repository-check",
            },
        }
    }

    /// A message about the row's permission.
    fn about(self, id: MessageId) -> Said {
        (
            id,
            vec![("permission", MsgArg::Text(self.code().to_owned()))],
        )
    }

    /// The row's name: its group's legend.
    pub(crate) fn name(self) -> Said {
        self.about(msg::UI_PERMISSION)
    }

    /// What the row opens, in plain words, under its legend.
    pub(crate) fn hint(self) -> Said {
        self.about(msg::UI_PERMISSION_HINT)
    }

    /// How a token's row lists the permission: its name and its level.
    pub(crate) fn granted(self) -> Said {
        self.about(msg::UI_PERMISSION_GRANTED)
    }

    /// The group's choices, values and words: no access, then the level.
    pub(crate) fn choices(self) -> [(&'static str, Said); 2] {
        let level = |code: &'static str| {
            (
                code,
                (
                    msg::UI_PERMISSION_LEVEL,
                    vec![("level", MsgArg::Text(code.to_owned()))],
                ),
            )
        };
        [level(NO_ACCESS), level(self.level_code())]
    }
}

/// What a new token starts with (decision 127): it sees and starts
/// backups, previews and proposes; what can show a file's name or a log
/// line is off until chosen.
pub(crate) fn new_token() -> PermissionsDto {
    PermissionsDto {
        backups: PermissionLevelDto::Read,
        backup_runs: PermissionLevelDto::StartAndCancel,
        previews: PermissionLevelDto::Preview,
        configuration: PermissionLevelDto::Propose,
        ..PermissionsDto::default()
    }
}

/// The value each group holds for `permissions`, in [`ROWS`]' order.
pub(crate) fn chosen_of(permissions: &PermissionsDto) -> [&'static str; 7] {
    let mut chosen = [NO_ACCESS; 7];
    for (value, (row, held)) in chosen
        .iter_mut()
        .zip(ROWS.into_iter().zip(levels(*permissions)))
    {
        if held == row.level() {
            *value = row.level_code();
        }
    }
    chosen
}

/// The permissions the groups' values `chosen` give, in [`ROWS`]' order: a
/// group at its row's level holds it, and any other value is no access.
pub(crate) fn permissions_from(chosen: [&str; 7]) -> PermissionsDto {
    let mut held = [PermissionLevelDto::None; 7];
    for (level, (row, value)) in held.iter_mut().zip(ROWS.into_iter().zip(chosen)) {
        if value == row.level_code() {
            *level = row.level();
        }
    }
    permissions_of(held)
}

/// The permissions a token's row lists: each it holds, in [`ROWS`]' order.
pub(crate) fn granted_of(permissions: &PermissionsDto) -> Vec<Row> {
    ROWS.into_iter()
        .zip(levels(*permissions))
        .filter_map(|(row, level)| (level != PermissionLevelDto::None).then_some(row))
        .collect()
}

/// The route that changes the permissions of the token `id`.
pub(crate) fn change_path(id: &str) -> String {
    format!("{TOKENS_PATH}/{id}/permissions")
}

/// The body of a token's change: its permissions from now on and the
/// current admin password.
pub(crate) fn change_request(
    permissions: PermissionsDto,
    password: String,
) -> ChangeApiTokenPermissionsDto {
    ChangeApiTokenPermissionsDto {
        permissions,
        current_password: password,
    }
}

/// The route of what AI agents may do without a token.
pub(crate) const WITHOUT_TOKEN_PATH: &str = "/api/v1/settings/without-token";

/// The body of a change of what AI agents may do without a token: the
/// current admin password only with sign-in on (`signed_in`), as with
/// sign-in off there is none.
pub(crate) fn without_token_request(
    permissions: PermissionsDto,
    password: String,
    signed_in: bool,
) -> ChangeWithoutTokenDto {
    ChangeWithoutTokenDto {
        permissions,
        current_password: signed_in.then_some(password),
    }
}

#[cfg(test)]
mod tests;
