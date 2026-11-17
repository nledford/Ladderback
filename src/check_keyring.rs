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

use std::io::{self, Read as _, Write};
use std::path::Path;
use std::process::ExitCode;

use api::MsgArg;
use app::keyring_check::{KeyringCheck, KeyringUser, check_keyring_entries};
use app::ports::ConfigError;
use config_adapter::{MAX_CONFIG_BYTES, Schemas, parse};
use domain::execution::refusal::SettingRefusal;
use domain::planning::AgentConfig;
use l10n::{Localizer, msg};
use platform::dirs;

use crate::{finish, said, say, text};

/// Reads the keyring entries and says what came of each.
pub(crate) fn check_keyring(plain: bool) -> ExitCode {
    // Core dumps and debugger attach are off before any secret is read.
    let localizer = crate::start_protected(plain, None);
    finish(&localizer, run(&localizer))
}

fn run(localizer: &Localizer) -> io::Result<ExitCode> {
    let Some(folder) = dirs::config_dir() else {
        say(localizer, msg::CLI_NO_CONFIG_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let config = match read_config(&folder.join("config.toml")) {
        Ok(config) => config,
        Err(error) => {
            let refused = crate::composition::config_refused(error);
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let checked = runtime.block_on(check_keyring_entries(
        &config,
        &secrets::SystemSecrets::new(),
    ));
    report(
        localizer,
        &checked,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// The configuration in `file`, only read, never migrated: a daemon may be
/// running on it, and only the one that holds the data folder's lock may
/// rewrite it. No file is a configuration with no repository.
fn read_config(file: &Path) -> Result<AgentConfig, ConfigError> {
    let shown = file.display().to_string();
    let opened = match std::fs::File::open(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AgentConfig::default());
        }
        Err(error) => {
            return Err(ConfigError::Unavailable {
                file: shown,
                detail: error.to_string(),
            });
        }
    };
    let mut contents = String::new();
    opened
        .take(MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_string(&mut contents)
        .map_err(|error| ConfigError::Unavailable {
            file: shown.clone(),
            detail: error.to_string(),
        })?;
    if u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            file: shown,
            limit: MAX_CONFIG_BYTES,
        });
    }
    parse(file, &contents, &Schemas::production()).map(|parsed| parsed.config)
}

/// Says what `checked` came to: each entry read on `out`, each refused on
/// `err`; the status to end with.
fn report(
    localizer: &Localizer,
    checked: &KeyringCheck,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<ExitCode> {
    if checked.reads.is_empty() {
        out.write_all(said(localizer, msg::CLI_CHECK_KEYRING_NONE, &[]).as_bytes())?;
    }
    for read in &checked.reads {
        let entry = ("entry", text(&read.entry));
        let code = |refusal: &SettingRefusal| ("code", MsgArg::Text(refusal.code().to_owned()));
        match (&read.user, &read.refused) {
            (KeyringUser::Repository(alias), None) => {
                let args = [("repository", text(alias.as_str())), entry];
                out.write_all(said(localizer, msg::CLI_CHECK_KEYRING_READ, &args).as_bytes())?;
            }
            (KeyringUser::Repository(alias), Some(refusal)) => {
                let (error, args, detail) = app::announce::refused("password", refusal);
                let reason = localizer.error(error, &args, detail.as_deref());
                let args = [
                    ("repository", text(alias.as_str())),
                    entry,
                    code(refusal),
                    ("reason", text(reason)),
                ];
                err.write_all(said(localizer, msg::CLI_CHECK_KEYRING_REFUSED, &args).as_bytes())?;
            }
            (KeyringUser::Channel(name), None) => {
                let args = [("channel", text(name.as_str())), entry];
                let line = said(localizer, msg::CLI_CHECK_KEYRING_CHANNEL_READ, &args);
                out.write_all(line.as_bytes())?;
            }
            // A run's message speaks of a repository's password, so a
            // channel's entry is said by its code alone.
            (KeyringUser::Channel(name), Some(refusal)) => {
                let args = [("channel", text(name.as_str())), entry, code(refusal)];
                let line = said(localizer, msg::CLI_CHECK_KEYRING_CHANNEL_REFUSED, &args);
                err.write_all(line.as_bytes())?;
            }
        }
    }
    out.flush()?;
    err.flush()?;
    Ok(if checked.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use app::fakes::FakeSecrets;
    use app::keyring_check::{KeyringRead, KeyringUser, check_keyring_entries};
    use domain::RepositoryAlias;
    use domain::execution::refusal::SettingRefusal;
    use domain::notifications::ChannelName;
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
            user: KeyringUser::Repository(RepositoryAlias::parse(alias).expect("an alias")),
            entry: entry(entry_text),
            refused,
        }
    }

    fn channel_read(
        channel: &str,
        entry_text: &str,
        refused: Option<SettingRefusal>,
    ) -> KeyringRead {
        KeyringRead {
            user: KeyringUser::Channel(ChannelName::parse(channel).expect("a channel name")),
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
        assert!(err.contains("no keyring service it can use"), "{err:?}");
        assert!(err.contains("the keychain said no"), "{err:?}");
        assert_eq!(err.lines().count(), 1, "{err:?}");
        assert_eq!(status, ExitCode::FAILURE);
    }

    #[test]
    fn a_channels_entries_are_said_by_the_channel_and_entry() {
        let checked = KeyringCheck {
            reads: vec![
                channel_read("alerts", "ladderback/alerts", None),
                channel_read(
                    "alerts",
                    "ladderback/alerts-token",
                    Some(SettingRefusal::SecretMissing {
                        source: "ladderback/alerts-token".to_owned(),
                    }),
                ),
            ],
        };
        let (out, err, status) = reported(&checked);
        assert!(
            out.starts_with("ladderback: notification channel"),
            "{out:?}"
        );
        assert!(out.contains("alerts"), "{out:?}");
        assert!(out.contains(": read the keyring entry"), "{out:?}");
        assert!(out.contains("ladderback/alerts"), "{out:?}");
        assert_eq!(out.lines().count(), 1, "{out:?}");
        assert!(
            err.starts_with("ladderback: notification channel"),
            "{err:?}"
        );
        assert!(
            err.contains(": could not read the keyring entry"),
            "{err:?}"
        );
        assert!(err.contains("ladderback/alerts-token"), "{err:?}");
        assert!(err.contains("(secret-missing)"), "{err:?}");
        assert_eq!(err.lines().count(), 1, "{err:?}");
        assert_eq!(status, ExitCode::FAILURE);
    }

    #[test]
    fn a_configuration_with_no_keyring_entry_says_there_is_nothing_to_read() {
        let (out, err, status) = reported(&KeyringCheck::default());
        assert!(
            out.contains("no repository or notification channel takes a secret from the keyring"),
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
