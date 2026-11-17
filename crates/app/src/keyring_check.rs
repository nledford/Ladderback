//! Reading each keyring entry the configuration names, once, while someone
//! is there to answer the keyring (#892, technical design §10.2). On macOS
//! the keychain asks once for each new build of the program before it lets
//! that build read an entry; `ladderback check-keyring` reads them with the
//! program that runs it, so the question comes at the terminal and not at
//! the next backup or notification nobody watches. Only keyring entries
//! are read, a repository's password and a notification channel's address,
//! token or password: no other source asks, and a command is never run.
//! What an entry holds is dropped as soon as it is read, and never reported.

use domain::RepositoryAlias;
use domain::execution::refusal::SettingRefusal;
use domain::notifications::{Auth, ChannelName};
use domain::planning::AgentConfig;
use domain::secrets::{KeyringEntry, Redactor, SecretRef};

use crate::ports::SecretResolver;
use crate::secrets::unresolved_refusal;

/// What takes a secret from a keyring entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyringUser {
    /// A repository, its password.
    Repository(RepositoryAlias),
    /// A notification channel, its address, its token or its password.
    Channel(ChannelName),
}

/// One keyring entry a repository or a channel takes a secret from, and
/// whether it could be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyringRead {
    pub user: KeyringUser,
    pub entry: KeyringEntry,
    /// `None` when the entry was read; otherwise why it couldn't be, as a
    /// run of a repository would be refused.
    pub refused: Option<SettingRefusal>,
}

/// What a check of the keyring entries came to: one read for each
/// repository whose password is in the keyring, in the aliases' order, then
/// one for each keyring entry of a channel, in the channels' order, its
/// address first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyringCheck {
    pub reads: Vec<KeyringRead>,
}

impl KeyringCheck {
    /// Whether every entry was read.
    pub fn passed(&self) -> bool {
        self.reads.iter().all(|read| read.refused.is_none())
    }
}

/// Reads, through `secrets`, each keyring entry a repository or a
/// notification channel of `config` takes a secret from, once, however
/// many of them share it.
pub async fn check_keyring_entries(
    config: &AgentConfig,
    secrets: &dyn SecretResolver,
) -> KeyringCheck {
    // Nothing was read, so there is no secret to mask.
    let redactor = Redactor::new([]);
    let repositories = config.repositories().iter().map(|(alias, repository)| {
        (
            KeyringUser::Repository(alias.clone()),
            &repository.settings.password,
        )
    });
    let channels = config
        .notifications()
        .channels()
        .iter()
        .flat_map(|(name, channel)| {
            let auth = channel.auth().map(|auth| match auth {
                Auth::Bearer(token) => token,
                Auth::Basic { password, .. } => password,
            });
            [Some(channel.url()), auth]
                .into_iter()
                .flatten()
                .map(|secret| (KeyringUser::Channel(name.clone()), secret))
        });
    let mut read: Vec<(KeyringEntry, Option<SettingRefusal>)> = Vec::new();
    let mut reads = Vec::new();
    for (user, secret) in repositories.chain(channels) {
        let SecretRef::Keyring(entry) = secret else {
            continue;
        };
        let refused = match read.iter().find(|(seen, _)| seen == entry) {
            Some((_, refused)) => refused.clone(),
            None => {
                // The value is dropped here, unread by anything.
                let refused = secrets
                    .resolve(secret)
                    .await
                    .err()
                    .map(|error| unresolved_refusal(&error, &redactor));
                read.push((entry.clone(), refused.clone()));
                refused
            }
        };
        reads.push(KeyringRead {
            user,
            entry: entry.clone(),
            refused,
        });
    }
    KeyringCheck { reads }
}
