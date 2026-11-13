//! Opening a repository for a job.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use api::ErrorCode;
use domain::RepositoryId;
use reqwest::StatusCode;
use rustic_backend::util::{BackendLocation, location_to_type_and_path};
use rustic_backend::{BackendOptions, RestBackend, SupportedBackend};
use rustic_core::{
    Credentials, FileType, OpenStatus, ReadBackend, Repository, RepositoryOptions, RusticError,
};

use super::Context;
use super::cancel::refusing_snapshots_once_cancelled;
use super::convert::JobError;
use super::progress::EventProgressBars;
use super::tmp_files::remove_stale_files;
use crate::job::{Level, ProbeResult, RepositorySpec};

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
/// tells a repository from a folder of files. A spec that names the
/// repository's ID opens only that repository: one with another ID is
/// refused at once (`repository-replaced`, §7.3), before anything in it is
/// read past its config or written. After it opens, each half that is a
/// local folder is cleaned up after killed workers.
///
/// A half on OpenDAL's `fs` whose root folder isn't there fails the job
/// before its backend is set up, which would make that folder (decision 53).
/// When the repository doesn't open, and only then, a REST server's refusal
/// of the credentials fails the job with `credentials-refused` (#478), and
/// its folders are looked at: when each half is a folder on this machine
/// holding nothing of a repository, the job fails with
/// `repository-not-found`, and otherwise with the error opening gave, as it
/// always did. A repository that opens costs no extra look.
pub(crate) fn open(
    ctx: &Context,
    spec: &RepositorySpec,
) -> Result<Repository<OpenStatus>, JobError> {
    if let Some(root) = missing_fs_root(spec) {
        return Err(JobError::failed(format!(
            "the repository's root folder {} is not there",
            root.display()
        )));
    }
    let credentials = Credentials::password(spec.password.expose());
    let repository = match new_repository(ctx, spec)?.open(&credentials) {
        Ok(repository) => repository,
        Err(error) => {
            if let Some(status) = refusal(spec, &error) {
                return Err(JobError::Failed {
                    code: ErrorCode::CredentialsRefused,
                    detail: format!(
                        "the server answered {status}: it refused the credentials sent for the repository"
                    ),
                });
            }
            if no_repository_there(spec) {
                return Err(JobError::Failed {
                    code: ErrorCode::RepositoryNotFound,
                    detail: "no repository config file is at the location".to_owned(),
                });
            }
            return Err(error.into());
        }
    };
    if let Some(expected) = &spec.id {
        let found = repository_id(&repository)?;
        if found != *expected {
            return Err(JobError::Failed {
                code: ErrorCode::RepositoryReplaced,
                detail: format!(
                    "the location holds the repository {}, not {}",
                    found.as_str(),
                    expected.as_str()
                ),
            });
        }
    }
    // Only a repository that opened is known to be one, and the one the
    // configuration means, so only then is what looks like its leftovers ours
    // to remove.
    for half in std::iter::once(spec.repository.as_str()).chain(spec.repo_hot.as_deref()) {
        remove_leftovers(ctx, half)?;
    }
    Ok(repository)
}

/// Tries one location (decision 80): builds its backend with its own
/// settings, reads the repository's `config` through it, opens the
/// repository with the password and reads its ID. It writes nothing: unlike
/// [`open`], it removes no leftovers, since nothing has yet said the location
/// holds the repository the entry means. A location that doesn't answer
/// fails the probe with what rustic said; the daemon then calls it silent.
pub(crate) fn probe(ctx: &Context, spec: &RepositorySpec) -> Result<ProbeResult, JobError> {
    if let Some(root) = missing_fs_root(spec) {
        return Err(JobError::failed(format!(
            "the repository's root folder {} is not there",
            root.display()
        )));
    }
    let repository = new_repository(ctx, spec)?;
    match repository.config_id() {
        Ok(Some(_)) => {}
        // rustic's REST backend reads `config` with a `HEAD` and takes any
        // failure for "not there", a refusal of the credentials included: a
        // listing, which fails on a refusal, tells the two apart.
        Ok(None) => {
            return Ok(match repository.infos_files() {
                Err(error) if refused(&error) => ProbeResult::Refused,
                Ok(_) | Err(_) => ProbeResult::NotARepository,
            });
        }
        Err(error) if refused(&error) => return Ok(ProbeResult::Refused),
        Err(error) => return Err(error.into()),
    }
    let credentials = Credentials::password(spec.password.expose());
    match repository.open(&credentials) {
        Ok(repository) => Ok(ProbeResult::Holds {
            id: repository_id(&repository)?,
        }),
        // rustic's code for "no key opens with this password".
        Err(error) if error.is_code(WRONG_PASSWORD) => Ok(ProbeResult::PasswordNotAccepted),
        Err(error) if refused(&error) => Ok(ProbeResult::Refused),
        Err(error) => Err(error.into()),
    }
}

