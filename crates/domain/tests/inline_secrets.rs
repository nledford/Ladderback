//! Which secrets a repository's settings hold as text (technical design
//! §10.2): a password written in a location, the password when it is inline,
//! and the options whose names say they are credentials, unless a reference
//! supplies them.

use std::collections::BTreeMap;

use domain::RepositoryAlias;
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Location, Locations, Maintenance, RepositoryConfig,
    RepositorySettings,
};
use domain::secrets::{
    FileRead, InlineSecret, KeyringEntry, Redactor, SecretRef, SecretValue, rclone_location,
};

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

// --- A password in a location (#190) ----------------------------------------

fn located(repository: &str) -> RepositorySettings {
    let mut demo = settings(env());
    demo.repository = repository.to_owned();
    demo
}

#[test]
fn a_password_in_the_url_of_a_location_is_an_inline_secret_in_each_half() {
    let mut demo = located("rest:https://demo:not-a-real-password@backup.example.com/nas");
    demo.repo_hot = Some("rest:http://:not-a-real-password@hot.example.com:8000/".to_owned());
    assert_eq!(
        demo.inline_secrets(),
        [InlineSecret::Location, InlineSecret::LocationHot]
    );
}

#[test]
fn a_location_is_named_as_the_configuration_names_its_setting_and_never_shown() {
    assert_eq!(InlineSecret::Location.field(), "repository");
    assert_eq!(InlineSecret::LocationHot.field(), "repo-hot");
}

#[test]
fn a_secret_in_a_location_is_found_in_the_location_that_holds_it() {
    let mut demo = located("rest:https://demo:a@backup.example.com/nas");
    demo.repo_hot = Some("rclone::sftp,pass=b:/hot".to_owned());
    assert_eq!(
        demo.location_of(&InlineSecret::Location),
        Some("rest:https://demo:a@backup.example.com/nas")
    );
    assert_eq!(
        demo.location_of(&InlineSecret::LocationHot),
        Some("rclone::sftp,pass=b:/hot")
    );
    for elsewhere in [
        InlineSecret::Password,
        InlineSecret::Option("token".to_owned()),
        InlineSecret::OptionHot("token".to_owned()),
        InlineSecret::OptionCold("token".to_owned()),
    ] {
        assert_eq!(demo.location_of(&elsewhere), None, "{elsewhere:?}");
    }
    demo.repo_hot = None;
    assert_eq!(demo.location_of(&InlineSecret::LocationHot), None);
}

#[test]
fn an_rclone_location_is_one_that_starts_with_rclone() {
    // rustic hands it to rclone as an argument, so a reference can't keep a
    // credential out of it: rclone's own configuration file does.
    for location in [
        "rclone:demo-remote:backups",
        "rclone::sftp,pass=x:/b",
        "rclone:",
    ] {
        assert!(rclone_location(location), "{location}");
    }
    for location in [
        "rest:https://demo:x@backup.example.com/nas",
        "/srv/rclone:demo",
        "local:rclone::s3,token=x",
        "Rclone::s3,token=x",
        "opendal:rclone",
        "",
    ] {
        assert!(!rclone_location(location), "{location}");
    }
}

#[test]
fn the_locations_come_first_then_the_password_then_the_options() {
    let mut demo = located("rest:https://demo:a@backup.example.com/nas");
    demo.repo_hot = Some("rest:https://demo:b@hot.example.com/nas".to_owned());
    demo.password = inline("not-a-real-password");
    demo.options = options(&[("token", "c")]);
    assert_eq!(
        demo.inline_secrets(),
        [
            InlineSecret::Location,
            InlineSecret::LocationHot,
            InlineSecret::Password,
            InlineSecret::Option("token".to_owned()),
        ]
    );
}

