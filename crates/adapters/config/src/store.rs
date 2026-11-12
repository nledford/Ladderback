//! `ConfigStore` over the file: load, migrate, write and keep history.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use app::ports::{Clock, ConfigError, ConfigStore, Problem, RecordError};
use async_trait::async_trait;
use domain::planning::{AgentConfig, RepositoryConfig};
use domain::{RepositoryAlias, RepositoryId};

use crate::files::{self, Contents};
use crate::schema::{MAX_CONFIG_BYTES, Schemas, parse, render_as};

/// The agent's configuration in a TOML file, with the earlier versions of the
/// file kept in a `history` folder next to it.
pub struct FileConfigStore {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    schemas: Schemas,
    /// One load or save at a time: a load that migrates must not undo a save
    /// that comes while it works.
    gate: Arc<Mutex<()>>,
}

impl FileConfigStore {
    /// A store over the file at `path`, which needn't exist yet, reading the
    /// versions of the format this build knows. `clock` names the history.
    pub fn new(path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            path: path.into(),
            clock,
            schemas: Schemas::production(),
            gate: Arc::new(Mutex::new(())),
        }
    }

    /// The same store reading other versions of the format.
    #[must_use]
    pub fn with_schemas(mut self, schemas: Schemas) -> Self {
        self.schemas = schemas;
        self
    }

    /// Runs `work` on a thread for blocking calls, behind the gate.
    async fn exclusively<T, F>(&self, work: F) -> Result<T, ConfigError>
    where
        T: Send + 'static,
        F: FnOnce(&Job) -> Result<T, ConfigError> + Send + 'static,
    {
        let job = Job {
            path: self.path.clone(),
            clock: self.clock.clone(),
            schemas: self.schemas.clone(),
        };
        let gate = self.gate.clone();
        tokio::task::spawn_blocking(move || {
            // A panic in an earlier job leaves nothing half-done that this one
            // would trip on: the file is only ever replaced whole.
            let _turn = gate.lock().unwrap_or_else(PoisonError::into_inner);
            work(&job)
        })
        .await
        .unwrap_or_else(|_| {
            Err(ConfigError::Unavailable {
                file: self.path.display().to_string(),
                detail: "the work on the file didn't finish".to_owned(),
            })
        })
    }
}

/// What a load or a save needs, owned so it can move to the blocking thread.
struct Job {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    schemas: Schemas,
}

impl Job {
    fn name(&self) -> String {
        self.path.display().to_string()
    }

    fn unavailable(&self, error: &std::io::Error) -> ConfigError {
        ConfigError::Unavailable {
            file: self.name(),
            detail: error.to_string(),
        }
    }

    fn too_large(&self) -> ConfigError {
        ConfigError::TooLarge {
            file: self.name(),
            limit: MAX_CONFIG_BYTES,
        }
    }

    /// The file as it is: its bytes, or `None` when there is no file.
    fn current_bytes(&self) -> Result<Option<Vec<u8>>, ConfigError> {
        match files::read(&self.path).map_err(|error| self.unavailable(&error))? {
            Contents::Missing => Ok(None),
            Contents::Bytes(bytes) => Ok(Some(bytes)),
            Contents::TooLarge => Err(self.too_large()),
        }
    }

    /// Keeps `old` in the history, then replaces the file with `text`.
    fn replace(&self, old: Option<&[u8]>, text: &str) -> Result<(), ConfigError> {
        if let Some(old) = old {
            files::keep(&self.path, old, self.clock.now())
                .map_err(|error| self.unavailable(&error))?;
        }
        files::write(&self.path, text.as_bytes()).map_err(|error| self.unavailable(&error))
    }

    fn load(&self) -> Result<AgentConfig, ConfigError> {
        let Some(bytes) = self.current_bytes()? else {
            return Ok(AgentConfig::default());
        };
        let text = String::from_utf8(bytes).map_err(|_| ConfigError::Invalid {
            file: self.name(),
            key: None,
            at: None,
            problem: Problem::NotUtf8,
            detail: String::new(),
        })?;
        let parsed = parse(&self.path, &text, &self.schemas)?;
        if parsed.migrated {
            // The file is only replaced once it is known to be valid, and
            // only after the version the user wrote is safe in the history.
            let current = render_as(&self.path, &parsed.config, self.schemas.current)?;
            self.replace(Some(text.as_bytes()), &current)?;
        }
        Ok(parsed.config)
    }

    fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        let text = render_as(&self.path, config, self.schemas.current)?;
        let old = self.current_bytes()?;
        self.replace(old.as_deref(), &text)
    }

    /// Records `id` in the entry `alias` by adding one key to the file as
    /// it is, and writes the file only when the edit reads back as the
    /// configuration with the ID and nothing else changed. `meanwhile` runs
    /// between the edit and the write, where a test changes the file.
    fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
        meanwhile: impl FnOnce(),
    ) -> Result<(), RecordError> {
        let _ = (alias, read, id, meanwhile);
        todo!()
    }
}

#[async_trait]
impl ConfigStore for FileConfigStore {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        self.exclusively(|job| job.load()).await
    }

    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        let config = config.clone();
        self.exclusively(move |job| job.save(&config)).await
    }

    async fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), RecordError> {
        let _ = (alias, read, id);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use app::fakes::FakeClock;
    use app::ports::{ConfigError, NotInPlace, Problem, RecordError};
    use domain::planning::{NotRecorded, RepositoryConfig};
    use domain::{RepositoryAlias, RepositoryId, Timestamp};

    use super::Job;
    use crate::schema::{Schemas, parse};

    const FILE: &str = "schema = 1\n[repositories.nas]\nrepository = \"/srv/demo\"\npassword-env = \"DEMO\"\n";

    fn job_at(path: PathBuf, schemas: Schemas) -> Job {
        Job {
            path,
            clock: Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).expect("a time"),
            )),
            schemas,
        }
    }

    fn nas() -> RepositoryAlias {
        RepositoryAlias::parse("nas").expect("an alias")
    }

    fn opened() -> RepositoryId {
        RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
    }

    /// The entry `nas` as a job made from `FILE` read it.
    fn read() -> RepositoryConfig {
        parse(Path::new("config.toml"), FILE, &Schemas::production())
            .expect("the file parses")
            .config
            .repository(&nas())
            .cloned()
            .expect("the entry")
    }

    fn files_in(folder: &Path) -> usize {
        std::fs::read_dir(folder).expect("the folder").count()
    }

    #[test]
    fn a_file_changed_while_the_id_was_added_is_left_as_it_was_changed() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        std::fs::write(&path, FILE).expect("the file");
        let job = job_at(path.clone(), Schemas::production());
        let changed = FILE.replace("/srv/demo", "/srv/other");
        let recorded = job.record_id(&nas(), &read(), &opened(), || {
            std::fs::write(&path, &changed).expect("the file changes");
        });
        assert_eq!(
            recorded,
            Err(RecordError::NotInPlace(NotInPlace::ChangedMeanwhile))
        );
        assert_eq!(std::fs::read_to_string(&path).expect("the file"), changed);
        assert_eq!(files_in(scratch.path()), 1, "nothing kept in a history");
    }

    #[test]
    fn a_file_gone_since_the_job_read_it_is_not_made_again() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let job = job_at(scratch.path().join("config.toml"), Schemas::production());
        assert_eq!(
            job.record_id(&nas(), &read(), &opened(), || {}),
            Err(RecordError::NotRecorded(NotRecorded::Changed))
        );
        assert_eq!(files_in(scratch.path()), 0, "no file made");
    }

    #[test]
    fn a_file_that_is_not_utf8_is_not_written() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        let mut bytes = FILE.as_bytes().to_vec();
        bytes.extend_from_slice(b"# \xff\n");
        std::fs::write(&path, &bytes).expect("the file");
        let recorded = job_at(path.clone(), Schemas::production()).record_id(
            &nas(),
            &read(),
            &opened(),
            || {},
        );
        assert!(
            matches!(
                recorded,
                Err(RecordError::Store(ConfigError::Invalid {
                    problem: Problem::NotUtf8,
                    ..
                }))
            ),
            "{recorded:?}"
        );
        assert_eq!(std::fs::read(&path).expect("the file"), bytes);
        assert_eq!(files_in(scratch.path()), 1, "nothing kept in a history");
    }

    /// A file in a version older than the build writes is the load's to
    /// migrate as a whole: a key added in place would be read in the old one.
    #[test]
    fn a_file_in_an_older_version_is_not_edited() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let path = scratch.path().join("config.toml");
        std::fs::write(&path, FILE).expect("the file");
        let newer = Schemas {
            current: 2,
            steps: vec![|_| Ok(())],
        };
        assert_eq!(
            job_at(path.clone(), newer).record_id(&nas(), &read(), &opened(), || {}),
            Err(RecordError::NotInPlace(NotInPlace::Form))
        );
        assert_eq!(std::fs::read_to_string(&path).expect("the file"), FILE);
        assert_eq!(files_in(scratch.path()), 1, "nothing kept in a history");
    }
}
