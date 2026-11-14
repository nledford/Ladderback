//! The decisions behind the API tokens of Settings (security §10.1, decision
//! 93): what the form lacks, the request it sends, which field a refusal is
//! about, how a scope and a token's last use read, and the words of each
//! button and dialog. Host-tested here; the DOM glue that holds the signals
//! and sends the requests is `settings.rs`.

use api::{ApiTokenScopeDto, CreateApiTokenDto, CreatedApiTokenDto, ErrorCode, MsgArg};
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

/// Every scope, in the order the form offers them.
pub(crate) const SCOPES: [ApiTokenScopeDto; 4] = [
    ApiTokenScopeDto::Read,
    ApiTokenScopeDto::Run,
    ApiTokenScopeDto::Preview,
    ApiTokenScopeDto::Propose,
];

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

/// A scope as the catalog's selects and the routes name it.
pub(crate) fn scope_code(scope: ApiTokenScopeDto) -> &'static str {
        todo!()
    }

/// The `id` of a scope's checkbox.
pub(crate) fn scope_id(scope: ApiTokenScopeDto) -> &'static str {
        todo!()
    }

/// A scope's name, as a row lists it and its checkbox says it.
pub(crate) fn scope_name(scope: ApiTokenScopeDto) -> Said {
        todo!()
    }

/// What a token of `scope` may do, under its checkbox.
pub(crate) fn scope_hint(scope: ApiTokenScopeDto) -> Said {
        todo!()
    }

/// The route that revokes the token `id`.
pub(crate) fn revoke_path(id: &str) -> String {
        todo!()
    }

/// The `id` of the Revoke button of the token `id`, where focus returns when
/// its dialog is left without revoking.
pub(crate) fn revoke_id(id: &str) -> String {
        todo!()
    }

/// A part of the form a problem can be about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Label,
    Scopes,
}

impl Field {
    /// The `id` an error summary's link points at: the label's input, or the
    /// first scope's checkbox.
    pub(crate) fn id(self) -> &'static str {
        todo!()
    }

    /// What the problem with the field says.
    fn problem(self) -> Said {
        todo!()
    }
}

/// What a person has typed and chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TokenForm {
    pub(crate) label: String,
    pub(crate) scopes: Vec<ApiTokenScopeDto>,
}

impl TokenForm {
    /// What the form leaves out, in the page's order: a label of only spaces
    /// is none, and a token needs a scope. The daemon judges the rest of the
    /// label (its length and characters).
    pub(crate) fn missing(&self) -> Vec<Field> {
        todo!()
    }

    /// The body of the create request: the label without the spaces around
    /// it, and the scopes chosen, each once, in the form's order.
    pub(crate) fn request(&self) -> CreateApiTokenDto {
        todo!()
    }
}

/// What the form lacks, as the error summary lists it, worded by `say`.
/// `None` when nothing is lacking.
pub(crate) fn problems_of(
    form: &TokenForm,
    say: impl Fn(MessageId, &[(&'static str, MsgArg)]) -> String,
) -> Option<Vec<SummaryItem>> {
        todo!()
    }

/// The field a refusal of the create request is about, when it is about one.
pub(crate) fn field_of(code: ErrorCode) -> Option<Field> {
        todo!()
    }

/// The error `field` shows under the summary `items`.
pub(crate) fn error_of(items: &[SummaryItem], field: Field) -> Option<String> {
        todo!()
    }

/// The token a create request's answer holds, or `None` for an answer that
/// is not the one the route gives.
pub(crate) fn created(answer: &str) -> Option<CreatedApiTokenDto> {
        todo!()
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
        todo!()
    }

    /// The message that says it, or the text as it came.
    pub(crate) fn said(&self) -> Result<Said, String> {
        todo!()
    }
}

/// A message about the token `label`.
fn about(id: MessageId, label: &str) -> Said {
        todo!()
    }

/// The Revoke button of the token `label`, named for it.
pub(crate) fn revoke_label(label: &str) -> Said {
        todo!()
    }

/// The title of the dialog that asks before the token `label` is revoked.
pub(crate) fn revoke_title(label: &str) -> Said {
        todo!()
    }

/// What is announced once the token `label` is revoked.
pub(crate) fn revoked(label: &str) -> Said {
        todo!()
    }

/// What the dialog that shows the new token `label` says.
pub(crate) fn created_body(label: &str) -> Said {
        todo!()
    }

#[cfg(test)]
mod tests;
