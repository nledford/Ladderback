//! The platform's folders for Ladderback's own files (technical design §4.2):
//! where `config.toml` lives, the data dir with the operation log and the
//! agent ID, and the cache dir that holds the workers' rustic caches. Each
//! answers to an environment override first, so a daemon (or a test) can run
//! against a folder of its own.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

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

/// Where the workers keep rustic's caches, one folder per repository and
/// worker (§4.1): `LADDERBACK_CACHE_DIR` when set to a non-empty value, else
/// the platform's cache folder, or `None` when there is no home directory.
/// Nothing in it is needed: a lost cache is read again from the repository.
pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_from(|var| std::env::var_os(var))
}

/// [`cache_dir`] with the environment supplied, for wiring and tests.
pub fn cache_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env_dir(&env, "LADDERBACK_CACHE_DIR") {
        return Some(dir);
    }
    platform_dir(&env, Folder::Cache)
}

/// Makes `dir`, and every folder missing above it, readable and writable by
/// its owner only (mode 0700, as the XDG base directory specification asks of
/// the folders it makes). A folder that is already there is left as it is.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Whether files can be made in `dir`: makes a file of its own there and
/// removes it again. A folder that is another user's, read-only or on a
/// read-only disk fails with the OS's error.
pub fn probe_writable(dir: &Path) -> io::Result<()> {
    let probe = dir.join(format!(".ladderback-probe-{}", std::process::id()));
    std::fs::File::create(&probe)?;
    std::fs::remove_file(&probe)
}

/// Where the stock `rustic` CLI keeps its profiles (§6.2), the folder
/// `use-profiles` looks in first: the platform's configuration folder for an
/// application called `rustic`, or `None` when there is no home directory.
pub fn rustic_config_dir() -> Option<PathBuf> {
    rustic_config_dir_from(|var| std::env::var_os(var))
}

/// [`rustic_config_dir`] with the environment supplied, for wiring and tests.
/// The `LADDERBACK_*` overrides do not apply: this is rustic's folder, not
/// Ladderback's.
pub fn rustic_config_dir_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    rustic_dir(&env)
}

