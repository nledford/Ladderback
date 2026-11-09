//! The versions of the file format, and the two pure functions that read a
//! file's text into a configuration and write one back.

use std::path::Path;

use app::ports::{ConfigError, Problem};
use domain::planning::AgentConfig;
use serde::Deserialize;
use toml::de::{DeTable, DeValue, Deserializer};

use crate::locate::{Doc, Path as KeyPath, path_of, position, scrub, value_span};
use crate::read::{Reader, Spot, config};
use crate::tables::FileTable;
use crate::write;

/// The most a configuration file may hold. It is far more than any
/// configuration needs: a file over it is refused unread, so a file that never
/// ends can't fill the memory.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Whether a file of `len` bytes is over [`MAX_CONFIG_BYTES`]: one at the cap
/// is not.
pub(crate) fn over_cap(len: usize) -> bool {
    u64::try_from(len).is_ok_and(|len| len > MAX_CONFIG_BYTES)
}

/// How many earlier versions of the file are kept in its history.
pub const KEPT_VERSIONS: usize = 10;

/// The version of the format this build writes, and the newest it reads.
pub const CURRENT_SCHEMA: u64 = 1;

/// Why a migration step gave up. `detail` is for a person, and never holds a
/// value from the file: a file may hold a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationError {
    pub detail: String,
}

/// One migration: it turns the table of a file in version `n` into the table
/// of version `n + 1`, and does nothing else.
pub type Step = fn(&mut toml::Table) -> Result<(), MigrationError>;

/// The versions of the format a build reads: `current`, and a `Step` for each
/// older one it can still migrate from, oldest first. The oldest version
/// read is `current` less the number of steps.
#[derive(Clone, Debug)]
pub struct Schemas {
    pub current: u64,
    pub steps: Vec<Step>,
}

impl Schemas {
    /// The versions this build knows: only the first one, so no steps yet.
    pub fn production() -> Self {
        Self {
            current: CURRENT_SCHEMA,
            steps: Vec::new(),
        }
    }

    pub fn oldest(&self) -> u64 {
        self.current
            .saturating_sub(u64::try_from(self.steps.len()).unwrap_or(u64::MAX))
    }
}

/// What a file held.
#[derive(Debug)]
pub struct Parsed {
    pub config: AgentConfig,
    /// Whether the file was in an older version and was migrated in memory.
    pub migrated: bool,
}

/// An error that isn't about a setting: the file as a whole.
fn invalid(file: &str, problem: Problem, detail: String) -> ConfigError {
    ConfigError::Invalid {
        file: file.to_owned(),
        key: None,
        at: None,
        problem,
        detail,
    }
}

/// A file that isn't TOML, where the parser stopped.
fn syntax(file: &str, text: &str, error: &toml::de::Error) -> ConfigError {
    ConfigError::Invalid {
        file: file.to_owned(),
        key: None,
        at: error.span().map(|span| position(text, span.start)),
        problem: Problem::Syntax,
        detail: scrub(error.message()),
    }
}

/// Reads the text of the file at `file`, which is only named in errors.
///
/// The order is: the syntax; the `schema` key alone, before anything else is
/// looked at, so that a file from a newer build is refused for that and not
/// for what it holds; the migrations the file's version still needs, on the
/// table as it is; then the strict tables and the rules of the domain.
pub fn parse(file: &Path, text: &str, schemas: &Schemas) -> Result<Parsed, ConfigError> {
    let name = file.display().to_string();
    let doc = DeTable::parse(text).map_err(|error| syntax(&name, text, &error))?;
    let version = probe(&name, text, &doc, schemas)?;
    if version == schemas.current {
        return read(&name, text, &doc, true).map(|config| Parsed {
            config,
            migrated: false,
        });
    }
    let migrated = migrate(&name, text, version, schemas)?;
    let doc = DeTable::parse(&migrated)
        .map_err(|error| invalid(&name, Problem::Migration, scrub(error.message())))?;
    read(&name, &migrated, &doc, false).map(|config| Parsed {
        config,
        migrated: true,
    })
}