#[test]
fn a_password_is_text_however_it_is_encoded() {
    for location in [
        // Percent-encoded, it is still the password, written in the file.
        "rest:https://demo:%6Eot-real@backup.example.com/nas",
        "rest:https://demo:%@backup.example.com/nas",
        // Encoded or escaped, a reference is text: nothing replaces it.
        "rest:https://demo:%24%7BDEMO_PASSWORD%7D@backup.example.com/nas",
        "rest:https://demo:$${DEMO_PASSWORD}@backup.example.com/nas",
        // A reference that is not valid is text as written.
        "rest:https://demo:${not valid}@backup.example.com/nas",
        // A user name with a reference in it doesn't make the password one.
        "rest:https://${DEMO_USER}:not-real@backup.example.com/nas",
        // The authority ends at the first `/`, so its last `@` ends the
        // userinfo, whatever the user name holds.
        "rest:https://demo@corp:not-real@backup.example.com/nas@x",
        "rest:https://démo:nöt-réal@backup.example.com/nas",
    ] {
        assert_eq!(
            located(location).inline_secrets(),
            [InlineSecret::Location],
            "{location}"
        );
    }
}

#[test]
fn a_location_with_no_password_written_in_it_is_not_one() {
    for location in [
        "/srv/demo-repo",
        "s3:s3.example.net/demo",
        "opendal:s3",
        "rest:https://backup.example.com/nas",
        // A user name alone, or with an empty password, holds no password.
        "rest:https://demo@backup.example.com/nas",
        "rest:https://demo:@backup.example.com/nas",
        "rest:https://demo@backup.example.com/a:b@c",
        // A password that a reference gives, whole or in part.
        "rest:https://demo:${DEMO_PASSWORD}@backup.example.com/nas",
        "rest:https://demo:${file:/run/demo/password}@backup.example.com/nas",
        "rest:https://demo:pre-${DEMO_PASSWORD}@backup.example.com/nas",
        "${DEMO_REPOSITORY}",
        // An `@` after the authority is in the path, the query or the
        // fragment, and a `:` before it is a port's.
        "rest:https://backup.example.com:8000/nas@2026",
        "rest:https://backup.example.com:8000?at=a:b@c",
        "rest:https://backup.example.com:8000#a:b@c",
        "rest:https://backup.example.com:8000\\a:b@c",
        // No `://`, no URL.
        "/srv/demo:not-real@repo",
        "rclone:demo-remote:backups",
    ] {
        assert_eq!(located(location).inline_secrets(), [], "{location}");
    }
}

#[test]
fn a_credential_in_an_rclone_connection_string_is_an_inline_secret() {
    for location in [
        "rclone::s3,provider=AWS,secret_access_key=not-a-real-key:demo-bucket",
        "rclone::s3,provider=AWS,session_token=not-a-real-token:demo-bucket",
        // rclone's SFTP, FTP and SMB backends name their password `pass`.
        "rclone::sftp,host=nas.example.com,pass=not-real:/backups",
        "rclone::webdav,url='https://demo:not-real@dav.example.com':",
        // A named remote with a setting of its own.
        "rclone:demo-remote,pass=not-real:backups",
        // A quoted value may hold `,` and `:`.
        "rclone::sftp,host=nas.example.com,pass='not:real,at-all':/backups",
        "rclone::sftp,host=nas.example.com,pass=\"not:real\":/backups",
        // A quote written twice inside quotes stands for itself.
        "rclone::sftp,host='it''s:22',pass=not-real:/backups",
        // A quote only starts a value: elsewhere it is text.
        "rclone::sftp,host=it's,pass=not-real:/backups",
        "rclone::sftp,PASS=not-real:/backups",
        // A value whose quote never closes is kept whole, quote and all.
        "rclone::sftp,pass='x",
        "rclone::sftp,pass='\"",
    ] {
        assert_eq!(
            located(location).inline_secrets(),
            [InlineSecret::Location],
            "{location}"
        );
    }
    for location in [
        "rclone::s3,provider=AWS,access_key_id=AKIADEMO:demo-bucket",
        "rclone::s3,secret_access_key=${DEMO_KEY}:demo-bucket",
        "rclone::s3,secret_access_key=${file:/run/demo/key}:demo-bucket",
        "rclone::s3,secret_access_key=:demo-bucket",
        "rclone::sftp,use-password=true:/backups",
        // `pass` is a credential only as the whole name of the option.
        "rclone::demo,bypass_cache=not-real:/backups",
        "rclone::ftp,passive=true:/backups",
        // What follows the first `:` outside quotes is the path, not settings.
        "rclone:demo-remote:backups/token=2024",
        "rclone::sftp,host=nas.example.com:/backups,pass=not-real",
        "rclone::sftp,host='nas:22':/backups,pass=not-real",
        // A quoted value with nothing in it is no credential.
        "rclone::sftp,pass='':/backups",
        "rclone::sftp,pass=\"\":/backups",
        // A key that is a reference names nothing in the file.
        "rclone::s3,${DEMO_KEY_NAME}=not-real:demo-bucket",
    ] {
        assert_eq!(located(location).inline_secrets(), [], "{location}");
    }
}

