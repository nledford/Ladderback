//! Opening a repository for a job.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::RepositoryId;
use rustic_backend::util::location_to_type_and_path;
use rustic_backend::{BackendOptions, SupportedBackend};
use rustic_core::{Credentials, OpenStatus, Repository, RepositoryOptions};

use super::Context;
use super::cancel::refusing_snapshots_once_cancelled;
use super::convert::JobError;
use super::progress::EventProgressBars;
use super::tmp_files::remove_stale_files;
use crate::job::{Level, RepositorySpec};

/// Builds the repository handle, both halves of it when it has a hot one:
/// the backends, the cache this worker may use, and the progress reporting
/// through `Context::events`. Opening it — or initialising it — is the
/// caller's step.
pub(crate) fn new_repository(
    ctx: &Context,
    spec: &RepositorySpec,
) -> Result<Repository<()>, JobError> {
    // rustic gives the cold half `options` and `options_cold`, the hot half
    // `options` and `options_hot`.
    let backend_options = BackendOptions::default()
        .repository(spec.repository.clone())
        .options(spec.options.clone())
        .options_hot(spec.options_hot.clone())
        .options_cold(spec.options_cold.clone());
    let backends = match &spec.repo_hot {
        Some(hot) => backend_options.repo_hot(hot.clone()),
        None => backend_options,
    }
    .to_backends()?;
    let backends = refusing_snapshots_once_cancelled(&backends, ctx.cancel());
    let options = RepositoryOptions::default();
    let options = match cache_dir(ctx)? {
        Some(dir) => options.cache_dir(dir),
        None => options.no_cache(true),
    };
    let repository = Repository::new_with_progress(
        &options,
        &backends,
        EventProgressBars::new(ctx.events.clone()),
    )?;
    Ok(repository)
}

/// Opens the repository: [`new_repository`] and the password, which is what
/// tells a repository from a folder of files. After it opens, each half
/// that is a local folder is cleaned up after killed workers.
pub(crate) fn open(
    ctx: &Context,
    spec: &RepositorySpec,
) -> Result<Repository<OpenStatus>, JobError> {
    let repository =
        new_repository(ctx, spec)?.open(&Credentials::password(spec.password.expose()))?;
    // Only a repository that opened is known to be one, so only then is what
    // looks like its leftovers ours to remove.
    for half in std::iter::once(spec.repository.as_str()).chain(spec.repo_hot.as_deref()) {
        remove_leftovers(ctx, half)?;
    }
    Ok(repository)
}

/// The cache directory the job may use: the one it was given, if no other
/// worker holds it. Two workers must not share one (§4.1), so a held
/// directory means running without a cache.
fn cache_dir(ctx: &Context) -> io::Result<Option<PathBuf>> {
    let Some(dir) = &ctx.cache_dir else {
        return Ok(None);
    };
    match ctx.lock_cache(dir) {
        Ok(()) => Ok(Some(dir.clone())),
        Err(error) => {
            let why = if error.kind() == io::ErrorKind::WouldBlock {
                "another worker is using it".to_owned()
            } else {
                error.to_string()
            };
            ctx.log(
                Level::Warn,
                format!(
                    "running without a cache: the cache folder {} can't be used: {why}",
                    dir.display()
                ),
            )?;
            Ok(None)
        }
    }
}

/// The repository's ID as Ladderback knows it: 64 lowercase hex characters
/// (`Id`'s `Display` is the first 8).
pub(crate) fn repository_id(repository: &Repository<OpenStatus>) -> Result<RepositoryId, JobError> {
    RepositoryId::parse(repository.config().id.to_hex().as_str())
        .map_err(|_| JobError::failed("the repository's ID is not 64 hexadecimal characters"))
}