/// The version the file says it is in, if this build reads it.
fn probe(file: &str, text: &str, doc: &Doc<'_>, schemas: &Schemas) -> Result<u64, ConfigError> {
    let path = KeyPath::default().key("schema");
    let Some(value) = doc
        .get_ref()
        .iter()
        .find(|(key, _)| key.get_ref().as_ref() == "schema")
        .map(|(_, value)| value)
    else {
        return Err(ConfigError::SchemaMissing {
            file: file.to_owned(),
        });
    };
    let at = value_span(doc, &path).map(|span| position(text, span.start));
    let refuse = |problem| ConfigError::Invalid {
        file: file.to_owned(),
        key: path.text(),
        at,
        problem,
        detail: String::new(),
    };
    let found = match value.get_ref() {
        DeValue::Integer(integer) => i64::from_str_radix(integer.as_str(), integer.radix())
            .ok()
            .and_then(|found| u64::try_from(found).ok()),
        _ => None,
    }
    .ok_or_else(|| refuse(Problem::SchemaNotAVersion))?;
    if found > schemas.current {
        return Err(ConfigError::SchemaNewer {
            file: file.to_owned(),
            found,
            supported: schemas.current,
        });
    }
    let oldest = schemas.oldest();
    if found < oldest {
        return Err(refuse(Problem::SchemaTooOld { found, oldest }));
    }
    Ok(found)
}

/// The text of the file after the steps from `version` on, as TOML: it isn't
/// the user's text, so errors in it name a key and no line.
fn migrate(file: &str, text: &str, version: u64, schemas: &Schemas) -> Result<String, ConfigError> {
    let broken = |detail: String| invalid(file, Problem::Migration, detail);
    let mut table: toml::Table = text
        .parse()
        .map_err(|error: toml::de::Error| broken(scrub(error.message())))?;
    let first = usize::try_from(version - schemas.oldest()).unwrap_or(usize::MAX);
    for step in schemas.steps.iter().skip(first) {
        step(&mut table).map_err(|error| broken(error.detail))?;
    }
    let schema = i64::try_from(schemas.current).unwrap_or(i64::MAX);
    table.insert("schema".to_owned(), toml::Value::Integer(schema));
    toml::to_string(&table).map_err(|error| broken(scrub(&error.to_string())))
}

/// The strict tables of a file in the current version, and then the domain.
fn read(file: &str, text: &str, doc: &Doc<'_>, located: bool) -> Result<AgentConfig, ConfigError> {
    let reader = Reader {
        file,
        text,
        doc,
        located,
    };
    let table = FileTable::deserialize(Deserializer::from(doc.clone()))
        .map_err(|error| strict(&reader, &error))?;
    config(&reader, table)
}

/// A table that doesn't have the shape of the format: a key it doesn't have,
/// or a value of another type than the key takes.
fn strict(reader: &Reader<'_, '_>, error: &toml::de::Error) -> ConfigError {
    let message = error.message();
    let found = error.span().and_then(|span| path_of(reader.doc, &span));
    let (path, spot) = found.map_or((KeyPath::default(), Spot::Value), |(path, is_key)| {
        (path, if is_key { Spot::Key } else { Spot::Value })
    });
    if message.starts_with("unknown field") {
        reader.error(&path, Spot::Key, Problem::UnknownKey, String::new())
    } else {
        reader.error(&path, spot, Problem::WrongType, scrub(message))
    }
}

/// Writes `config` in the current version of the format: whatever is the
/// default is left out, so the text holds what the user chose.
pub fn render(file: &Path, config: &AgentConfig) -> Result<String, ConfigError> {
    render_as(file, config, CURRENT_SCHEMA)
}

/// [`render`] with the version the store says is the current one, which is
/// another than [`CURRENT_SCHEMA`] only for a store that reads other versions
/// than production's.
pub(crate) fn render_as(
    file: &Path,
    config: &AgentConfig,
    schema: u64,
) -> Result<String, ConfigError> {
    let name = file.display().to_string();
    let schema = i64::try_from(schema).unwrap_or(i64::MAX);
    let tables = write::file(config, schema).map_err(|key| write::unwritable(&name, key))?;
    let text =
        write::text(&tables).map_err(|error| invalid(&name, Problem::Unwritable, scrub(&error)))?;
    if over_cap(text.len()) {
        return Err(ConfigError::TooLarge {
            file: name,
            limit: MAX_CONFIG_BYTES,
        });
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_over_the_cap_from_one_byte_past_it() {
        let cap = usize::try_from(MAX_CONFIG_BYTES).expect("a size");
        assert!(!over_cap(0));
        assert!(!over_cap(cap - 1));
        assert!(!over_cap(cap));
        assert!(over_cap(cap + 1));
    }
}