#[test]
fn a_credential_under_a_name_of_rclones_own_is_an_inline_secret() {
    for location in [
        "rclone::sftp,host=nas.example.com,key_file_pass=not-real:/backups",
        "rclone::sftp,host=nas.example.com,key_pem='not-real':/backups",
        "rclone::azureblob,account=demo,sas_url='https://demo.example.com/?sig=not-real':demo",
        "rclone::azureblob,connection_string='AccountKey=not-real':demo",
        "rclone::s3,sse_customer_key_base64=not-real:demo-bucket",
        // B2's application key, Swift's API key, Azure Blob's shared key.
        "rclone::b2,account=demo,key=not-real:demo-bucket",
        "rclone::storj,access_grant=not-real:demo-bucket",
        "rclone::filen,email=demo@example.com,master_keys=not-real:/backups",
        "rclone::sugarsync,authorization=not-real:/backups",
        "rclone::webdav,url='https://dav.example.com',headers='Authorization,Bearer not-real':/b",
        "rclone::protondrive,username=demo,2fa=123456:/backups",
    ] {
        assert_eq!(
            located(location).inline_secrets(),
            [InlineSecret::Location],
            "{location}"
        );
    }
}

/// A setting's value may be a location of its own, whose credentials are
/// written in the configuration as much as the location's own are.
#[test]
fn a_credential_of_a_location_in_an_rclone_setting_is_an_inline_secret() {
    for location in [
        "rclone::crypt,remote=':s3,secret_access_key=not-real:bucket':path",
        "rclone::crypt,remote='demo,pass=not-real:bucket':path",
        "rclone::crypt,remote=':crypt,remote='':s3,secret_access_key=not-real:b'':p':path",
    ] {
        assert_eq!(
            located(location).inline_secrets(),
            [InlineSecret::Location],
            "{location}"
        );
    }
    for location in [
        "rclone::crypt,remote=':s3,secret_access_key=${DEMO_KEY}:bucket':path",
        "rclone::crypt,remote=':s3,access_key_id=AKIADEMO:bucket':path",
        // No `:` outside quotes: rclone reads a local path.
        "rclone::crypt,remote='demo,pass=not-real':path",
    ] {
        assert_eq!(located(location).inline_secrets(), [], "{location}");
    }
}

#[test]
fn an_option_whose_value_is_a_url_with_a_password_is_an_inline_secret() {
    let mut demo = settings(env());
    demo.options = options(&[
        ("endpoint", "https://demo:not-real@minio.example.com"),
        ("proxy", "http://demo@proxy.example.com:3128"),
        ("region", "eu-demo-1"),
    ]);
    demo.options_hot = options(&[("endpoint", "https://demo:${DEMO_KEY}@minio.example.com")]);
    assert_eq!(
        demo.inline_secrets(),
        [InlineSecret::Option("endpoint".to_owned())]
    );
}

