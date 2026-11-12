//! The decisions behind the Repositories page (technical design §4.4): what a
//! form is missing, the request each button sends, which field a refusal
//! belongs to, what a password source reads as, and what an answer means for
//! the form and the list. Host-tested here; the DOM glue that holds the
//! signals and sends the requests is `routes.rs` and `ws.rs`.

use std::collections::BTreeMap;

use api::{
    ConnectRepositoryDto, ErrorCode, InitRepositoryDto, MsgArg, PasswordDto, PasswordSourceDto,
    RepositoryIdDto, RepositoryInitDto, RepositorySettingsDto, TestRepositoryDto, WarningDto,
};
use l10n::{Localizer, MessageId, msg};

use crate::components::SummaryItem;
use crate::ws::Phase;

/// A field of the form, in the order the page shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Location,
    HotStorage,
    Password,
}

impl Field {
    /// The `id` of the field's input, which an error summary's link points at.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Name => "repository-name",
            Self::Location => "repository-location",
            Self::HotStorage => "repository-hot-storage",
            Self::Password => "repository-password",
        }
    }

    /// The message that says the field is empty, for a field a form can leave
    /// empty and still be refused: hot storage is optional.
    pub(crate) fn required(self) -> Option<MessageId> {
        match self {
            Self::Name => Some(msg::UI_REPOSITORIES_NAME_REQUIRED),
            Self::Location => Some(msg::UI_REPOSITORIES_LOCATION_REQUIRED),
            Self::HotStorage => None,
            Self::Password => Some(msg::UI_REPOSITORIES_PASSWORD_REQUIRED),
        }
    }
}

/// What a button asks the daemon to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open the repository and save nothing.
    Test,
    /// Open the repository and save it under the name.
    Connect,
    /// Create the repository where there is none, saving it when it has a name.
    Create,
}

impl Action {
    /// The route the action posts to, from the API's root.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Test => "/api/v1/repositories/_test",
            Self::Connect => "/api/v1/repositories",
            Self::Create => "/api/v1/repositories/_init",
        }
    }
}

/// What a person has typed and chosen. It has no `Debug`, so nothing can
/// print the password it holds (I4).
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Form {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) hot_storage: String,
    pub(crate) password: String,
    /// Whether a connected repository is to be maintained by this agent.
    /// Off unless the person turns it on: only one place may maintain a
    /// repository, and another may already do it (I1).
    pub(crate) maintain_here: bool,
}

impl Form {
    /// The fields the action needs and the form leaves empty, in the order the
    /// page shows them. Spaces around a name or a location are not part of it,
    /// so a field of only spaces is empty; a password is taken as typed.
    pub(crate) fn missing(&self, action: Action) -> Vec<Field> {
        let wanted = [
            (
                Field::Name,
                action == Action::Connect && self.name.trim().is_empty(),
            ),
            (Field::Location, self.location.trim().is_empty()),
            (Field::Password, self.password.is_empty()),
        ];
        wanted
            .into_iter()
            .filter_map(|(field, missing)| missing.then_some(field))
            .collect()
    }

    /// The name the repository is to be saved under: what was typed, without
    /// the spaces around it.
    pub(crate) fn alias(&self) -> &str {
        self.name.trim()
    }

    /// The settings the form describes: the password is the one typed, and
    /// hot storage is left out when it is empty.
    pub(crate) fn settings(&self) -> RepositorySettingsDto {
        RepositorySettingsDto {
            repository: self.location.trim().to_owned(),
            repo_hot: Some(self.hot_storage.trim())
                .filter(|hot| !hot.is_empty())
                .map(str::to_owned),
            password: PasswordDto::Inline {
                value: self.password.clone(),
            },
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        }
    }

    /// The body of `Action::Test`.
    pub(crate) fn test_request(&self) -> TestRepositoryDto {
        TestRepositoryDto {
            settings: self.settings(),
        }
    }

    /// The body of `Action::Connect`.
    pub(crate) fn connect_request(&self) -> ConnectRepositoryDto {
        ConnectRepositoryDto {
            alias: self.alias().to_owned(),
            settings: self.settings(),
            maintain_here: self.maintain_here,
        }
    }

    /// The body of `Action::Create`: the repository is saved only when the
    /// form names it.
    pub(crate) fn create_request(&self) -> InitRepositoryDto {
        InitRepositoryDto {
            alias: Some(self.alias())
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
            settings: self.settings(),
        }
    }
}