/// rustic_core's error code for a password that opens no key (`keyfile.rs`).
const WRONG_PASSWORD: &str = "C002";

/// Whether the backend answered and refused the credentials: an HTTP 401 or
/// 403 from a REST or OpenDAL server, or OpenDAL's permission error, found
/// anywhere in the error's chain.
fn refused(error: &RusticError) -> bool {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = cause {
        let text = error.to_string();
        if ["401 Unauthorized", "403 Forbidden", "PermissionDenied"]
            .iter()
            .any(|sign| text.contains(sign))
        {
            return true;
        }
        cause = error.source();
    }
    false
}

/// The refusal a REST server the repository is on gave its credentials
/// (HTTP 401 or 403), if one did, as the failed opening's `error` tells it.
/// rustic's REST backend reads the config with a `HEAD` and takes any status
/// but success for "no config file", dropping the status (`rest.rs`). So
/// when the error holds no answer of a server, and only then, each half on
/// a REST server lists its keys, a `GET` whose status rustic keeps, once,
/// without retrying. No other half is set up again: an rclone half starts a
/// process, and one on OpenDAL's `fs` makes its root folder.
fn refusal(spec: &RepositorySpec, error: &RusticError) -> Option<StatusCode> {
    match answer(error) {
        Some(status) => status.filter(refuses),
        None => rest_halves(spec).find_map(|(location, options)| {
            let listed =
                RestBackend::new(location, options).and_then(|backend| backend.list(FileType::Key));
            answer(listed.err()?.as_ref()).flatten().filter(refuses)
        }),
    }
}

/// What `error`'s chain says of an HTTP request: `None` when it holds no
/// HTTP client's error, and otherwise the status the server answered with,
/// if it answered at all.
fn answer(error: &(dyn std::error::Error + 'static)) -> Option<Option<StatusCode>> {
    std::iter::successors(Some(error), |error| error.source())
        .find_map(|error| error.downcast_ref::<reqwest::Error>())
        .map(reqwest::Error::status)
}

/// Whether `status` is a server's refusal of the credentials.
fn refuses(status: &StatusCode) -> bool {
    matches!(*status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

/// Each half of the repository on a REST server, the cold one first: its
/// location, and its options as rustic gives them to it (`options_cold` or
/// `options_hot` over `options`), with retries off.
fn rest_halves(
    spec: &RepositorySpec,
) -> impl Iterator<Item = (BackendLocation, BTreeMap<String, String>)> + '_ {
    let cold = (spec.repository.as_str(), &spec.options_cold);
    let hot = spec.repo_hot.as_deref().map(|hot| (hot, &spec.options_hot));
    std::iter::once(cold)
        .chain(hot)
        .filter_map(|(location, half_options)| {
            let Ok((SupportedBackend::Rest, location)) = location_to_type_and_path(location) else {
                return None;
            };
            let mut options = spec.options.clone();
            options.extend(half_options.clone());
            options.insert("retry".to_owned(), "off".to_owned());
            Some((location, options))
        })
}

/// Where a half of the repository is, as far as telling "no repository"
/// goes (decision 53).
enum Half {
    /// A local folder. rustic's local backend makes no folder when it opens.
    Local(PathBuf),
    /// The root folder of OpenDAL's `fs`, the root as rustic sets the half
    /// up: `options_cold` over `options` for the cold half, `options_hot`
    /// over `options` for the hot one. Setting that backend up makes a
    /// missing root folder.
    Fs(PathBuf),
    /// Anywhere else: a server or a store elsewhere (REST, rclone, and every
    /// other OpenDAL service), or a location rustic refuses, such as `fs`
    /// with no root.
    Elsewhere,
}

/// The repository's halves, the cold one first.
fn halves(spec: &RepositorySpec) -> impl Iterator<Item = Half> + '_ {
    let cold = (spec.repository.as_str(), &spec.options_cold);
    let hot = spec.repo_hot.as_deref().map(|hot| (hot, &spec.options_hot));
    std::iter::once(cold).chain(hot).map(
        |(location, half_options)| match location_to_type_and_path(location) {
            Ok((SupportedBackend::Local, path)) => Half::Local(PathBuf::from(path.as_str())),
            Ok((SupportedBackend::OpenDAL, service))
                if service
                    .as_str()
                    .split(':')
                    .next()
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("fs")) =>
            {
                half_options
                    .get("root")
                    .or_else(|| spec.options.get("root"))
                    .map_or(Half::Elsewhere, |root| Half::Fs(PathBuf::from(root)))
            }
            _ => Half::Elsewhere,
        },
    )
}