#[test]
fn hostile_locations_end_without_a_panic() {
    // One pass over each is milliseconds; a reading quadratic in the length
    // would run for minutes on these, past the test runner's time limit.
    let long = [
        "@".repeat(200_000),
        ":".repeat(200_000),
        "://".repeat(100_000),
        format!("rest:https://{}@host", ":".repeat(200_000)),
        format!("rest:https://demo:{}@host", "${A}".repeat(50_000)),
        format!("rclone:{}", ",a=b".repeat(100_000)),
        format!("rclone::sftp,pass='{}", "'',".repeat(100_000)),
        format!("rest:https://{}", "${".repeat(100_000)),
    ];
    let short = [
        "",
        "://",
        "://@",
        "://:@",
        ":@",
        "rest:",
        "rest:https://",
        "rclone:",
        "rclone:=",
        "rclone:,:=:",
        "https://é:€@",
        "https://:\u{0}@h",
        "${",
        "${}",
        "$${",
        "https://${x",
    ];
    for location in long.iter().map(String::as_str).chain(short) {
        let _ = located(location).inline_secrets();
    }
    assert_eq!(
        located("https://:\u{0}@h").inline_secrets(),
        [InlineSecret::Location]
    );
}

// --- What a configuration holds as written, as values (#853) ----------------

/// What a redactor of `secrets` leaves of `text`.
fn masked_by(secrets: &[SecretValue], text: &str) -> String {
    Redactor::new(secrets).redact(text)
}

#[test]
fn the_secrets_settings_hold_as_written_are_masked_and_nothing_else() {
    let mut demo = located("rest:https://demo:canary-url@backup.example.com/nas");
    demo.password = inline("canary-password");
    demo.repo_hot = Some("rest:https://demo:canary-hot-url@hot.example.com/".to_owned());
    demo.options = options(&[("secret_access_key", "canary-key"), ("region", "eu-demo")]);
    demo.options_hot = options(&[("session-token", "canary-token")]);
    demo.options_cold = options(&[("account_key", "canary-cold-key")]);
    assert_eq!(
        masked_by(
            &demo.written_secrets(),
            "canary-password canary-url canary-hot-url canary-key canary-token canary-cold-key eu-demo"
        ),
        "*** *** *** *** *** *** eu-demo"
    );
}

#[test]
fn a_password_read_from_a_source_is_masked_only_as_written() {
    let demo = settings(env());
    assert_eq!(
        masked_by(&demo.written_secrets(), "DEMO_PASSWORD"),
        "DEMO_PASSWORD"
    );
    assert!(
        demo.written_secrets()
            .iter()
            .all(|secret| secret.expose().is_empty()),
        "a reference adds no secret of its own"
    );
}

fn repository(settings: RepositorySettings, further: Vec<Location>) -> RepositoryConfig {
    RepositoryConfig {
        settings,
        locations: Locations {
            first_when: None,
            further,
        },
        id: None,
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    }
}

#[test]
fn the_configuration_holds_the_written_secrets_of_every_repository_at_every_location() {
    let mut first = settings(inline("canary-first"));
    first.options = options(&[("secret_access_key", "canary-first-key")]);
    let further = Location {
        repository: "rest:https://demo:canary-further-url@far.example.com/nas".to_owned(),
        options: options(&[("secret_access_key", "canary-further-key")]),
        ..Location::default()
    };
    let mut second = located("/srv/second");
    second.password = inline("canary-second");
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([
            (
                RepositoryAlias::parse("first").expect("an alias"),
                repository(first, vec![further]),
            ),
            (
                RepositoryAlias::parse("second").expect("an alias"),
                repository(second, Vec::new()),
            ),
        ]),
        Vec::new(),
    )
    .expect("a valid configuration");
    assert_eq!(
        masked_by(
            &config.written_secrets(),
            "canary-first canary-first-key canary-further-url canary-further-key canary-second"
        ),
        "*** *** *** *** ***"
    );
}

// --- A credential after a flag of an rclone-command (#1136) -----------------

/// The secrets settings hold as text whose `options` start rclone with
/// `command`.
fn rclone_command(command: &str) -> Vec<InlineSecret> {
    let mut demo = located("rclone:demo:backups");
    demo.options = options(&[("rclone-command", command)]);
    demo.inline_secrets()
}