/// What the form lacks for `action`, as the error summary lists it: one item
/// for each field the action needs and the form leaves empty, in the page's
/// order, worded by `say`. `None` when nothing is lacking.
pub(crate) fn problems_of(
    form: &Form,
    action: Action,
    say: impl Fn(MessageId) -> String,
) -> Option<Vec<SummaryItem>> {
    let items: Vec<_> = form
        .missing(action)
        .into_iter()
        .filter_map(|field| {
            Some(SummaryItem {
                field: Some(field.id()),
                sentence: Sentence::new(&say(field.required()?), None),
            })
        })
        .collect();
    (!items.is_empty()).then_some(items)
}

/// The error `field` shows under the summary `items`: the sentence of the
/// item that is about it, if there is one.
pub(crate) fn error_of(items: &[SummaryItem], field: Field) -> Option<String> {
    items
        .iter()
        .find(|item| item.field == Some(field.id()))
        .map(|item| item.sentence.plain())
}

/// Whether an answer to a request makes the page read its list again: any
/// answer shows a daemon that is there, and a page that could not read the
/// list takes it up again.
pub(crate) fn rereads_list(answered: bool, phase: Phase) -> bool {
    answered && phase == Phase::Unreachable
}

/// The field a refusal is about, when it is about one: a name the daemon will
/// not take, a secret it could not read, or a path it would read from
/// wherever it was started.
pub(crate) fn field_of(code: ErrorCode, args: &[(String, MsgArg)]) -> Option<Field> {
    match code {
        ErrorCode::RepositoryAliasTaken | ErrorCode::RepositoryAliasInvalid => Some(Field::Name),
        ErrorCode::SecretMissing
        | ErrorCode::SecretUnreadable
        | ErrorCode::SecretNotText
        | ErrorCode::SecretTooLarge
        | ErrorCode::SecretCommandFailed
        | ErrorCode::SecretCommandOutputOpen
        | ErrorCode::SecretCommandTimedOut
        | ErrorCode::SecretInProcessArguments
        | ErrorCode::SecretExpansionTooLong
        | ErrorCode::SecretsTooLargeToMask
        | ErrorCode::TemplateInvalid
        | ErrorCode::LocationTooManyCredentials
        | ErrorCode::RepositoryPathRelative => {
            let named = args.iter().find_map(|(name, value)| match value {
                MsgArg::Text(field) if name == "field" => Some(field.as_str()),
                _ => None,
            })?;
            match named {
                "password" => Some(Field::Password),
                "repository" => Some(Field::Location),
                "repo-hot" => Some(Field::HotStorage),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether a refusal comes with the page's own advice for the next try: the
/// engine could not open the repository with what was typed.
pub(crate) fn advises_retry(code: ErrorCode) -> bool {
    code == ErrorCode::OperationFailed
}

/// What an answer from the daemon means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The repository opened; nothing was saved.
    Opens { id: String },
    /// The repository opened and was saved.
    Connected { id: String },
    /// The repository was created, and saved when `saved`.
    Created { id: String, saved: bool },
}

impl Outcome {
    /// What the daemon's answer to `action` says, or `None` when it is not
    /// the answer that action gets.
    pub(crate) fn of(action: Action, answer: &str) -> Option<Self> {
        match action {
            Action::Test => serde_json::from_str::<RepositoryIdDto>(answer)
                .ok()
                .map(|answered| Self::Opens { id: answered.id }),
            Action::Connect => serde_json::from_str::<RepositoryIdDto>(answer)
                .ok()
                .map(|answered| Self::Connected { id: answered.id }),
            Action::Create => {
                serde_json::from_str::<RepositoryInitDto>(answer)
                    .ok()
                    .map(|answered| Self::Created {
                        id: answered.id,
                        saved: answered.saved,
                    })
            }
        }
    }

    /// Whether the configuration changed, so the list is read again and what
    /// was typed has been kept.
    pub(crate) fn saved(&self) -> bool {
        match self {
            Self::Opens { .. } => false,
            Self::Connected { .. } => true,
            Self::Created { saved, .. } => *saved,
        }
    }
}

impl Outcome {
    /// The sentence the page shows once the daemon answered: the message and
    /// its arguments. `alias` is the name the form was connected under.
    pub(crate) fn shown(&self, alias: &str) -> (MessageId, Vec<(&'static str, MsgArg)>) {
        match self {
            Self::Opens { id } => (msg::UI_REPOSITORIES_OPENS_WITH_ID, vec![("id", text(id))]),
            Self::Connected { .. } => {
                (msg::UI_REPOSITORIES_CONNECTED, vec![("alias", text(alias))])
            }
            Self::Created { id, .. } => {
                (msg::UI_REPOSITORIES_CREATED_WITH_ID, vec![("id", text(id))])
            }
        }
    }

    /// The sentence the announcer says politely: the same news without the
    /// ID, which is a long run of digits a screen reader would spell out.
    pub(crate) fn announced(&self, alias: &str) -> (MessageId, Vec<(&'static str, MsgArg)>) {
        match self {
            Self::Opens { .. } => (msg::UI_REPOSITORIES_OPENS, vec![]),
            Self::Connected { .. } => self.shown(alias),
            Self::Created { .. } => (msg::UI_REPOSITORIES_CREATED, vec![]),
        }
    }
}

/// The route a row's Create posts to: the saved repository named `alias` is
/// created where its entry says, with the password the entry names. An
/// alias is lower-case letters, digits and hyphens, so it needs no escaping.
pub(crate) fn create_saved_path(alias: &str) -> String {
    format!("/api/v1/repositories/{alias}/init")
}

/// The name of a row's Create button: it names the row, so each one is told
/// apart by name alone.
pub(crate) fn create_label(alias: &str) -> (MessageId, Vec<(&'static str, MsgArg)>) {
    (msg::UI_REPOSITORY_CREATE, vec![("alias", text(alias))])
}

/// A repository created from its row: the daemon answered with its ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowCreated {
    pub(crate) id: String,
}

impl RowCreated {
    /// What the daemon's answer to a row's Create says, or `None` when it is
    /// not that answer.
    pub(crate) fn of(answer: &str) -> Option<Self> {
        serde_json::from_str::<RepositoryIdDto>(answer)
            .ok()
            .map(|answered| Self { id: answered.id })
    }

    /// The short confirmation the page shows, and moves focus to: the
    /// repository by name, without its ID.
    pub(crate) fn confirmation(alias: &str) -> (MessageId, Vec<(&'static str, MsgArg)>) {
        (msg::UI_REPOSITORY_CREATED, vec![("alias", text(alias))])
    }

    /// The sentence that follows the confirmation, outside what has focus:
    /// the repository's ID, which a screen reader would spell out digit by
    /// digit.
    pub(crate) fn id_sentence(&self) -> (MessageId, Vec<(&'static str, MsgArg)>) {
        (msg::UI_REPOSITORY_CREATED_ID, vec![("id", text(&self.id))])
    }
}

/// The row's own words for a refusal of its Create, when it has them: a
/// repository already where the entry says names the row. Any other refusal
/// reads as the daemon's sentence, as the form shows it.
pub(crate) fn row_refusal(
    code: ErrorCode,
    alias: &str,
) -> Option<(MessageId, Vec<(&'static str, MsgArg)>)> {
    (code == ErrorCode::RepositoryExists).then(|| {
        (
            msg::UI_REPOSITORY_ALREADY_THERE,
            vec![("alias", text(alias))],
        )
    })
}

fn text(value: &str) -> MsgArg {
    MsgArg::Text(value.to_owned())
}

/// The message that says where a configured repository's password comes
/// from, with its arguments. Never the password itself.
pub(crate) fn password_source(
    source: &PasswordSourceDto,
) -> (MessageId, Vec<(&'static str, MsgArg)>) {
    match source {
        PasswordSourceDto::Inline => (msg::UI_PASSWORD_INLINE, vec![]),
        PasswordSourceDto::File { path } => (msg::UI_PASSWORD_FILE, vec![("path", text(path))]),
        PasswordSourceDto::Keyring { entry } => {
            (msg::UI_PASSWORD_KEYRING, vec![("entry", text(entry))])
        }
        PasswordSourceDto::Env { name } => (msg::UI_PASSWORD_ENV, vec![("name", text(name))]),
        PasswordSourceDto::Command { program } => {
            (msg::UI_PASSWORD_COMMAND, vec![("program", text(program))])
        }
    }
}

/// The notes a repository's row carries, one per warning, as the catalog
/// writes them in the page's language (§8.4). A warning holds a code and the
/// names of the settings it is about, never a secret (I4).
pub(crate) fn repository_notes(localizer: &Localizer, warnings: &[WarningDto]) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| {
            let args: Vec<(&str, MsgArg)> = warning
                .args
                .iter()
                .map(|(name, arg)| (name.as_str(), arg.clone()))
                .collect();
            localizer.warning(warning.code, &args, None).into_text()
        })
        .collect()
}

/// A refusal's sentence, in two parts when the engine's own words are in it:
/// those are English whatever the page's language, and are marked so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sentence {
    pub(crate) before: String,
    /// The engine's words, and the text after them.
    pub(crate) detail: Option<(String, String)>,
}

impl Sentence {
    /// The whole sentence as plain text, for a place that cannot mark the
    /// engine's words.
    pub(crate) fn plain(&self) -> String {
        match &self.detail {
            Some((detail, after)) => format!("{}{detail}{after}", self.before),
            None => self.before.clone(),
        }
    }

    /// `text` with `detail` marked, when `detail` is in it. The catalog puts
    /// bidi isolation marks around a placeable, and the words are found
    /// between them; a text without marks has them where they stand last,
    /// which is where a message puts the sender's words. A text that holds
    /// none of them (the catalog cut a long one short) is one plain run.
    pub(crate) fn new(text: &str, detail: Option<&str>) -> Self {
        let Some(detail) = detail.filter(|detail| !detail.is_empty()) else {
            return Self::plain_run(text);
        };
        let isolated = format!("{FIRST_ISOLATE}{detail}{POP_ISOLATE}");
        let (start, end) = match text.find(&isolated) {
            Some(at) => (
                at + FIRST_ISOLATE.len_utf8(),
                at + isolated.len() - POP_ISOLATE.len_utf8(),
            ),
            None => match text.rfind(detail) {
                Some(at) => (at, at + detail.len()),
                None => return Self::plain_run(text),
            },
        };
        match (text.get(..start), text.get(end..)) {
            (Some(before), Some(after)) => Self {
                before: before.to_owned(),
                detail: Some((detail.to_owned(), after.to_owned())),
            },
            _ => Self::plain_run(text),
        }
    }

    fn plain_run(text: &str) -> Self {
        Self {
            before: text.to_owned(),
            detail: None,
        }
    }
}

/// The marks the catalog's placeables are wrapped in, which keep a text that
/// runs the other way from reordering the sentence around it (§4.5).
const FIRST_ISOLATE: char = '\u{2068}';
const POP_ISOLATE: char = '\u{2069}';

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form {
            name: "copy".to_owned(),
            location: "/srv/backups/main".to_owned(),
            hot_storage: String::new(),
            password: "not-a-real-password".to_owned(),
            maintain_here: false,
        }
    }

    fn text(value: &str) -> MsgArg {
        MsgArg::Text(value.to_owned())
    }

    #[test]
    fn each_field_has_its_own_id_for_a_link_to_point_at() {
        let ids = [
            Field::Name,
            Field::Location,
            Field::HotStorage,
            Field::Password,
        ]
        .map(Field::id);
        assert_eq!(
            ids,
            [
                "repository-name",
                "repository-location",
                "repository-hot-storage",
                "repository-password"
            ]
        );
    }

    #[test]
    fn each_action_posts_to_its_own_route() {
        assert_eq!(Action::Test.path(), "/api/v1/repositories/_test");
        assert_eq!(Action::Connect.path(), "/api/v1/repositories");
        assert_eq!(Action::Create.path(), "/api/v1/repositories/_init");
    }

    #[test]
    fn a_complete_form_misses_nothing() {
        for action in [Action::Test, Action::Connect, Action::Create] {
            assert_eq!(form().missing(action), vec![], "{action:?}");
        }
    }

    #[test]
    fn an_empty_form_misses_the_location_and_the_password_in_page_order() {
        let empty = Form::default();
        assert_eq!(
            empty.missing(Action::Test),
            vec![Field::Location, Field::Password]
        );
        assert_eq!(
            empty.missing(Action::Create),
            vec![Field::Location, Field::Password],
            "a repository can be created without a name: it is then not saved"
        );
        assert_eq!(
            empty.missing(Action::Connect),
            vec![Field::Name, Field::Location, Field::Password],
            "a repository is connected under a name"
        );
    }

    #[test]
    fn spaces_alone_are_no_name_and_no_location_but_a_password_is_taken_as_typed() {
        let spaces = Form {
            name: "  ".to_owned(),
            location: " \t".to_owned(),
            hot_storage: " ".to_owned(),
            password: " ".to_owned(),
            maintain_here: false,
        };
        assert_eq!(
            spaces.missing(Action::Connect),
            vec![Field::Name, Field::Location],
            "a password of one space is a password"
        );
    }

    #[test]
    fn hot_storage_is_never_required() {
        assert_eq!(form().missing(Action::Connect), vec![]);
    }

    fn english(id: MessageId) -> String {
        crate::text(&l10n::Localizer::new(&[]), id)
    }

    #[test]
    fn a_form_that_lacks_nothing_has_no_problems() {
        for action in [Action::Test, Action::Connect, Action::Create] {
            assert_eq!(problems_of(&form(), action, english), None, "{action:?}");
        }
    }

    #[test]
    fn each_missing_field_is_a_problem_about_it_in_page_order() {
        let items = problems_of(&Form::default(), Action::Connect, english)
            .expect("an empty form lacks fields");
        let listed: Vec<_> = items
            .iter()
            .map(|item| (item.field, item.sentence.plain()))
            .collect();
        assert_eq!(
            listed,
            vec![
                (
                    Some("repository-name"),
                    "Enter a name for the repository.".to_owned()
                ),
                (
                    Some("repository-location"),
                    "Enter where the repository is.".to_owned()
                ),
                (
                    Some("repository-password"),
                    "Enter the repository's password.".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_field_shows_the_error_of_the_item_about_it_and_no_other() {
        let items = problems_of(&Form::default(), Action::Test, english).expect("problems");
        assert_eq!(error_of(&items, Field::Name), None);
        assert_eq!(
            error_of(&items, Field::Location).as_deref(),
            Some("Enter where the repository is.")
        );
        assert_eq!(error_of(&items, Field::HotStorage), None);
        assert_eq!(
            error_of(&items, Field::Password).as_deref(),
            Some("Enter the repository's password.")
        );
        assert_eq!(error_of(&[], Field::Location), None);
    }

    #[test]
    fn only_an_answer_to_a_page_that_could_not_read_its_list_rereads_it() {
        assert!(rereads_list(true, Phase::Unreachable));
        assert!(!rereads_list(false, Phase::Unreachable), "no answer at all");
        assert!(!rereads_list(true, Phase::Ready));
        assert!(!rereads_list(true, Phase::Loading));
    }

    #[test]
    fn the_settings_carry_the_typed_password_and_leave_out_empty_hot_storage() {
        let settings = form().settings();
        assert_eq!(settings.repository, "/srv/backups/main");
        assert_eq!(settings.repo_hot, None);
        assert_eq!(
            settings.password,
            PasswordDto::Inline {
                value: "not-a-real-password".to_owned()
            }
        );
        assert!(settings.options.is_empty());
        assert!(settings.options_hot.is_empty());
        assert!(settings.options_cold.is_empty());
    }

    #[test]
    fn the_settings_trim_the_locations_and_keep_the_password_whole() {
        let padded = Form {
            location: "  /srv/backups/main \n".to_owned(),
            hot_storage: " /srv/hot ".to_owned(),
            password: " pass word ".to_owned(),
            ..form()
        };
        let settings = padded.settings();
        assert_eq!(settings.repository, "/srv/backups/main");
        assert_eq!(settings.repo_hot.as_deref(), Some("/srv/hot"));
        assert_eq!(
            settings.password,
            PasswordDto::Inline {
                value: " pass word ".to_owned()
            }
        );
    }

    #[test]
    fn a_test_sends_only_the_settings() {
        assert_eq!(form().test_request().settings, form().settings());
    }

    #[test]
    fn a_connection_sends_the_trimmed_name_with_the_settings() {
        let padded = Form {
            name: " copy ".to_owned(),
            ..form()
        };
        let request = padded.connect_request();
        assert_eq!(request.alias, "copy");
        assert_eq!(request.settings, form().settings());
    }

    #[test]
    fn a_connection_asks_to_maintain_the_repository_here_only_when_the_form_does() {
        assert!(!form().connect_request().maintain_here);
        let chosen = Form {
            maintain_here: true,
            ..form()
        };
        assert!(chosen.connect_request().maintain_here);
    }

    #[test]
    fn a_creation_names_the_repository_only_when_the_form_does() {
        let named = form().create_request();
        assert_eq!(named.alias.as_deref(), Some("copy"));
        assert_eq!(named.settings, form().settings());
        for blank in ["", "   "] {
            let unnamed = Form {
                name: blank.to_owned(),
                ..form()
            };
            assert_eq!(unnamed.create_request().alias, None, "{blank:?}");
        }
    }

    #[test]
    fn a_refused_name_belongs_to_the_name_field() {
        for code in [
            ErrorCode::RepositoryAliasTaken,
            ErrorCode::RepositoryAliasInvalid,
        ] {
            assert_eq!(field_of(code, &[]), Some(Field::Name), "{code:?}");
        }
    }

    #[test]
    fn every_secret_and_template_refusal_belongs_to_the_field_that_holds_it() {
        for code in [
            ErrorCode::SecretMissing,
            ErrorCode::SecretUnreadable,
            ErrorCode::SecretNotText,
            ErrorCode::SecretTooLarge,
            ErrorCode::SecretCommandFailed,
            ErrorCode::SecretCommandOutputOpen,
            ErrorCode::SecretCommandTimedOut,
            ErrorCode::SecretInProcessArguments,
            ErrorCode::SecretExpansionTooLong,
            ErrorCode::SecretsTooLargeToMask,
            ErrorCode::TemplateInvalid,
        ] {
            assert_eq!(
                field_of(code, &[("field".into(), text("password"))]),
                Some(Field::Password),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_location_with_too_many_credentials_belongs_to_the_location_it_is() {
        let field = |name: &str| {
            field_of(
                ErrorCode::LocationTooManyCredentials,
                &[("field".into(), text(name))],
            )
        };
        assert_eq!(field("repository"), Some(Field::Location));
        assert_eq!(field("repo-hot"), Some(Field::HotStorage));
    }

    #[test]
    fn a_relative_path_belongs_to_the_field_that_holds_it() {
        let field = |name: &str| {
            field_of(
                ErrorCode::RepositoryPathRelative,
                &[("field".into(), text(name))],
            )
        };
        assert_eq!(field("repository"), Some(Field::Location));
        assert_eq!(field("repo-hot"), Some(Field::HotStorage));
        assert_eq!(field("password"), Some(Field::Password));
        assert_eq!(
            field("options.root"),
            None,
            "the form has no field for an option"
        );
    }

    #[test]
    fn an_unreadable_secret_belongs_to_the_field_that_holds_it() {
        let field =
            |name: &str| field_of(ErrorCode::SecretMissing, &[("field".into(), text(name))]);
        assert_eq!(field("password"), Some(Field::Password));
        assert_eq!(field("repository"), Some(Field::Location));
        assert_eq!(field("repo-hot"), Some(Field::HotStorage));
        assert_eq!(
            field("options.token"),
            None,
            "the form has no field for an option"
        );
        assert_eq!(field_of(ErrorCode::SecretMissing, &[]), None);
        assert_eq!(
            field_of(
                ErrorCode::SecretMissing,
                &[("field".into(), MsgArg::Count(1))]
            ),
            None
        );
        assert_eq!(
            field_of(
                ErrorCode::SecretMissing,
                &[("other".into(), text("password"))]
            ),
            None,
            "only the argument named field says which field"
        );
    }

    #[test]
    fn a_refusal_about_the_repository_or_the_daemon_belongs_to_no_field() {
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::RepositoryExists,
            // The repository is configured under another name: no name typed
            // here makes it another repository, so the name field is not it.
            ErrorCode::RepositoryAlreadyConfigured,
            ErrorCode::EngineUnavailable,
            ErrorCode::ConfigUnavailable,
            ErrorCode::InvalidBody,
        ] {
            assert_eq!(field_of(code, &[]), None, "{code:?}");
        }
    }

    #[test]
    fn only_a_repository_that_would_not_open_is_followed_by_advice() {
        assert!(advises_retry(ErrorCode::OperationFailed));
        for code in [
            ErrorCode::RepositoryExists,
            ErrorCode::EngineUnavailable,
            ErrorCode::RepositoryAliasTaken,
            ErrorCode::SecretMissing,
        ] {
            assert!(!advises_retry(code), "{code:?} says its own next step");
        }
    }

    #[test]
    fn a_test_and_a_connection_answer_with_the_repository_id() {
        assert_eq!(
            Outcome::of(Action::Test, r#"{"id":"abc"}"#),
            Some(Outcome::Opens { id: "abc".into() })
        );
        assert_eq!(
            Outcome::of(Action::Connect, r#"{"id":"abc"}"#),
            Some(Outcome::Connected { id: "abc".into() })
        );
    }

    #[test]
    fn a_creation_answers_with_the_id_and_whether_it_was_saved() {
        assert_eq!(
            Outcome::of(Action::Create, r#"{"id":"abc","saved":true}"#),
            Some(Outcome::Created {
                id: "abc".into(),
                saved: true
            })
        );
        assert_eq!(
            Outcome::of(Action::Create, r#"{"id":"abc","saved":false}"#),
            Some(Outcome::Created {
                id: "abc".into(),
                saved: false
            })
        );
    }

    #[test]
    fn an_answer_that_is_not_the_one_the_action_gets_is_no_outcome() {
        assert_eq!(Outcome::of(Action::Create, r#"{"id":"abc"}"#), None);
        assert_eq!(Outcome::of(Action::Test, "not json"), None);
        assert_eq!(Outcome::of(Action::Connect, "{}"), None);
    }

    #[test]
    fn only_a_connection_and_a_saved_creation_change_the_configuration() {
        let id = || "abc".to_owned();
        assert!(!Outcome::Opens { id: id() }.saved());
        assert!(Outcome::Connected { id: id() }.saved());
        assert!(
            Outcome::Created {
                id: id(),
                saved: true
            }
            .saved()
        );
        assert!(
            !Outcome::Created {
                id: id(),
                saved: false
            }
            .saved()
        );
    }

    #[test]
    fn a_password_source_reads_as_its_own_message_and_never_as_a_value() {
        let cases = [
            (PasswordSourceDto::Inline, msg::UI_PASSWORD_INLINE, vec![]),
            (
                PasswordSourceDto::File {
                    path: "/etc/pw".into(),
                },
                msg::UI_PASSWORD_FILE,
                vec![("path", text("/etc/pw"))],
            ),
            (
                PasswordSourceDto::Keyring {
                    entry: "svc/name".into(),
                },
                msg::UI_PASSWORD_KEYRING,
                vec![("entry", text("svc/name"))],
            ),
            (
                PasswordSourceDto::Env {
                    name: "REPO_PW".into(),
                },
                msg::UI_PASSWORD_ENV,
                vec![("name", text("REPO_PW"))],
            ),
            (
                PasswordSourceDto::Command {
                    program: "pass".into(),
                },
                msg::UI_PASSWORD_COMMAND,
                vec![("program", text("pass"))],
            ),
        ];
        for (source, id, args) in cases {
            assert_eq!(password_source(&source), (id, args), "{source:?}");
        }
    }

    #[test]
    fn a_repositorys_warnings_read_as_notes_in_the_catalogs_words_naming_the_setting() {
        let localizer = Localizer::new(&[]);
        let warning = |field: &str| WarningDto {
            code: api::WarningCode::ConfigInlineSecret,
            args: [("field".to_owned(), text(field))].into(),
        };
        let notes = repository_notes(
            &localizer,
            &[warning("password"), warning("options.secret_access_key")],
        );
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes[0].contains("The password of this repository is written as text"),
            "{notes:?}"
        );
        assert!(notes[1].contains("options.secret_access_key"), "{notes:?}");
    }

    #[test]
    fn a_repository_with_no_warnings_has_no_notes() {
        assert_eq!(
            repository_notes(&Localizer::new(&[]), &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_sentence_marks_the_engines_words_where_they_stand() {
        assert_eq!(
            Sentence::new(
                "The operation failed: wrong password.",
                Some("wrong password")
            ),
            Sentence {
                before: "The operation failed: ".to_owned(),
                detail: Some(("wrong password".to_owned(), ".".to_owned())),
            }
        );
    }

    #[test]
    fn the_engines_words_are_found_between_the_marks_the_catalog_put_around_them() {
        // A word as short as `a` is in the catalog's own text too.
        let text = "The operation failed: \u{2068}a\u{2069}";
        assert_eq!(
            Sentence::new(text, Some("a")),
            Sentence {
                before: "The operation failed: \u{2068}".to_owned(),
                detail: Some(("a".to_owned(), "\u{2069}".to_owned())),
            }
        );
    }

    #[test]
    fn without_marks_the_engines_words_are_where_they_stand_last() {
        assert_eq!(
            Sentence::new("The operation failed: a", Some("a")),
            Sentence {
                before: "The operation failed: ".to_owned(),
                detail: Some(("a".to_owned(), String::new())),
            }
        );
    }

    #[test]
    fn a_sentence_with_no_detail_or_not_holding_it_is_one_plain_run() {
        let plain = |text: &str| Sentence {
            before: text.to_owned(),
            detail: None,
        };
        assert_eq!(
            Sentence::new("Nothing to add.", None),
            plain("Nothing to add.")
        );
        assert_eq!(
            Sentence::new("The operation failed: wrong", Some("wrong password")),
            plain("The operation failed: wrong"),
            "a detail the catalog cut short is not marked"
        );
        assert_eq!(
            Sentence::new("The operation failed.", Some("")),
            plain("The operation failed."),
            "nothing to mark is no mark"
        );
    }

    #[test]
    fn only_a_field_a_form_can_leave_empty_and_be_refused_has_a_required_message() {
        assert_eq!(
            Field::Name.required(),
            Some(msg::UI_REPOSITORIES_NAME_REQUIRED)
        );
        assert_eq!(
            Field::Location.required(),
            Some(msg::UI_REPOSITORIES_LOCATION_REQUIRED)
        );
        assert_eq!(
            Field::Password.required(),
            Some(msg::UI_REPOSITORIES_PASSWORD_REQUIRED)
        );
        assert_eq!(
            Field::HotStorage.required(),
            None,
            "hot storage is optional"
        );
    }

    #[test]
    fn each_outcome_is_shown_with_what_the_reader_needs_and_announced_without_the_id() {
        let opens = Outcome::Opens { id: "abc".into() };
        assert_eq!(
            opens.shown("copy"),
            (
                msg::UI_REPOSITORIES_OPENS_WITH_ID,
                vec![("id", text("abc"))]
            )
        );
        assert_eq!(
            opens.announced("copy"),
            (msg::UI_REPOSITORIES_OPENS, vec![])
        );
        let created = Outcome::Created {
            id: "abc".into(),
            saved: false,
        };
        assert_eq!(
            created.shown("copy"),
            (
                msg::UI_REPOSITORIES_CREATED_WITH_ID,
                vec![("id", text("abc"))]
            )
        );
        assert_eq!(
            created.announced("copy"),
            (msg::UI_REPOSITORIES_CREATED, vec![])
        );
        let connected = Outcome::Connected { id: "abc".into() };
        let said = (
            msg::UI_REPOSITORIES_CONNECTED,
            vec![("alias", text("copy"))],
        );
        assert_eq!(connected.shown("copy"), said);
        assert_eq!(connected.announced("copy"), said);
    }

    #[test]
    fn a_sentence_is_whole_again_as_plain_text() {
        let text = "The operation failed: wrong password.";
        assert_eq!(Sentence::new(text, Some("wrong password")).plain(), text);
        assert_eq!(Sentence::new(text, None).plain(), text);
    }

    #[test]
    fn a_rows_create_posts_to_the_route_of_its_saved_repository() {
        assert_eq!(create_saved_path("demo"), "/api/v1/repositories/demo/init");
        assert_eq!(
            create_saved_path("off-site-2"),
            "/api/v1/repositories/off-site-2/init"
        );
    }

    #[test]
    fn a_rows_create_button_is_named_after_its_row() {
        assert_eq!(
            create_label("demo"),
            (msg::UI_REPOSITORY_CREATE, vec![("alias", text("demo"))])
        );
        assert_eq!(english_of(create_label("demo")), "Create demo");
    }

    #[test]
    fn a_rows_create_answers_with_the_repository_id() {
        assert_eq!(
            RowCreated::of(r#"{"id":"abc"}"#),
            Some(RowCreated { id: "abc".into() })
        );
        assert_eq!(RowCreated::of("not json"), None);
        assert_eq!(RowCreated::of("{}"), None);
    }

    #[test]
    fn a_row_created_is_confirmed_by_name_and_its_id_said_apart() {
        assert_eq!(
            RowCreated::confirmation("demo"),
            (msg::UI_REPOSITORY_CREATED, vec![("alias", text("demo"))])
        );
        assert_eq!(
            english_of(RowCreated::confirmation("demo")),
            "Created demo."
        );
        let created = RowCreated { id: "abc".into() };
        assert_eq!(
            created.id_sentence(),
            (msg::UI_REPOSITORY_CREATED_ID, vec![("id", text("abc"))])
        );
        assert_eq!(english_of(created.id_sentence()), "Its ID is abc.");
    }

    #[test]
    fn a_repository_already_there_is_refused_in_the_rows_own_words() {
        let said = row_refusal(ErrorCode::RepositoryExists, "demo");
        assert_eq!(
            said,
            Some((
                msg::UI_REPOSITORY_ALREADY_THERE,
                vec![("alias", text("demo"))]
            ))
        );
        assert!(
            english_of(said.unwrap_or((msg::UI_REPOSITORY_CREATE, vec![])))
                .starts_with("There is already a repository at the location of")
        );
    }

    #[test]
    fn any_other_refusal_of_a_rows_create_reads_as_the_daemons_sentence() {
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::RepositoryUnknown,
            ErrorCode::SecretMissing,
            ErrorCode::EngineUnavailable,
            ErrorCode::InvalidBody,
        ] {
            assert_eq!(row_refusal(code, "demo"), None, "{code:?}");
        }
    }

    fn english_of((id, args): (MessageId, Vec<(&'static str, MsgArg)>)) -> String {
        crate::routes::text_with(&Localizer::new(&[]), id, &args)
    }
}
