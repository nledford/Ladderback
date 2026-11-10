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
fn a_value_with_a_dollar_brace_that_is_no_reference_is_still_written_inline() {
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