#[test]
fn a_credential_after_a_flag_of_the_rclone_command_is_named_by_the_flag() {
    for (command, flag) in [
        ("rclone serve restic --pass=canary-pass", "pass"),
        ("rclone serve restic --pass canary-pass", "pass"),
        ("rclone --password canary-pass serve restic", "password"),
        (
            "rclone --s3-secret-access-key='canary pass' serve restic",
            "s3-secret-access-key",
        ),
        (
            "rclone \"--s3-session-token=canary pass\" serve restic",
            "s3-session-token",
        ),
        ("rclone --b2-key \"canary-pass\" serve restic", "b2-key"),
        (
            "rclone --azureblob-key=canary-pass serve restic",
            "azureblob-key",
        ),
        ("rclone --sftp-pass canary-pass serve restic", "sftp-pass"),
        (
            "rclone --sftp-key-file-pass=canary-pass serve restic",
            "sftp-key-file-pass",
        ),
        (
            "rclone --sftp-key-pem=canary-pass serve restic",
            "sftp-key-pem",
        ),
        (
            "rclone --drive-token canary-pass serve restic",
            "drive-token",
        ),
        (
            "rclone --drive-client-secret canary-pass serve restic",
            "drive-client-secret",
        ),
        ("rclone --rc-pass canary-pass serve restic", "rc-pass"),
        (
            "rclone --webdav-bearer-token=canary-pass serve restic",
            "webdav-bearer-token",
        ),
        (
            "rclone --azureblob-sas-url=canary-pass serve restic",
            "azureblob-sas-url",
        ),
        (
            "rclone --crypt-password2 canary-pass serve restic",
            "crypt-password2",
        ),
        (
            "rclone --s3-sse-customer-key canary-pass serve restic",
            "s3-sse-customer-key",
        ),
        // A word that starts `--` after the flag is still its value.
        ("rclone serve restic --pass --canary-pass", "pass"),
    ] {
        assert_eq!(
            rclone_command(command),
            [InlineSecret::RcloneFlag(flag.to_owned())],
            "{command}"
        );
    }
}

/// Each of these words names a command, a file, an address or a switch,
/// never a credential, so only the `--rc-pass` after it is one.
#[test]
fn a_flag_that_names_a_command_a_file_or_a_switch_holds_no_credential() {
    for words in [
        "--password-command 'pass show canary-pass'",
        "--password-command=canary-pass",
        "--webdav-bearer-token-command='cat canary-pass'",
        "--sftp-key-file /home/demo/.ssh/canary-pass",
        "--sftp-pubkey-file=/home/demo/.ssh/canary-pass.pub",
        "--oos-sse-customer-key-file=/home/demo/canary-pass",
        "--gcs-service-account-file /home/demo/canary-pass.json",
        "--key /home/demo/tls/canary-pass.pem",
        "--client-key=/home/demo/tls/canary-pass.pem",
        "--rc-key /home/demo/tls/canary-pass.pem",
        "--rc-htpasswd /home/demo/canary-pass",
        "--htpasswd=/home/demo/canary-pass",
        "--ask-password=false",
        "--sftp-ask-password",
        "--drive-client-credentials",
        "--drive-token-url https://oauth.example.net/canary-pass",
        "--s3-access-key-id canary-pass",
        "--sftp-key-use-agent",
        "--dump-headers",
        "--sftp-pin-host-key",
        "--seafile-2fa",
        "--user canary-pass",
        "-pass canary-pass",
        "'--pass canary-pass'",
    ] {
        let command = format!("rclone {words} --rc-pass=canary-pass serve restic");
        assert_eq!(
            rclone_command(&command),
            [InlineSecret::RcloneFlag("rc-pass".to_owned())],
            "{command}"
        );
    }
}

