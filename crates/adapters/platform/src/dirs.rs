//! The platform's folders for Ladderback's own files (technical design §4.2):
//! where `config.toml` lives, and the data dir with the operation log and the
//! agent ID. Both answer to an environment override first, so a daemon (or a
//! test) can run against a folder of its own.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::ffi::OsString;
use std::path::PathBuf;

/// Where the agent's `config.toml` lives (§6.1): `LADDERBACK_CONFIG_DIR` when
/// set to a non-empty value, else the platform's configuration folder, or
/// `None` when there is no home directory to place one under.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(|var| std::env::var_os(var))
}

/// [`config_dir`] with the environment supplied, for wiring and tests.
pub fn config_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env_dir(&env, "LADDERBACK_CONFIG_DIR") {
        return Some(dir);
    }
    platform_dir(&env, Folder::Config)
}

/// Where the agent keeps its own files: the operation log (`ladderback.db`)
/// and the agent ID. `LADDERBACK_DATA_DIR` when set to a non-empty value, else
/// the platform's data folder, or `None` when there is no home directory.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_from(|var| std::env::var_os(var))
}

/// [`data_dir`] with the environment supplied, for wiring and tests.
pub fn data_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env_dir(&env, "LADDERBACK_DATA_DIR") {
        return Some(dir);
    }
    platform_dir(&env, Folder::Data)
}

/// A non-empty value of `var` as a path; an empty one is as good as unset.
fn env_dir(env: &impl Fn(&str) -> Option<OsString>, var: &str) -> Option<PathBuf> {
    env(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[derive(Clone, Copy)]
enum Folder {
    Config,
    Data,
}

#[cfg(target_os = "macos")]
fn platform_dir(env: &impl Fn(&str) -> Option<OsString>, folder: Folder) -> Option<PathBuf> {
    // Both live in the application-support folder, the Mac's own place for
    // an application's documents.
    let _ = folder;
    home(env).map(|home| home.join("Library/Application Support/ladderback"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_dir(env: &impl Fn(&str) -> Option<OsString>, folder: Folder) -> Option<PathBuf> {
    // The XDG base directories: `config` for the configuration, `data` for
    // the agent's own files.
    let (var, fallback) = match folder {
        Folder::Config => ("XDG_CONFIG_HOME", ".config"),
        Folder::Data => ("XDG_DATA_HOME", ".local/share"),
    };
    if let Some(dir) = env_dir(env, var) {
        return Some(dir.join("ladderback"));
    }
    home(env).map(|home| home.join(fallback).join("ladderback"))
}

#[cfg(not(unix))]
fn platform_dir(env: &impl Fn(&str) -> Option<OsString>, folder: Folder) -> Option<PathBuf> {
    // Ladderback's daemon runs on macOS and Linux (technical design §1); on
    // any other platform only the environment overrides say where to go.
    let _ = (env, folder);
    None
}

fn home(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    env_dir(env, "HOME")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let vars: BTreeMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).into()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn an_override_names_its_own_folder() {
        let from = env(&[
            ("LADDERBACK_CONFIG_DIR", "/home/demo/test-config"),
            ("LADDERBACK_DATA_DIR", "/home/demo/test-data"),
        ]);
        assert_eq!(
            config_dir_from(&from),
            Some(PathBuf::from("/home/demo/test-config"))
        );
        assert_eq!(
            data_dir_from(&from),
            Some(PathBuf::from("/home/demo/test-data"))
        );
    }

    #[test]
    fn an_empty_override_or_home_is_as_good_as_none() {
        let from = env(&[("LADDERBACK_CONFIG_DIR", ""), ("HOME", "")]);
        assert_eq!(config_dir_from(&from), None, "neither is a folder");
        let from = env(&[("LADDERBACK_DATA_DIR", "")]);
        assert_eq!(data_dir_from(&from), None);
    }

    #[test]
    fn without_a_home_there_is_no_folder() {
        let from = env(&[]);
        assert_eq!(config_dir_from(&from), None);
        assert_eq!(data_dir_from(&from), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_places_both_in_application_support() {
        let from = env(&[("HOME", "/Users/demo")]);
        let expected = Some(PathBuf::from(
            "/Users/demo/Library/Application Support/ladderback",
        ));
        assert_eq!(config_dir_from(&from), expected);
        assert_eq!(data_dir_from(&from), expected);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn xdg_places_both_folders() {
        let from = env(&[("HOME", "/home/demo")]);
        assert_eq!(
            config_dir_from(&from),
            Some(PathBuf::from("/home/demo/.config/ladderback"))
        );
        assert_eq!(
            data_dir_from(&from),
            Some(PathBuf::from("/home/demo/.local/share/ladderback"))
        );
    }
}
