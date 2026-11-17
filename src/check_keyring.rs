//! `ladderback check-keyring` (#892, operations §12): reads, with this
//! program, each keyring entry the configuration's repositories take their
//! password from, once, and says for each whether it was read, or the code
//! and the message of its refusal. On macOS the keychain asks once for each
//! new build before it lets the build read an entry, so the question comes
//! at the terminal and not at the next backup nobody watches. It reads the
//! configuration the daemon reads (`LADDERBACK_CONFIG_DIR`, or the
//! platform's folder), only reads it, and prints no secret: what an entry
//! holds is dropped as soon as it is read (I4). What was read goes to
//! stdout; what couldn't be, to stderr, and it then ends with 1.

use std::io::{self, Write};
use std::process::ExitCode;

use app::keyring_check::KeyringCheck;
use l10n::Localizer;

/// Reads the keyring entries and says what came of each.
pub(crate) fn check_keyring(plain: bool) -> ExitCode {
    let _ = plain;
    todo!()
}

/// Says what `checked` came to: each entry read on `out`, each refused on
/// `err`; the status to end with.
fn report(
    localizer: &Localizer,
    checked: &KeyringCheck,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<ExitCode> {
    let _ = (localizer, checked, out, err);
    todo!()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use app::fakes::FakeSecrets;
    use app::keyring_check::{KeyringRead, check_keyring_entries};
    use domain::RepositoryAlias;
    use domain::execution::refusal::SettingRefusal;
    use domain::planning::{
        AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig,
        RepositorySettings,
    };
    use domain::secrets::{KeyringEntry, SecretRef};

    use super::*;

    fn entry(text: &str) -> KeyringEntry {
        KeyringEntry::parse(text).expect("an entry")
    }

    fn read(alias: &str, entry_text: &str, refused: Option<SettingRefusal>) -> KeyringRead {
        KeyringRead {
            repository: RepositoryAlias::parse(alias).expect("an alias"),
            entry: entry(entry_text),
            refused,
        }
    }

    /// What `report` printed of `checked`, on stdout and stderr, and the
    /// status it ended with.
    fn reported(checked: &KeyringCheck) -> (String, String, ExitCode) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = report(&Localizer::new(&[]), checked, &mut out, &mut err)
            .expect("the report is written");
        (
            String::from_utf8(out).expect("text"),
            String::from_utf8(err).expect("text"),
            status,
        )
    }

    #[test]
    fn an_entry_read_is_said_on_stdout_by_its_repository_and_entry() {
        let checked = KeyringCheck {
            reads: vec![read("nas", "ladderback/nas", None)],
        };
        let (out, err, status) = reported(&checked);
        assert!(
            out.starts_with("ladderback: nas: read the keyring entry"),
            "{out:?}"
        );
        assert!(out.contains("ladderback/nas"), "{out:?}");
        assert_eq!(out.lines().count(), 1, "{out:?}");
        assert_eq!(err, "");
        assert_eq!(status, ExitCode::SUCCESS);
    }

    #[test]
    fn an_entry_refused_is_said_on_stderr_with_its_code_and_message_and_fails() {
        let checked = KeyringCheck {
            reads: vec![
                read("nas", "ladderback/nas", None),
                read(
                    "usb",
                    "ladderback/usb",
                    Some(SettingRefusal::SecretKeyringUnavailable {
                        source: "ladderback/usb".to_owned(),
                        detail: Some("the keychain said no".to_owned()),
                    }),
                ),
            ],
        };
        let (out, err, status) = reported(&checked);
        assert!(out.contains("nas: read the keyring entry"), "{out:?}");
        assert!(!out.contains("usb"), "{out:?}");
        assert!(
            err.starts_with("ladderback: usb: could not read the keyring entry"),
            "{err:?}"
        );
        assert!(err.contains("(secret-keyring-unavailable)"), "{err:?}");
        // The refusal's own message, with what the platform said.
        assert!(err.contains("could not read the keyring entry"), "{err:?}");
        assert!(err.contains("the keychain said no"), "{err:?}");
        assert_eq!(err.lines().count(), 1, "{err:?}");
        assert_eq!(status, ExitCode::FAILURE);
    }

    #[test]
    fn a_configuration_with_no_keyring_entry_says_there_is_nothing_to_read() {
        let (out, err, status) = reported(&KeyringCheck::default());
        assert!(
            out.contains("no repository reads its password from the keyring"),
            "{out:?}"
        );
        assert_eq!(err, "");
        assert_eq!(status, ExitCode::SUCCESS);
    }

    /// I4: what an entry holds is nowhere in what the command prints.
    #[tokio::test]
    async fn nothing_an_entry_holds_is_printed() {
        let secrets = Arc::new(FakeSecrets::new());
        secrets.set_keyring_entry(&entry("ladderback/nas"), "CANARY-keyring-password");
        let repository = RepositoryConfig {
            settings: RepositorySettings {
                repository: "/srv/nas".to_owned(),
                repo_hot: None,
                password: SecretRef::Keyring(entry("ladderback/nas")),
                options: Default::default(),
                options_hot: Default::default(),
                options_cold: Default::default(),
            },
            locations: Default::default(),
            id: None,
            maintenance: Maintenance::Local,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        };
        let config = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            [(RepositoryAlias::parse("nas").expect("an alias"), repository)].into(),
            Vec::new(),
        )
        .expect("a configuration");
        let checked = check_keyring_entries(&config, secrets.as_ref()).await;
        let (out, err, status) = reported(&checked);
        assert!(out.contains("nas: read the keyring entry"), "{out:?}");
        assert!(!out.contains("CANARY"), "{out:?}");
        assert!(!err.contains("CANARY"), "{err:?}");
        assert!(!format!("{checked:?}").contains("CANARY"));
        assert_eq!(status, ExitCode::SUCCESS);
    }
}
