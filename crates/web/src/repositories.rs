//! The decisions behind the Repositories page (technical design §4.4): what a
//! form is missing, the request each button sends, which field a refusal
//! belongs to, what a password source reads as, and what an answer means for
//! the form and the list. Host-tested here; the DOM glue that holds the
//! signals and sends the requests is `routes.rs` and `ws.rs`.

use std::collections::BTreeMap;

use api::{
    ConnectRepositoryDto, ErrorCode, InitRepositoryDto, MsgArg, PasswordDto, PasswordSourceDto,
    RepositoryIdDto, RepositoryInitDto, RepositorySettingsDto, TestRepositoryDto,
};
use l10n::{MessageId, msg};

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

/// What a person has typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Form {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) hot_storage: String,
    pub(crate) password: String,
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

/// The field a refusal is about, when it is about one: a name the daemon will
/// not take, or a secret it could not read.
pub(crate) fn field_of(code: ErrorCode, args: &[(String, MsgArg)]) -> Option<Field> {
    match code {
        ErrorCode::RepositoryAliasTaken | ErrorCode::RepositoryAliasInvalid => Some(Field::Name),
        ErrorCode::SecretUnresolved => {
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

    /// `text` with `detail` marked, when `detail` is in it; a text that
    /// holds none of it (the catalog cut a long one short) is one plain run.
    pub(crate) fn new(text: &str, detail: Option<&str>) -> Self {
        let marked = detail
            .filter(|detail| !detail.is_empty())
            .and_then(|detail| text.find(detail).map(|at| (detail, at)));
        match marked {
            Some((detail, at)) => Self {
                before: text.get(..at).unwrap_or_default().to_owned(),
                detail: Some((
                    detail.to_owned(),
                    text.get(at + detail.len()..).unwrap_or_default().to_owned(),
                )),
            },
            None => Self {
                before: text.to_owned(),
                detail: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form {
            name: "copy".to_owned(),
            location: "/srv/backups/main".to_owned(),
            hot_storage: String::new(),
            password: "not-a-real-password".to_owned(),
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
    fn an_unreadable_secret_belongs_to_the_field_that_holds_it() {
        let field =
            |name: &str| field_of(ErrorCode::SecretUnresolved, &[("field".into(), text(name))]);
        assert_eq!(field("password"), Some(Field::Password));
        assert_eq!(field("repository"), Some(Field::Location));
        assert_eq!(field("repo-hot"), Some(Field::HotStorage));
        assert_eq!(
            field("options.token"),
            None,
            "the form has no field for an option"
        );
        assert_eq!(field_of(ErrorCode::SecretUnresolved, &[]), None);
        assert_eq!(
            field_of(
                ErrorCode::SecretUnresolved,
                &[("field".into(), MsgArg::Count(1))]
            ),
            None
        );
    }

    #[test]
    fn a_refusal_about_the_repository_or_the_daemon_belongs_to_no_field() {
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::RepositoryExists,
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
            ErrorCode::SecretUnresolved,
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
}
