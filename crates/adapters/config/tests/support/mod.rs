//! Shared by the config adapter's integration tests.

#![allow(dead_code, clippy::result_large_err)]

pub mod model;

use app::fakes::FakeClock;
use app::ports::{ConfigError, PasswordProblem, Position, Problem};
use config_adapter::{Parsed, Schemas, parse};
use domain::planning::ScheduleZone;
use domain::{TimeZoneDatabase, Timestamp};
use std::path::Path;
use std::sync::Arc;

/// The path an error names, in tests that parse text without a file.
pub const FILE: &str = "/home/demo/.config/ladderback/config.toml";

pub fn parsed(text: &str) -> Result<Parsed, ConfigError> {
    parse(Path::new(FILE), text, &Schemas::production())
}

pub fn parsed_with(text: &str, schemas: &Schemas) -> Result<Parsed, ConfigError> {
    parse(Path::new(FILE), text, schemas)
}

/// The schedule zone `name` names in the database built in. Zones are equal
/// by name, so it equals the one read from the system's database.
pub fn zone(name: &str) -> ScheduleZone {
    let found = TimeZoneDatabase::bundled()
        .get(name)
        .expect("a zone of the database");
    ScheduleZone::new(name, found).expect("a zone with a name")
}

/// The fields of a [`ConfigError::Invalid`].
#[derive(Debug)]
pub struct Invalid {
    pub file: String,
    pub key: Option<String>,
    pub at: Option<Position>,
    pub problem: Problem,
    pub detail: String,
}

pub fn invalid<T: std::fmt::Debug>(result: Result<T, ConfigError>) -> Invalid {
    match result {
        Err(ConfigError::Invalid {
            file,
            key,
            at,
            problem,
            detail,
        }) => Invalid {
            file,
            key,
            at,
            problem,
            detail,
        },
        other => panic!("the file should be invalid, got {other:?}"),
    }
}

/// Where the first `needle` in `text` starts, counted from 1 in lines and in
/// characters. The fixtures are ASCII up to the place they are searched for.
pub fn position_of(text: &str, needle: &str) -> Position {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not in the text"));
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |start| start.chars().count())
        + 1;
    Position { line, column }
}

/// What one repository and one plan need, and nothing else.
pub const BASE: &str = "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\n\
     password-env = \"DEMO_PASSWORD\"\n\n[[plans]]\nid = \"home\"\nrepository = \"nas\"\n\
     sources = [\"/home/demo\"]\n";

/// [`BASE`] with `extra` after the plan's own keys: more keys of the plan, or
/// a table of their own.
pub fn base_with(extra: &str) -> String {
    format!("{BASE}{extra}")
}

/// A repository with `password_line` as its password and nothing else.
pub fn repository_with(password_lines: &str) -> String {
    format!("schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\n{password_lines}")
}

pub fn clock_at(timestamp: &str) -> Arc<FakeClock> {
    let start: Timestamp = timestamp.parse().expect("a timestamp");
    Arc::new(FakeClock::new(start))
}

pub fn password_problem(result: Invalid) -> PasswordProblem {
    match result.problem {
        Problem::Password(problem) => problem,
        other => panic!("the problem should be the password's, got {other:?}"),
    }
}

/// A scratch folder with a config file's place in it, and a clock that only
/// moves when told to.
pub struct Scratch {
    pub dir: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub clock: Arc<FakeClock>,
}

impl Scratch {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let path = dir.path().join("config.toml");
        Self {
            dir,
            path,
            clock: clock_at("2026-11-08T13:20:21.123Z"),
        }
    }

    pub fn store(&self) -> config_adapter::FileConfigStore {
        config_adapter::FileConfigStore::new(self.path.clone(), self.clock.clone())
    }

    pub fn store_with(&self, schemas: Schemas) -> config_adapter::FileConfigStore {
        self.store().with_schemas(schemas)
    }

    /// The name the store gives the file in errors.
    pub fn name(&self) -> String {
        self.path.display().to_string()
    }

    pub fn write(&self, text: &str) {
        std::fs::write(&self.path, text).expect("the config file is written");
    }

    pub fn write_bytes(&self, bytes: &[u8]) {
        std::fs::write(&self.path, bytes).expect("the config file is written");
    }

    pub fn read(&self) -> String {
        std::fs::read_to_string(&self.path).expect("the config file is read")
    }

    pub fn read_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.path).expect("the config file is read")
    }

    pub fn history_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("history")
    }

    /// The kept versions, by name, oldest name first.
    pub fn history(&self) -> Vec<(String, Vec<u8>)> {
        let Ok(entries) = std::fs::read_dir(self.history_dir()) else {
            return Vec::new();
        };
        let mut versions: Vec<_> = entries
            .map(|entry| {
                let path = entry.expect("an entry").path();
                let name = path
                    .file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned();
                (name, std::fs::read(&path).expect("a kept version"))
            })
            .collect();
        versions.sort();
        versions
    }

    /// What the scratch folder holds, by name.
    pub fn entries(&self) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(self.dir.path())
            .expect("the scratch folder")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

pub fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}