/// The root folder of a half on OpenDAL's `fs` that isn't there, or isn't a
/// folder. Setting that backend up would make it, and on an unplugged drive
/// that is an empty folder at the mount point, on the system disk: the next
/// job would find no repository in it and the row would offer to create one
/// there (decision 53).
fn missing_fs_root(spec: &RepositorySpec) -> Option<PathBuf> {
    halves(spec).find_map(|half| match half {
        Half::Fs(root) if !is_folder(&root) => Some(root),
        _ => None,
    })
}

/// Whether the repository is, definitely, not there (decision 53): every
/// half is a folder on this machine that holds nothing of a repository.
/// A half anywhere else is no answer: a server's or a store's missing root
/// is the remote form of an unplugged drive, and rustic's backends there
/// list no config file for one, or for a refused password.
fn no_repository_there(spec: &RepositorySpec) -> bool {
    halves(spec).all(|half| match half {
        Half::Local(folder) | Half::Fs(folder) => holds_nothing_of_a_repository(&folder),
        Half::Elsewhere => false,
    })
}

/// What a repository holds at the top of its folder: its config file, and
/// the folders of its keys, packs, index and snapshots.
const REPOSITORY_ENTRIES: [&str; 5] = ["config", "keys", "data", "index", "snapshots"];

/// Whether `folder` is there, is a folder, and holds none of
/// [`REPOSITORY_ENTRIES`], as the file system tells it. A folder that isn't
/// there is what an unplugged drive looks like, and a repository made there
/// would be a new, empty one in the old one's place; a folder that can't be
/// read is no answer; and a folder holding a repository's other entries
/// without its config file is an initialisation cut short or a repository
/// that lost its config, which a new repository would be mixed into.
fn holds_nothing_of_a_repository(folder: &Path) -> bool {
    is_folder(folder)
        && REPOSITORY_ENTRIES
            .iter()
            .all(|entry| matches!(folder.join(entry).try_exists(), Ok(false)))
}

