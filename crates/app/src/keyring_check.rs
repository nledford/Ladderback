//! Reading each keyring entry the configuration names, once, while someone
//! is there to answer the keyring (#892, technical design §10.2). On macOS
//! the keychain asks once for each new build of the program before it lets
//! that build read an entry; `ladderback check-keyring` reads them with the
//! program that runs it, so the question comes at the terminal and not at
//! the next backup nobody watches. Only `password-keyring` entries are
//! read: no other source asks, and a `password-command` is never run. What
//! an entry holds is dropped as soon as it is read, and never reported.

use domain::RepositoryAlias;
use domain::execution::refusal::SettingRefusal;
use domain::planning::AgentConfig;
use domain::secrets::{KeyringEntry, Redactor, SecretRef};

use crate::ports::SecretResolver;
use crate::secrets::unresolved_refusal;

/// One repository whose password is in the keyring, and whether its entry
/// could be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyringRead {
    pub repository: RepositoryAlias,
    pub entry: KeyringEntry,
    /// `None` when the entry was read; otherwise why it couldn't be, as a
    /// run of the repository would be refused.
    pub refused: Option<SettingRefusal>,
}

/// What a check of the keyring entries came to: one read for each
/// repository whose password is in the keyring, in the aliases' order.
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

/// Reads, through `secrets`, each keyring entry a repository of `config`
/// takes its password from, once, however many repositories share it.
pub async fn check_keyring_entries(
    config: &AgentConfig,
    secrets: &dyn SecretResolver,
) -> KeyringCheck {
    // Nothing was read, so there is no secret to mask.
    let redactor = Redactor::new([]);
    let mut read: Vec<(KeyringEntry, Option<SettingRefusal>)> = Vec::new();
    let mut reads = Vec::new();
    for (alias, repository) in config.repositories() {
        let SecretRef::Keyring(entry) = &repository.settings.password else {
            continue;
        };
        let refused = match read.iter().find(|(seen, _)| seen == entry) {
            Some((_, refused)) => refused.clone(),
            None => {
                // The value is dropped here, unread by anything.
                let refused = secrets
                    .resolve(&repository.settings.password)
                    .await
                    .err()
                    .map(|error| unresolved_refusal(&error, &redactor));
                read.push((entry.clone(), refused.clone()));
                refused
            }
        };
        reads.push(KeyringRead {
            repository: alias.clone(),
            entry: entry.clone(),
            refused,
        });
    }
    KeyringCheck { reads }
}
