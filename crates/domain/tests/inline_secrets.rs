//! Which secrets a repository's settings hold as text (technical design
//! §10.2): the password when it is inline, and the options whose names say
//! they are credentials, unless a reference supplies them.

use std::collections::BTreeMap;

use domain::planning::RepositorySettings;
use domain::secrets::{FileRead, InlineSecret, KeyringEntry, SecretRef, SecretValue};

fn settings(password: SecretRef) -> RepositorySettings {
    RepositorySettings {
        repository: "s3:s3.example.net/demo".to_owned(),
        repo_hot: None,
        password,
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    }
}

fn inline(text: &str) -> SecretRef {
    SecretRef::Inline(SecretValue::new(text))
}

fn env() -> SecretRef {
    SecretRef::Env("DEMO_PASSWORD".to_owned())
}

fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn a_password_written_in_the_settings_is_an_inline_secret() {
    assert_eq!(
        settings(inline("not-a-real-password")).inline_secrets(),
        [InlineSecret::Password]
    );
}

#[test]
fn an_option_that_names_a_credential_is_one_in_each_of_the_three_sets() {
    let mut demo = settings(env());
    demo.options = options(&[("secret_access_key", "not-a-real-key")]);
    demo.options_hot = options(&[("session-token", "not-a-real-token")]);
    demo.options_cold = options(&[("account_key", "not-a-real-key")]);
    assert_eq!(
        demo.inline_secrets(),
        [
            InlineSecret::Option("secret_access_key".to_owned()),
            InlineSecret::OptionHot("session-token".to_owned()),
            InlineSecret::OptionCold("account_key".to_owned()),
        ]
    );
}

#[test]
fn a_value_that_is_not_a_valid_template_is_still_text_in_the_config() {
    let mut demo = settings(env());
    demo.options = options(&[("token", "not-a-${real-token")]);
    assert_eq!(
        demo.inline_secrets(),
        [InlineSecret::Option("token".to_owned())]
    );
}

#[test]
fn the_password_comes_first_and_each_set_is_in_the_order_of_its_names() {
    let mut demo = settings(inline("not-a-real-password"));
    demo.options = options(&[("token", "a"), ("password", "b")]);
    demo.options_cold = options(&[("token", "c")]);
    assert_eq!(
        demo.inline_secrets(),
        [
            InlineSecret::Password,
            InlineSecret::Option("password".to_owned()),
            InlineSecret::Option("token".to_owned()),
            InlineSecret::OptionCold("token".to_owned()),
        ]
    );
}

#[test]
fn a_secret_is_named_as_the_configuration_names_its_setting() {
    assert_eq!(InlineSecret::Password.field(), "password");
    assert_eq!(
        InlineSecret::Option("secret_access_key".to_owned()).field(),
        "options.secret_access_key"
    );
    assert_eq!(
        InlineSecret::OptionHot("token".to_owned()).field(),
        "options-hot.token"
    );
    assert_eq!(
        InlineSecret::OptionCold("token".to_owned()).field(),
        "options-cold.token"
    );
}

#[test]
fn a_password_read_from_a_source_is_not() {
    let sources = [
        env(),
        SecretRef::File {
            path: "/run/secrets/demo".to_owned(),
            read: FileRead::FirstLine,
        },
        SecretRef::Keyring(KeyringEntry::parse("ladderback/demo").unwrap()),
    ];
    for source in sources {
        assert_eq!(settings(source.clone()).inline_secrets(), [], "{source:?}");
    }
}

#[test]
fn a_secret_that_is_empty_is_nothing_to_move() {
    let mut empty = settings(inline(""));
    empty.options = options(&[("token", "")]);
    assert_eq!(empty.inline_secrets(), []);
}

#[test]
fn an_option_that_does_not_name_a_credential_is_not_one_whatever_it_holds() {
    let mut demo = settings(env());
    demo.options = options(&[("region", "eu-demo-1"), ("access_key_id", "AKIADEMO")]);
    assert_eq!(demo.inline_secrets(), []);
}

#[test]
fn a_credential_option_that_holds_a_reference_is_not_written_inline() {
    let mut demo = settings(env());
    demo.options = options(&[
        ("secret_access_key", "${DEMO_SECRET}"),
        ("token", "${file:/run/secrets/token}"),
    ]);
    assert_eq!(demo.inline_secrets(), []);
}

#[test]
fn an_option_name_with_a_line_break_or_without_end_is_shown_as_one_bounded_line() {
    // A name is what a person typed: it must not forge a log line, and no
    // length of it may make the line or the row's note unbounded.
    let forged = InlineSecret::Option("token\n[ERROR] backups disabled\u{1b}[0m".to_owned());
    assert_eq!(forged.field(), "options.token?[ERROR] backups disabled?[0m");
    let long = InlineSecret::OptionHot(format!("token{}", "x".repeat(500)));
    let field = long.field();
    assert!(field.starts_with("options-hot.tokenxxx"), "{field}");
    assert_eq!(field.chars().count(), "options-hot.".len() + 80);
}