#[cfg(target_os = "macos")]
fn rustic_dir(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    home(env).map(|home| home.join("Library/Application Support/rustic"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn rustic_dir(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env_dir(env, "XDG_CONFIG_HOME") {
        return Some(dir.join("rustic"));
    }
    home(env).map(|home| home.join(".config/rustic"))
}

#[cfg(not(unix))]
fn rustic_dir(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let _ = env;
    None
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
    Cache,
}

#[cfg(target_os = "macos")]
fn platform_dir(env: &impl Fn(&str) -> Option<OsString>, folder: Folder) -> Option<PathBuf> {
    // Config and data live in the application-support folder, the Mac's own
    // place for an application's documents; the cache in the user's caches
    // folder, which backups and migrations leave out.
    let under = match folder {
        Folder::Config | Folder::Data => "Library/Application Support/ladderback",
        Folder::Cache => "Library/Caches/ladderback",
    };
    home(env).map(|home| home.join(under))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_dir(env: &impl Fn(&str) -> Option<OsString>, folder: Folder) -> Option<PathBuf> {
    // The XDG base directories: `config` for the configuration, `data` for
    // the agent's own files, `cache` for the workers' rustic caches.
    let (var, fallback) = match folder {
        Folder::Config => ("XDG_CONFIG_HOME", ".config"),
        Folder::Data => ("XDG_DATA_HOME", ".local/share"),
        Folder::Cache => ("XDG_CACHE_HOME", ".cache"),
    };
    // The specification: a relative path in these variables is invalid and
    // ignored. Config and data take one as it is (an older choice, kept
    // here); the cache folder follows the specification.
    let set = env_dir(env, var).filter(|dir| !matches!(folder, Folder::Cache) || dir.is_absolute());
    if let Some(dir) = set {
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

#[cfg(all(test, target_os = "macos"))]
mod rustic_macos {
    use super::*;

    #[test]
    fn macos_keeps_rustics_profiles_in_application_support() {
        let from = |name: &str| (name == "HOME").then(|| OsString::from("/Users/demo"));
        assert_eq!(
            rustic_config_dir_from(from),
            Some(PathBuf::from(
                "/Users/demo/Library/Application Support/rustic"
            ))
        );
    }

    #[test]
    fn xdg_config_home_does_not_move_it_on_macos() {
        let from = |name: &str| match name {
            "HOME" => Some(OsString::from("/Users/demo")),
            "XDG_CONFIG_HOME" => Some(OsString::from("/elsewhere")),
            _ => None,
        };
        assert_eq!(
            rustic_config_dir_from(from),
            Some(PathBuf::from(
                "/Users/demo/Library/Application Support/rustic"
            ))
        );
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod rustic_xdg {
    use super::*;

    #[test]
    fn xdg_config_home_wins_over_home() {
        let from = |name: &str| match name {
            "HOME" => Some(OsString::from("/home/demo")),
            "XDG_CONFIG_HOME" => Some(OsString::from("/srv/config")),
            _ => None,
        };
        assert_eq!(
            rustic_config_dir_from(from),
            Some(PathBuf::from("/srv/config/rustic"))
        );
    }

    #[test]
    fn without_xdg_config_home_it_is_under_home() {
        let from = |name: &str| match name {
            "HOME" => Some(OsString::from("/home/demo")),
            "XDG_CONFIG_HOME" => Some(OsString::new()),
            _ => None,
        };
        assert_eq!(
            rustic_config_dir_from(from),
            Some(PathBuf::from("/home/demo/.config/rustic"))
        );
    }
}

#[cfg(all(test, unix))]
mod rustic_any_unix {
    use super::*;

    #[test]
    fn without_a_home_there_is_no_folder() {
        assert_eq!(rustic_config_dir_from(|_| None), None);
        let empty = |_: &str| Some(OsString::new());
        assert_eq!(rustic_config_dir_from(empty), None);
    }

    #[test]
    fn ladderbacks_own_override_does_not_move_rustics_folder() {
        let from = |name: &str| match name {
            "HOME" => Some(OsString::from("/home/demo")),
            "LADDERBACK_CONFIG_DIR" => Some(OsString::from("/srv/lb")),
            _ => None,
        };
        let found = rustic_config_dir_from(from);
        assert!(found.is_some_and(|dir| !dir.starts_with("/srv/lb")));
    }
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

#[cfg(test)]
mod cache {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn its_override_names_the_folder_itself() {
        let from = env(&[
            ("HOME", "/home/demo"),
            ("XDG_CACHE_HOME", "/srv/cache"),
            ("LADDERBACK_CACHE_DIR", "/srv/lb-cache"),
        ]);
        assert_eq!(cache_dir_from(from), Some(PathBuf::from("/srv/lb-cache")));
    }

    #[test]
    fn an_empty_override_is_as_good_as_none() {
        let from = env(&[("LADDERBACK_CACHE_DIR", ""), ("HOME", "/home/demo")]);
        let found = cache_dir_from(from).expect("the platform's folder");
        assert!(found.starts_with("/home/demo"), "{}", found.display());
    }

    #[test]
    fn without_a_home_or_an_override_there_is_no_folder() {
        assert_eq!(cache_dir_from(env(&[])), None);
        assert_eq!(cache_dir_from(env(&[("HOME", "")])), None);
    }

    #[test]
    fn the_other_overrides_do_not_move_it() {
        let from = env(&[
            ("HOME", "/home/demo"),
            ("LADDERBACK_CONFIG_DIR", "/srv/lb"),
            ("LADDERBACK_DATA_DIR", "/srv/lb"),
        ]);
        let found = cache_dir_from(from).expect("the platform's folder");
        assert!(!found.starts_with("/srv/lb"), "{}", found.display());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keeps_it_in_the_users_caches_folder() {
        let from = env(&[("HOME", "/Users/demo"), ("XDG_CACHE_HOME", "/elsewhere")]);
        assert_eq!(
            cache_dir_from(from),
            Some(PathBuf::from("/Users/demo/Library/Caches/ladderback"))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn xdg_cache_home_wins_over_home() {
        let from = env(&[("HOME", "/home/demo"), ("XDG_CACHE_HOME", "/srv/cache")]);
        assert_eq!(
            cache_dir_from(from),
            Some(PathBuf::from("/srv/cache/ladderback"))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn without_xdg_cache_home_it_is_under_home() {
        let from = env(&[("HOME", "/home/demo"), ("XDG_CACHE_HOME", "")]);
        assert_eq!(
            cache_dir_from(from),
            Some(PathBuf::from("/home/demo/.cache/ladderback"))
        );
    }

    /// The XDG base directory specification: "If an implementation encounters
    /// a relative path in any of these variables it should consider the path
    /// invalid and ignore it."
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_relative_xdg_cache_home_is_ignored() {
        let from = env(&[("HOME", "/home/demo"), ("XDG_CACHE_HOME", "cache")]);
        assert_eq!(
            cache_dir_from(from),
            Some(PathBuf::from("/home/demo/.cache/ladderback"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_folders_it_makes_are_its_owners_alone() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("cache");
        let dir = parent.join("ladderback");
        create_private_dir(&dir).unwrap();
        for made in [&parent, &dir] {
            let mode = std::fs::metadata(made).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{} is {mode:o}", made.display());
        }
        // Making it again is no error.
        create_private_dir(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_already_there_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ladderback");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).unwrap();
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);
    }

    #[test]
    fn a_folder_where_a_file_is_cannot_be_made() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        std::fs::write(&file, b"").unwrap();
        assert!(create_private_dir(&file.join("ladderback")).is_err());
    }

    #[test]
    fn a_writable_folder_passes_the_probe_and_is_left_as_it_was() {
        let root = tempfile::tempdir().unwrap();
        probe_writable(root.path()).unwrap();
        let left: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
        assert!(left.is_empty(), "the probe left {left:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_written_fails_the_probe() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ladderback");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        if std::fs::File::create(dir.join("probe")).is_ok() {
            testkit::skip!("running as root: permissions don't stop writing to a folder");
        }
        let refused = probe_writable(&dir).expect_err("a read-only folder");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
    }
}