#[test]
fn a_flag_with_no_credential_after_it_holds_none() {
    for command in [
        // Nothing after it, or nothing in it.
        "rclone --rc-pass=canary-pass serve restic --pass",
        "rclone --rc-pass=canary-pass serve restic --pass=",
        "rclone --rc-pass=canary-pass serve restic --pass ''",
        // A comment is no word of the command.
        "rclone --rc-pass=canary-pass serve restic # --pass canary-pass",
    ] {
        assert_eq!(
            rclone_command(command),
            [InlineSecret::RcloneFlag("rc-pass".to_owned())],
            "{command}"
        );
    }
}

#[test]
fn a_flag_given_twice_is_named_once() {
    assert_eq!(
        rclone_command("rclone --pass=canary-one serve restic --pass canary-two --rc-pass=canary"),
        [
            InlineSecret::RcloneFlag("pass".to_owned()),
            InlineSecret::RcloneFlag("rc-pass".to_owned())
        ]
    );
}

/// A command with a reference in it is refused when a job starts, as
/// every option rustic runs as a command is (§10.2), and its words are
/// known only then; one that can't be split into words is rustic's to
/// refuse. Neither is read for a flag.
#[test]
fn a_command_with_a_reference_or_that_cannot_be_split_is_not_read_for_flags() {
    let mut demo = located("rclone:demo:backups");
    demo.options = options(&[(
        "rclone-command",
        "rclone serve restic --pass=${RCLONE_PASS}",
    )]);
    demo.options_hot = options(&[("rclone-command", "rclone --pass='canary-pass serve")]);
    demo.options_cold = options(&[("rclone-command", "rclone --pass=canary-pass serve")]);
    assert_eq!(
        demo.inline_secrets(),
        [InlineSecret::RcloneFlagCold("pass".to_owned())]
    );
}

#[test]
fn only_the_rclone_command_is_read_for_flags() {
    let mut demo = located("/srv/demo-repo");
    demo.options = options(&[
        ("post-create-command", "notify --pass canary-pass"),
        (
            "rclone-command",
            "rclone --rc-pass canary-pass serve restic",
        ),
    ]);
    assert_eq!(
        demo.inline_secrets(),
        [InlineSecret::RcloneFlag("rc-pass".to_owned())]
    );
}

#[test]
fn a_credential_in_the_rclone_command_of_each_half_is_named_by_its_set_and_flag() {
    let mut demo = located("rclone:demo:backups");
    demo.repo_hot = Some("rclone:hot:backups".to_owned());
    demo.options_hot = options(&[("rclone-command", "rclone --pass canary-pass serve restic")]);
    demo.options_cold = options(&[(
        "rclone-command",
        "rclone --s3-session-token=canary-pass serve restic",
    )]);
    let found = demo.inline_secrets();
    assert_eq!(
        found,
        [
            InlineSecret::RcloneFlagHot("pass".to_owned()),
            InlineSecret::RcloneFlagCold("s3-session-token".to_owned())
        ]
    );
    let named: Vec<(String, Option<String>)> = found
        .iter()
        .map(|secret| (secret.field(), secret.flag()))
        .collect();
    assert_eq!(
        named,
        [
            (
                "options-hot.rclone-command".to_owned(),
                Some("--pass".to_owned())
            ),
            (
                "options-cold.rclone-command".to_owned(),
                Some("--s3-session-token".to_owned())
            ),
        ]
    );
    assert!(!format!("{found:?}").contains("canary-pass"), "{found:?}");
}

#[test]
fn a_flag_is_named_as_one_bounded_line_and_other_secrets_have_none() {
    let found = rclone_command(&format!(
        "rclone --s3-secret{}=canary-pass serve restic",
        "x".repeat(500)
    ));
    let flag = found.first().and_then(InlineSecret::flag).expect("a flag");
    assert!(flag.starts_with("--s3-secretxxx"), "{flag}");
    assert_eq!(flag.chars().count(), "--".len() + 80);
    assert_eq!(
        found.first().map(InlineSecret::field).as_deref(),
        Some("options.rclone-command")
    );
    for other in [
        InlineSecret::Password,
        InlineSecret::Location,
        InlineSecret::Option("pass".to_owned()),
    ] {
        assert_eq!(other.flag(), None, "{other:?}");
    }
}