/// Whether a folder is at `path`, through any symbolic links; anything that
/// can't be looked at is not one.
fn is_folder(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
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
    use std::assert_matches;
    use std::fs::{self, File, FileTimes};
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
            id: None,
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
        fs::set_times(
            &path,
            FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 3600)),
        )
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
        // A repository that is there, with the wrong password, is the
        // engine's own failure: only a missing config is not-found.
        let Err(JobError::Failed { code, detail }) = open(&ctx, &spec) else {
            panic!("a wrong password must fail the job");
        };
        assert_eq!(code, api::ErrorCode::OperationFailed);
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
        assert_matches!(opened, Err(JobError::Failed { .. }));
        assert!(lookalike.exists(), "somebody's file was removed");
        assert_eq!(logged(&capture), []);
    }

    #[test]
    fn a_repository_that_does_not_exist_is_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let (ctx, _capture) = context(None);
        let missing = root.path().join("nowhere");
        let opened = open(&ctx, &spec_at(&missing.to_string_lossy()));
        assert_matches!(opened, Err(JobError::Failed { .. }));
        assert!(!missing.exists(), "opening made a repository folder");
    }

    /// A repository folder that is there and holds no repository config
    /// file is no repository: the job fails with `repository-not-found`,
    /// decided by looking for the config file rather than by rustic's
    /// words, and nothing is made there.
    #[test]
    fn an_existing_folder_with_no_repository_config_is_no_repository_found() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let (ctx, _capture) = context(None);
        let Err(JobError::Failed { code, detail }) = open(&ctx, &spec_at(&empty.to_string_lossy()))
        else {
            panic!("opening an empty folder must fail the job");
        };
        assert_eq!(code, api::ErrorCode::RepositoryNotFound);
        assert!(!detail.is_empty());
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    }

    /// A repository folder that isn't there, or isn't a folder, is no
    /// definite answer: it is what an unplugged drive looks like, and
    /// creating a repository there would make a new, empty one in its
    /// place (decision 53). The job fails as it would have, never as
    /// `repository-not-found`, and nothing is made.
    #[test]
    fn a_repository_folder_that_is_not_there_is_not_said_to_hold_no_repository() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        let missing = root.path().join("nowhere");
        let unplugged = root.path().join("usb").join("restic");
        for location in [&missing, &unplugged, &file] {
            assert_eq!(
                failed_code(&spec_at(&location.to_string_lossy())),
                api::ErrorCode::OperationFailed,
                "{location:?}"
            );
        }
        assert!(!missing.exists(), "opening made a repository folder");
        assert!(!unplugged.exists(), "opening made a repository folder");
    }

    /// Opens `spec` and returns the code the job failed with.
    fn failed_code(spec: &RepositorySpec) -> api::ErrorCode {
        let (ctx, _capture) = context(None);
        match open(&ctx, spec) {
            Err(JobError::Failed { code, .. }) => code,
            Err(other) => panic!("opening {} broke the events: {other:?}", spec.repository),
            Ok(_) => panic!("{} opened", spec.repository),
        }
    }

    /// A location the backend says no config file is at, when the backend
    /// says so of a location it couldn't read too, is not a definite answer:
    /// the job fails as it would have, never as `repository-not-found`, so
    /// a repository that is only out of reach is never offered to be
    /// created (decision 53). Here a real repository in a folder nobody may
    /// read, which rustic's local backend lists as holding no config file.
    #[test]
    fn a_folder_that_cannot_be_read_is_not_said_to_hold_no_repository() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let _restored = Writable(repository.clone());
        fs::set_permissions(&repository, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&repository).is_ok() {
            testkit::skip!("running as root: permissions don't stop reading a folder");
        }
        assert_eq!(
            failed_code(&spec_at(&repository.to_string_lossy())),
            api::ErrorCode::OperationFailed
        );
    }

    /// An HTTP server on a free local port that answers every request with
    /// `status` (`401 Unauthorized`, say) and an empty body. It serves until
    /// the test process ends.
    fn server_answering(status: &'static str) -> std::net::SocketAddr {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                let mut length = 0;
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let _ = io::copy(&mut io::Read::take(&mut reader, length), &mut io::sink());
                let _ = (&stream).write_all(
                    format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                        .as_bytes(),
                );
            }
        });
        address
    }

    /// A REST server that refuses the credentials, as `401 Unauthorized`.
    fn refusing_rest_server() -> String {
        format!("rest:http://{}/", server_answering("401 Unauthorized"))
    }

    /// rustic's REST backend lists no config file when the server answers
    /// the config's request with any status but success, a refusal of the
    /// credentials included, and opening then says no config file is there.
    /// The job says the server refused them, as 401 and as 403 alike (#478).
    #[test]
    fn a_rest_server_that_refuses_the_credentials_fails_the_job_as_refused() {
        assert_eq!(
            failed_code(&spec_at(&refusing_rest_server())),
            api::ErrorCode::CredentialsRefused
        );
        let forbidding = format!("rest:http://{}/", server_answering("403 Forbidden"));
        assert_eq!(
            failed_code(&spec_at(&forbidding)),
            api::ErrorCode::CredentialsRefused
        );
    }

    /// A hot half on a REST server that refuses the credentials fails the job
    /// as refused too, though the cold half is a repository that opens.
    #[test]
    fn a_hot_half_on_a_rest_server_that_refuses_the_credentials_fails_the_job_as_refused() {
        let root = tempfile::tempdir().unwrap();
        let cold = root.path().join("cold");
        init(&cold);
        let spec = RepositorySpec {
            repo_hot: Some(refusing_rest_server()),
            ..spec_at(&cold.to_string_lossy())
        };
        assert_eq!(failed_code(&spec), api::ErrorCode::CredentialsRefused);
    }

    /// A REST server that takes the credentials and holds no repository
    /// answers `404 Not Found`, which is no refusal: the job fails as it
    /// always did, never as `repository-not-found`, since a server's missing
    /// repository can't be told from one out of reach (decision 53).
    #[test]
    fn a_rest_server_that_holds_no_repository_fails_the_job_as_before() {
        let empty = format!("rest:http://{}/", server_answering("404 Not Found"));
        assert_eq!(
            failed_code(&spec_at(&empty)),
            api::ErrorCode::OperationFailed
        );
    }

    // The probe of a location (decision 80).

    fn probed(spec: &RepositorySpec) -> Result<ProbeResult, JobError> {
        let (ctx, _capture) = context(None);
        probe(&ctx, spec)
    }

    #[test]
    fn a_probe_of_a_repository_answers_with_its_id_and_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let lookalike = leftover(&repository);
        let Ok(ProbeResult::Holds { id }) = probed(&spec_at(&repository.to_string_lossy())) else {
            panic!("the repository must answer");
        };
        let (ctx, _capture) = context(None);
        let opened = open(&ctx, &spec_at(&repository.to_string_lossy())).unwrap();
        assert_eq!(id, repository_id(&opened).unwrap());
        // `open` removed it just now; the probe before it had left it.
        assert!(!lookalike.exists());
        let again = leftover(&repository);
        probed(&spec_at(&repository.to_string_lossy())).unwrap();
        assert!(again.exists(), "the probe removed a file");
    }

    #[test]
    fn a_probe_with_a_password_that_opens_nothing_says_so() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let spec = RepositorySpec {
            password: SecretValue::new("hunter2-not-the-password"),
            ..spec_at(&repository.to_string_lossy())
        };
        assert_eq!(probed(&spec).ok(), Some(ProbeResult::PasswordNotAccepted));
    }

    #[test]
    fn a_probe_of_an_empty_folder_finds_no_repository() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            probed(&spec_at(&root.path().to_string_lossy())).ok(),
            Some(ProbeResult::NotARepository)
        );
    }

    #[test]
    fn a_probe_of_a_rest_server_that_refuses_the_credentials_says_so() {
        assert_eq!(
            probed(&spec_at(&refusing_rest_server())).ok(),
            Some(ProbeResult::Refused)
        );
        let forbidding = format!("rest:http://{}/", server_answering("403 Forbidden"));
        assert_eq!(
            probed(&spec_at(&forbidding)).ok(),
            Some(ProbeResult::Refused)
        );
    }

    /// The refusal is told by rustic's own error text at the revision the
    /// workspace pins (`refused`): these two and the REST ones above fail
    /// when an upgrade changes that text, rather than the probe calling a
    /// refusal something else. OpenDAL's `fs` service says `PermissionDenied`
    /// of a folder nobody may read.
    #[test]
    fn a_probe_of_an_opendal_folder_nobody_may_read_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let _restored = Writable(repository.clone());
        fs::set_permissions(&repository, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&repository).is_ok() {
            testkit::skip!("running as root: permissions don't stop reading a folder");
        }
        let spec = RepositorySpec {
            options: [("root".to_owned(), repository.to_string_lossy().into_owned())].into(),
            ..spec_at("opendal:fs")
        };
        assert_eq!(probed(&spec).ok(), Some(ProbeResult::Refused));
    }

    #[test]
    fn a_probe_of_a_location_that_is_not_there_fails() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("gone").join("repo");
        let spec = RepositorySpec {
            options: [("root".to_owned(), missing.to_string_lossy().into_owned())].into(),
            ..spec_at("opendal:fs")
        };
        assert_matches!(probed(&spec), Err(JobError::Failed { .. }));
    }

    /// A server that can't be reached gives no answer at all: the job fails
    /// with the backend's error, as it would have.
    #[test]
    fn a_rest_server_that_cannot_be_reached_is_not_said_to_hold_no_repository() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let spec = RepositorySpec {
            options: [("retry".to_owned(), "off".to_owned())].into(),
            ..spec_at(&format!("rest:http://127.0.0.1:{port}/"))
        };
        assert_eq!(failed_code(&spec), api::ErrorCode::OperationFailed);
    }

    /// OpenDAL's `fs` is a folder on this machine, looked at as a local one:
    /// an empty root folder holds no repository, and one that can't be read
    /// is no answer, which fails the job as before. A root folder that isn't
    /// there is no definite answer either, as with a local folder: it fails
    /// the job as before, and before the backend is set up, which would
    /// make it.
    #[test]
    fn an_opendal_fs_root_is_no_repository_only_when_it_is_an_empty_folder() {
        let root = tempfile::tempdir().unwrap();
        let at = |folder: &Path| RepositorySpec {
            options: [("root".to_owned(), folder.to_string_lossy().into_owned())].into(),
            ..spec_at("opendal:fs")
        };
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(failed_code(&at(&empty)), api::ErrorCode::RepositoryNotFound);
        let in_options_cold = RepositorySpec {
            options_cold: [("root".to_owned(), empty.to_string_lossy().into_owned())].into(),
            ..at(&root.path().join("overridden"))
        };
        assert_eq!(
            failed_code(&in_options_cold),
            api::ErrorCode::RepositoryNotFound
        );
        let missing = root.path().join("usb").join("restic");
        assert_eq!(failed_code(&at(&missing)), api::ErrorCode::OperationFailed);
        assert!(
            !root.path().join("usb").exists(),
            "opening made a root folder"
        );
        // The hot half's root is `options_hot` over `options`; the cold half
        // here is a local folder, which has no use for a root option.
        let hot_missing = root.path().join("usb-hot").join("restic");
        let hot_unplugged = RepositorySpec {
            repo_hot: Some("opendal:fs".to_owned()),
            options: [("root".to_owned(), empty.to_string_lossy().into_owned())].into(),
            options_hot: [(
                "root".to_owned(),
                hot_missing.to_string_lossy().into_owned(),
            )]
            .into(),
            ..spec_at(&empty.to_string_lossy())
        };
        assert_eq!(failed_code(&hot_unplugged), api::ErrorCode::OperationFailed);
        assert!(
            !root.path().join("usb-hot").exists(),
            "opening made a hot root folder"
        );
        let repository = root.path().join("repo");
        init(&repository);
        let _restored = Writable(repository.clone());
        fs::set_permissions(&repository, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&repository).is_ok() {
            testkit::skip!("running as root: permissions don't stop reading a folder");
        }
        assert_eq!(
            failed_code(&at(&repository)),
            api::ErrorCode::OperationFailed
        );
    }

    /// Only OpenDAL's `fs` is a folder on this machine. Every other service
    /// is a server or a store elsewhere, where a missing root is the remote
    /// form of an unplugged drive: rustic lists no config file for one that
    /// isn't there, and creating a repository would make it (decision 53).
    /// So the job fails as it would have, never as `repository-not-found`,
    /// like REST's. Here a WebDAV server that answers `404 Not Found` to
    /// everything and an empty in-memory store.
    #[test]
    fn an_opendal_service_other_than_fs_is_never_said_to_hold_no_repository() {
        let webdav = RepositorySpec {
            options: [
                (
                    "endpoint".to_owned(),
                    format!("http://{}", server_answering("404 Not Found")),
                ),
                ("retry".to_owned(), "off".to_owned()),
            ]
            .into(),
            ..spec_at("opendal:webdav")
        };
        for spec in [webdav, spec_at("opendal:memory")] {
            assert_eq!(
                failed_code(&spec),
                api::ErrorCode::OperationFailed,
                "{}",
                spec.repository
            );
        }
    }

    /// A folder holding what a repository holds beside its config file (a
    /// `keys`, `data`, `index` or `snapshots` folder) is no empty location:
    /// an initialisation cut short, or a repository whose config file was
    /// lost. Creating one there would mix a new repository into what is left
    /// of another, so the job fails as it would have, never as
    /// `repository-not-found` (decision 53), on either half and through
    /// OpenDAL's `fs` as well.
    #[test]
    fn a_folder_holding_repository_content_but_no_config_is_not_said_to_hold_no_repository() {
        let root = tempfile::tempdir().unwrap();
        let lost_config = root.path().join("lost-config");
        init(&lost_config);
        fs::remove_file(lost_config.join("config")).unwrap();
        assert_eq!(
            failed_code(&spec_at(&lost_config.to_string_lossy())),
            api::ErrorCode::OperationFailed
        );
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        for content in ["keys", "data", "index", "snapshots"] {
            let folder = root.path().join(content);
            fs::create_dir_all(folder.join(content)).unwrap();
            let location = folder.to_string_lossy().into_owned();
            let through_fs = RepositorySpec {
                options: [("root".to_owned(), location.clone())].into(),
                ..spec_at("opendal:fs")
            };
            let as_hot_half = RepositorySpec {
                repo_hot: Some(location.clone()),
                ..spec_at(&empty.to_string_lossy())
            };
            for spec in [spec_at(&location), through_fs, as_hot_half] {
                assert_eq!(
                    failed_code(&spec),
                    api::ErrorCode::OperationFailed,
                    "{content}: {spec:?}"
                );
            }
        }
    }

    /// A repository with a hot half is no repository only when both halves
    /// are empty folders: a hot half on a local folder that isn't there is
    /// what an unplugged drive looks like, as with the cold half, and
    /// creating the repository would make that folder (decision 53).
    #[test]
    fn a_local_hot_half_that_is_not_there_is_not_said_to_hold_no_repository() {
        let root = tempfile::tempdir().unwrap();
        let cold = root.path().join("cold");
        let hot = root.path().join("hot");
        fs::create_dir(&cold).unwrap();
        let with_hot = RepositorySpec {
            repo_hot: Some(hot.to_string_lossy().into_owned()),
            ..spec_at(&cold.to_string_lossy())
        };
        assert_eq!(failed_code(&with_hot), api::ErrorCode::OperationFailed);
        assert!(!hot.exists(), "opening made the hot folder");
        fs::create_dir(&hot).unwrap();
        assert_eq!(failed_code(&with_hot), api::ErrorCode::RepositoryNotFound);
    }

    #[test]
    fn a_backend_rustic_cannot_set_up_is_a_failure_before_anything_is_opened() {
        let (ctx, _capture) = context(None);
        let opened = new_repository(&ctx, &spec_at("nosuchbackend:somewhere"));
        assert_matches!(opened, Err(JobError::Failed { .. }));
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

    /// A spec that names the repository's ID opens that repository and no
    /// other (§7.3): a location that now holds another repository is refused
    /// as replaced, and its leftovers are not touched.
    #[test]
    fn a_location_that_holds_another_repository_than_the_one_named_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let old = leftover(&repository);
        let (ctx, capture) = context(None);
        let other = RepositoryId::parse(&"0a".repeat(32)).unwrap();
        let spec = RepositorySpec {
            id: Some(other.clone()),
            ..spec_at(&repository.to_string_lossy())
        };
        let Err(JobError::Failed { code, detail }) = open(&ctx, &spec) else {
            panic!("a repository with another ID must be refused");
        };
        assert_eq!(code, ErrorCode::RepositoryReplaced);
        assert!(detail.contains(other.as_str()), "{detail}");
        assert!(
            old.exists(),
            "a leftover of the other repository was removed"
        );
        assert_eq!(logged(&capture), []);
    }

    #[test]
    fn a_location_that_holds_the_repository_named_opens() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        init(&repository);
        let (ctx, _capture) = context(None);
        let location = repository.to_string_lossy();
        let id = repository_id(&open(&ctx, &spec_at(&location)).unwrap()).unwrap();
        let spec = RepositorySpec {
            id: Some(id.clone()),
            ..spec_at(&location)
        };
        let opened = open(&ctx, &spec).expect("the repository named opens");
        assert_eq!(repository_id(&opened).unwrap(), id);
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
