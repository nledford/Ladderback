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
use domain::secrets::KeyringEntry;

use crate::ports::SecretResolver;

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
        todo!()
    }
}

/// Reads, through `secrets`, each keyring entry a repository of `config`
/// takes its password from, once, however many repositories share it.
pub async fn check_keyring_entries(
    config: &AgentConfig,
    secrets: &dyn SecretResolver,
) -> KeyringCheck {
    let _ = (config, secrets);
    todo!()
}