/// A killed worker leaves `<id>-tmp-*` files in a repository folder, the hot
/// half's as well as the cold half's (§4.1). `half` is the repository string
/// of one of them; one that is not a local folder has nothing to clean.
/// Removing the stale ones is a courtesy, so failing to is only logged.
fn remove_leftovers(ctx: &Context, half: &str) -> io::Result<()> {
    let Ok((SupportedBackend::Local, location)) = location_to_type_and_path(half) else {
        return Ok(());
    };
    let cleanup = remove_stale_files(Path::new(location.as_str()), SystemTime::now());
    for path in &cleanup.removed {
        ctx.log(
            Level::Info,
            format!("removed a leftover temporary file: {}", path.display()),
        )?;
    }
    for (path, error) in &cleanup.failed {
        ctx.log(
            Level::Warn,
            format!(
                "could not remove a leftover temporary file {}: {error}",
                path.display()
            ),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use domain::secrets::SecretValue;
    use domain::{OperationId, Uuid};
    use rustic_core::{ConfigOptions, KeyOptions};

    use super::super::Process;
    use super::super::events::testing::{Capture, breaking_after, capture};
    use super::*;
    use crate::cache_lock::CacheLock;
    use crate::job::{JobKind, JobSpec, WorkerEvent};

    const PASSWORD: &str = "correct horse battery staple";

    fn context_with(cache_dir: Option<PathBuf>, events: super::super::Events) -> Context {
        let spec = JobSpec {
            op_id: OperationId::from_uuid(Uuid::from_u128(1)),
            cache_dir,
            watchdog_interval_ms: 10,
            job: JobKind::Identify(spec_at("/unused")),
        };
        Context::new(events, &spec, 1, Process::real())
    }

    fn context(cache_dir: Option<PathBuf>) -> (Context, Capture) {
        let (events, capture) = capture();
        (context_with(cache_dir, events), capture)
    }

    fn spec_at(repository: &str) -> RepositorySpec {
        RepositorySpec {
            repository: repository.to_owned(),
            repo_hot: None,
            password: SecretValue::new(PASSWORD),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
            secrets: Vec::new(),
        }
    }

    /// A repository made in `dir` with rustic itself.
    fn init(dir: &Path) {
        let backends = BackendOptions::default()
            .repository(dir.to_string_lossy().into_owned())
            .to_backends()
            .unwrap();
        Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
            .unwrap()
            .init(
                &Credentials::password(PASSWORD),
                &KeyOptions::default(),
                &ConfigOptions::default(),
            )
            .unwrap();
    }

    /// The lines the worker logged, as (level, text).
    fn logged(capture: &Capture) -> Vec<(Level, String)> {
        capture
            .parsed()
            .into_iter()
            .filter_map(|event| match event {
                WorkerEvent::Log { level, message } => Some((level, message)),
                _ => None,
            })
            .collect()
    }

    /// A temporary file of the local backend that is two days old.
    fn leftover(repository: &Path) -> PathBuf {
        let path = repository
            .join("data")
            .join("ab")
            .join(format!("{}-tmp-1", "ab".repeat(32)));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 3600))
            .unwrap();
        path
    }

    /// Makes a folder writable again when dropped, so a failing test doesn't
    /// leave one the scratch folder can't remove.
    struct Writable(PathBuf);

    impl Drop for Writable {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    // The cache folder.

    #[test]
    fn a_job_with_no_cache_configured_runs_without_one_and_says_nothing() {
        let (ctx, capture) = context(None);
        assert_eq!(cache_dir(&ctx).unwrap(), None);
        assert_eq!(logged(&capture), []);
    }

    #[test]
    fn a_cache_folder_nobody_holds_is_made_used_and_held_until_the_worker_ends() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ladderback").join("ab").join("0");
        let (ctx, capture) = context(Some(dir.clone()));
        assert_eq!(cache_dir(&ctx).unwrap(), Some(dir.clone()));
        assert!(dir.is_dir(), "the folder is made");
        assert_eq!(logged(&capture), [], "nothing to warn about");
        let held = CacheLock::acquire(&dir).expect_err("another worker's lock");
        assert_eq!(held.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn a_cache_folder_another_worker_holds_means_no_cache_and_a_warning_that_says_why() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("0");
        let _other_worker = CacheLock::acquire(&dir).unwrap();
        let (ctx, capture) = context(Some(dir.clone()));
        assert_eq!(cache_dir(&ctx).unwrap(), None);
        let [(level, message)] = logged(&capture).try_into().unwrap();
        assert_eq!(level, Level::Warn);
        assert!(
            message.contains("running without a cache")
                && message.contains("another worker is using it")
                && message.contains(&dir.display().to_string()),
            "{message}"
        );
    }

    #[test]
    fn a_cache_folder_that_cannot_be_made_means_no_cache_and_a_warning_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        let dir = file.join("cache");
        let reason = CacheLock::acquire(&dir)
            .expect_err("under a file")
            .to_string();
        let (ctx, capture) = context(Some(dir.clone()));
        assert_eq!(cache_dir(&ctx).unwrap(), None);
        let [(level, message)] = logged(&capture).try_into().unwrap();
        assert_eq!(level, Level::Warn);
        assert!(
            message.contains("running without a cache")
                && message.contains(&dir.display().to_string())
                && message.contains(&reason),
            "{message} should give the reason: {reason}"
        );
        assert!(
            !message.contains("another worker"),
            "a folder that can't be made isn't a held one: {message}"
        );
    }

    #[test]
    fn a_stream_that_broke_while_the_missing_cache_was_reported_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("0");
        let _other_worker = CacheLock::acquire(&dir).unwrap();
        let (events, _gave_up) = breaking_after(0);
        let ctx = context_with(Some(dir), events);
        let error = cache_dir(&ctx).expect_err("the warning can't be written");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn a_repository_opened_with_a_cache_folder_uses_it() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let cache = root.path().join("cache");
        let (ctx, _capture) = context(Some(cache.clone()));
        let opened = open(&ctx, &spec_at(&repository.to_string_lossy())).unwrap();
        drop(opened);
        assert!(
            fs::read_dir(&cache).is_ok_and(|mut entries| entries.next().is_some()),
            "rustic kept nothing in the cache folder"
        );
    }

    #[test]
    fn a_repository_opened_while_another_worker_holds_the_cache_folder_leaves_it_alone() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let cache = root.path().join("cache");
        let _other_worker = CacheLock::acquire(&cache).unwrap();
        let (ctx, capture) = context(Some(cache.clone()));
        drop(open(&ctx, &spec_at(&repository.to_string_lossy())).unwrap());
        assert_eq!(
            fs::read_dir(&cache).unwrap().count(),
            0,
            "two workers shared one cache folder"
        );
        assert!(
            logged(&capture)
                .iter()
                .any(|(level, _)| *level == Level::Warn)
        );
    }

    // Opening.

    #[test]
    fn a_wrong_password_fails_the_job() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let (ctx, _capture) = context(None);
        let wrong = "hunter2-not-the-password";
        let spec = RepositorySpec {
            password: SecretValue::new(wrong),
            ..spec_at(&repository.to_string_lossy())
        };
        let Err(JobError::Failed { detail, .. }) = open(&ctx, &spec) else {
            panic!("a wrong password must fail the job");
        };
        assert!(!detail.is_empty());
        assert!(!detail.contains(wrong), "the password is in {detail}");
    }

    #[test]
    fn a_folder_that_is_no_repository_is_a_failure_and_is_left_as_it_is() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("home");
        let lookalike = leftover(&folder);
        let (ctx, capture) = context(None);
        let opened = open(&ctx, &spec_at(&folder.to_string_lossy()));
        assert!(matches!(opened, Err(JobError::Failed { .. })));
        assert!(lookalike.exists(), "somebody's file was removed");
        assert_eq!(logged(&capture), []);
    }

    #[test]
    fn a_repository_that_does_not_exist_is_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let (ctx, _capture) = context(None);
        let missing = root.path().join("nowhere");
        let opened = open(&ctx, &spec_at(&missing.to_string_lossy()));
        assert!(matches!(opened, Err(JobError::Failed { .. })));
        assert!(!missing.exists(), "opening made a repository folder");
    }

    #[test]
    fn a_backend_rustic_cannot_set_up_is_a_failure_before_anything_is_opened() {
        let (ctx, _capture) = context(None);
        let opened = new_repository(&ctx, &spec_at("nosuchbackend:somewhere"));
        assert!(matches!(opened, Err(JobError::Failed { .. })));
    }

    #[test]
    fn the_id_of_an_opened_repository_is_its_64_hex_characters() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let (ctx, _capture) = context(None);
        let opened = open(&ctx, &spec_at(&repository.to_string_lossy())).unwrap();
        let id = repository_id(&opened).unwrap();
        assert_eq!(id.as_str(), opened.config().id.to_hex().as_str());
        assert_eq!(id.as_str().len(), 64);
        assert!(
            id.as_str()
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
            "{id:?}"
        );
    }

    // Leftovers.

    #[test]
    fn a_leftover_removed_after_opening_is_logged_by_name() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let old = leftover(&repository);
        let (ctx, capture) = context(None);
        drop(open(&ctx, &spec_at(&repository.to_string_lossy())).unwrap());
        assert!(!old.exists());
        let [(level, message)] = logged(&capture).try_into().unwrap();
        assert_eq!(level, Level::Info);
        assert!(
            message.contains("removed a leftover temporary file")
                && message.contains(&old.display().to_string()),
            "{message}"
        );
    }

    #[test]
    fn both_halves_of_a_hot_cold_repository_are_cleaned() {
        let root = tempfile::tempdir().unwrap();
        let cold = root.path().join("cold");
        let hot = root.path().join("hot");
        let backends = BackendOptions::default()
            .repository(cold.to_string_lossy().into_owned())
            .repo_hot(hot.to_string_lossy().into_owned())
            .to_backends()
            .unwrap();
        Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
            .unwrap()
            .init(
                &Credentials::password(PASSWORD),
                &KeyOptions::default(),
                &ConfigOptions::default(),
            )
            .unwrap();
        let (old_cold, old_hot) = (leftover(&cold), leftover(&hot));
        let (ctx, capture) = context(None);
        let spec = RepositorySpec {
            repo_hot: Some(hot.to_string_lossy().into_owned()),
            ..spec_at(&cold.to_string_lossy())
        };
        drop(open(&ctx, &spec).unwrap());
        assert!(!old_cold.exists() && !old_hot.exists());
        assert_eq!(logged(&capture).len(), 2, "one line for each");
    }

    /// What decides is the kind of backend, not whether the place exists: a
    /// folder of a repository is cleaned when it is the local half, and left
    /// alone when the same path is what another backend is told to use.
    #[test]
    fn only_a_local_half_is_cleaned_whatever_exists_at_its_path() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let old = leftover(&repository);
        let (ctx, capture) = context(None);
        for half in [
            format!("rest:{}", repository.display()),
            format!("rclone:{}", repository.display()),
        ] {
            remove_leftovers(&ctx, &half).unwrap();
            assert!(old.exists(), "{half} was cleaned like a local folder");
        }
        assert_eq!(logged(&capture), []);
        remove_leftovers(&ctx, &repository.to_string_lossy()).unwrap();
        assert!(!old.exists(), "the local half is cleaned");
    }

    #[test]
    fn a_leftover_that_cannot_be_removed_is_a_warning_and_the_job_goes_on() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let stuck = leftover(&repository);
        let folder = stuck.parent().unwrap().to_owned();
        let _restored = Writable(folder.clone());
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        // Running as root, permissions don't stop a delete.
        let can_write_anyway = File::create(folder.join("probe")).is_ok();
        let (ctx, capture) = context(None);
        let opened = open(&ctx, &spec_at(&repository.to_string_lossy()));
        if can_write_anyway {
            testkit::skip!("running as root: permissions don't stop deleting a file");
        }
        assert!(opened.is_ok(), "failing to clean up must not fail the job");
        let [(level, message)] = logged(&capture).try_into().unwrap();
        assert_eq!(level, Level::Warn);
        assert!(
            message.contains("could not remove a leftover temporary file")
                && message.contains(&stuck.display().to_string()),
            "{message}"
        );
        assert!(stuck.exists());
    }

    #[test]
    fn a_stream_that_broke_while_the_leftovers_were_reported_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        leftover(&repository);
        let (events, _gave_up) = breaking_after(0);
        let ctx = context_with(None, events);
        let error =
            remove_leftovers(&ctx, &repository.to_string_lossy()).expect_err("nobody can be told");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
