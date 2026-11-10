//! Folders for a daemon a test starts. `ladderback serve` keeps its operation
//! log and agent ID in a data folder and reads its `config.toml` from a config
//! folder; without an override it uses the platform's, the ones a developer's
//! own daemon uses. A test that started it that way would share that
//! operation log, its migrations and its schema version with every other
//! daemon on the machine. Every test that starts the daemon gives it folders
//! of its own through [`isolate`]; `tests/isolation.rs` fails when one doesn't.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The variable that names the daemon's config folder.
pub const CONFIG_DIR_ENV: &str = "LADDERBACK_CONFIG_DIR";

/// The variable that names the daemon's data folder.
pub const DATA_DIR_ENV: &str = "LADDERBACK_DATA_DIR";

/// The config folder [`isolate`] gives a daemon whose scratch folder is `root`.
pub fn config_dir(root: &Path) -> PathBuf {
    root.join("config")
}

/// The data folder [`isolate`] gives a daemon whose scratch folder is `root`.
pub fn data_dir(root: &Path) -> PathBuf {
    root.join("data")
}

/// Points the daemon that `command` starts at the config and data folders
/// under `root`, a scratch folder of that daemon's own (the daemon creates the
/// data folder; the config folder may stay absent).
pub fn isolate<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
    command
        .env(CONFIG_DIR_ENV, config_dir(root))
        .env(DATA_DIR_ENV, data_dir(root))
}
