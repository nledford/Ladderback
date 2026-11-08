use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use async_trait::async_trait;
use domain::secrets::{CommandLine, KeyringEntry, SecretError, SecretRef};

use crate::ports::{Resolved, SecretResolver};

/// What the fake does when told to run a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Script {
    /// Runs to its end with this exit status and output.
    Exits {
        status: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// Never ends on its own: the resolver's time limit does.
    Hangs,
}

/// A secret store in memory: an environment, files with permissions, a
/// keyring and scripted commands, read through the same rules as the real
/// resolver (`domain::secrets::{decode, file_warning}`). It remembers what it
/// was asked, so a scenario can tell how often a secret was read.
#[derive(Default)]
pub struct FakeSecrets {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    env: BTreeMap<String, String>,
    files: BTreeMap<String, File>,
    directories: BTreeSet<String>,
    keyring: BTreeMap<String, String>,
    commands: BTreeMap<(String, Vec<String>), Script>,
    reads: Vec<SecretRef>,
}

struct File {
    contents: Vec<u8>,
    /// Unix permission bits.
    mode: u32,
}

impl FakeSecrets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_env(&self, _name: &str, _value: &str) {
        todo!("set an environment variable")
    }

    pub fn write_file(&self, _path: &str, _contents: &[u8], _mode: u32) {
        todo!("write a file with permission bits")
    }

    /// A directory at `path`: reading it as a file fails.
    pub fn make_directory(&self, _path: &str) {
        todo!("make a directory")
    }

    pub fn set_keyring_entry(&self, _entry: &KeyringEntry, _secret: &str) {
        todo!("keep a secret in the keyring")
    }

    /// What running `command` does. A command with no script can't be started.
    pub fn script(&self, _command: &CommandLine, _script: Script) {
        todo!("script what a command does")
    }

    /// Every secret the resolver was asked for, in order.
    pub fn reads(&self) -> Vec<SecretRef> {
        todo!("what was asked for")
    }

    /// How many times `secret` was asked for.
    pub fn read_count(&self, _secret: &SecretRef) -> usize {
        todo!("how often it was asked for")
    }
}

#[async_trait]
impl SecretResolver for FakeSecrets {
    async fn resolve(&self, _secret: &SecretRef) -> Result<Resolved, SecretError> {
        todo!("read the secret as the real resolver does")
    }
}
