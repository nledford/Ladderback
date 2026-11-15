//! Starting a worker: its job, its scrubbed environment (invariant I4) and
//! what masks the job's secrets in what it says, its cache slot, and the
//! tasks that supervise it and read its output.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use app::ports::{Engine, EngineError, Job, RepositoryAccess, RunningJob};
use async_trait::async_trait;
use domain::secrets::{MAX_MASKED_BYTES, MaskedSecrets, Redactor, RepositorySecrets, SecretValue};
use domain::{OperationId, RepositoryId};
use futures::stream::{self, StreamExt};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::events::{Listing, Reading};
use super::job_kind::job_kind;
use super::stderr::forward_stderr;
use super::stop::{Control, DaemonStop, EventPipe, ForEvents, ForStop, kill_now, supervise};
use super::{MAX_CACHE_SLOTS, WorkerEngine};
use crate::environment;
use crate::job::{JobSpec, MAX_MESSAGE_BYTES};

#[async_trait]
impl Engine for WorkerEngine {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        // Held until the worker has exited, so a stop of the workers waits
        // for it; taken before anything starts, so a stop asked for meanwhile
        // reaches it.
        let stopping = self.stopping.subscribe();
        if *stopping.borrow() {
            // No worker, which nothing would be left to stop, and no answer:
            // a refusal would end the operation, announce it and run its
            // hooks, where the daemon's stop records it and announces nothing
            // (decision 92). The run ends with the daemon, and keeps no stop
            // of the workers waiting.
            drop(stopping);
            return std::future::pending().await;
        }
        let slot = match (&self.cache_root, repository_id(&job)) {
            (Some(root), Some(id)) => {
                let (slots, root, id) = (Arc::clone(&self.slots), root.clone(), id.clone());
                // Looking at the slots' locks is file I/O.
                tokio::task::spawn_blocking(move || slots.acquire(&root, &id))
                    .await
                    .unwrap_or(None)
            }
            _ => None,
        };
        if let Some(why) = without_cache(&job, self.cache_root.is_some(), slot.is_some()) {
            log::info!(
                "operation {}: its {:?} job runs without a cache: {why}",
                op.as_uuid().hyphenated(),
                job.kind()
            );
        }
        // What the worker starts with, and so what its proxy settings are:
        // a proxy URL's credentials are masked like the job's own secrets,
        // and a job past what is masked is not started (§10.2).
        let environment = environment::scrubbed(self.base_environment());
        refuse_too_much_to_mask(&job, &environment)?;
        let redactor = job_redactor(&job, environment::proxy_credentials(&environment));
        let spec = serde_json::to_vec(&JobSpec {
            op_id: op,
            cache_dir: slot.as_ref().map(|slot| slot.dir.clone()),
            watchdog_interval_ms: u64::try_from(self.watchdog_interval.as_millis())
                .unwrap_or(u64::MAX),
            job: job_kind(job)?,
        })
        .map_err(|e| failed_to_start(&self.program, e))?;
        // The worker refuses a longer spec, so don't start one to be refused.
        if spec.len() > MAX_MESSAGE_BYTES {
            return Err(EngineError::Unavailable {
                detail: format!("job spec exceeds {MAX_MESSAGE_BYTES} bytes"),
            });
        }
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            // Nothing the daemon holds in its environment goes to a worker
            // that doesn't need it: see `environment`.
            .env_clear()
            .envs(environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Piped, not inherited: the worker leads a process group of its
            // own, which a terminal with `stty tostop` treats as a background
            // job, and a write to the terminal would suspend it.
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // The worker leads a process group of its own: see `signal_group`.
        #[cfg(unix)]
        command.process_group(0);
        // Through the spawn gate: no other start inherits the worker's
        // pipes, its stdin above all, and it inherits none of theirs. Waited
        // for on the blocking pool: a start in flight may stall (decision 58).
        let mut child = spawn_gate::hold_blocking(move || command.spawn())
            .await
            .and_then(std::convert::identity)
            .map_err(|e| failed_to_start(&self.program, e))?;
        let Some(mut stdin) = child.stdin.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stdin"));
        };
        // The deadline for the job to be taken and answered: see
        // `Reading::first_line_by`. `None` is a timeout too long to
        // represent, which never passes.
        let first_line_by = tokio::time::Instant::now().checked_add(self.start_timeout);
        // A worker that never reads its job, because it hung before it got to
        // it, leaves this write blocked once the pipe is full.
        let write = stdin.write_all(&spec);
        let written = match first_line_by {
            Some(at) => tokio::time::timeout_at(at, write).await,
            None => Ok(write.await),
        };
        match written {
            Ok(written) => written.map_err(|e| failed_to_start(&self.program, e))?,
            Err(_) => {
                // The worker is not waited for: nothing here may be able to
                // hold `run` back. The runtime reaps the dropped child, and
                // the cache slot is free again as `slot` goes out of scope.
                kill_now(&mut child);
                return Err(EngineError::StartTimedOut);
            }
        }
        // Closing stdin tells the worker the job spec is complete.
        drop(stdin);
        let Some(stdout) = child.stdout.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stdout"));
        };
        let Some(stderr) = child.stderr.take() else {
            return Err(failed_to_start(&self.program, "the worker has no stderr"));
        };

        let (commands, command_rx) = mpsc::unbounded_channel();
        let (exited_tx, exited) = watch::channel(None);
        let stopped = Arc::new(AtomicBool::new(false));
        let daemon_stopped = Arc::new(AtomicBool::new(false));
        let pipe = EventPipe {
            stdout: Arc::new(Mutex::new(stdout)),
            stopped: Arc::clone(&stopped),
        };
        tokio::spawn(supervise(
            child,
            command_rx,
            exited_tx,
            self.stop_grace,
            DaemonStop {
                asked: stopping,
                pipe: ForStop(pipe.clone()),
                stopped: Arc::clone(&daemon_stopped),
            },
            slot,
        ));
        tokio::spawn(forward_stderr(
            BufReader::new(stderr),
            exited.clone(),
            redactor.clone(),
        ));
        let reading = Reading {
            stdout: BufReader::new(ForEvents(pipe.clone())),
            line: Vec::new(),
            exited: exited.clone(),
            commands: commands.clone(),
            stopped: Arc::clone(&stopped),
            daemon_stopped,
            done: false,
            drain_until: None,
            redactor,
            exit_timeout: self.exit_timeout,
            first_line_by,
            listing: Listing::default(),
        };
        Ok(RunningJob {
            events: stream::unfold(reading, Reading::next).boxed(),
            control: Box::new(Control {
                commands,
                exited,
                stopped,
                pipe: ForStop(pipe),
            }),
        })
    }
}

fn failed_to_start(program: &Path, error: impl std::fmt::Display) -> EngineError {
    EngineError::Unavailable {
        detail: format!("{}: {error}", program.display()),
    }
}

fn repository_id(job: &Job) -> Option<&RepositoryId> {
    match job {
        Job::Fake(_) => None,
        Job::Identify(access) | Job::Init(access) | Job::Ls(access) | Job::Stats(access) => {
            access.id.as_ref()
        }
        Job::Browse(job) => job.repository.id.as_ref(),
        // A probe names no repository, so it takes no cache slot: it opens
        // with no cache (decision 80).
        Job::Probe(_) => None,
        Job::Backup(job) => job.repository.id.as_ref(),
        Job::Forget(job) => job.repository.id.as_ref(),
        Job::Rewrite(job) => job.repository.id.as_ref(),
        Job::Prune(job) | Job::PrunePlan(job) => job.repository.id.as_ref(),
        Job::Check(job) => job.repository.id.as_ref(),
        Job::Restore(job) | Job::RestorePlan(job) => job.repository.id.as_ref(),
    }
}

/// Why a job that opens a repository runs without a cache, for the daemon's
/// log: a cache is kept by the repository's ID, in a slot no other worker
/// uses (§4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoCache {
    /// The job names no repository ID: an identify that is run to learn it,
    /// an initialisation, or a job of a repository a request gave.
    IdUnknown,
    /// Every one of the repository's cache slots is held by another worker.
    EverySlotTaken,
}

impl std::fmt::Display for NoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdUnknown => {
                f.write_str("the repository's ID isn't known to it, and a cache is kept by that ID")
            }
            Self::EverySlotTaken => write!(
                f,
                "all {MAX_CACHE_SLOTS} of the repository's cache slots are in use by other workers"
            ),
        }
    }
}

/// Why `job` runs without a cache when the engine has a cache folder
/// (`cache_root`), and gave it a slot or not (`slot`). An engine with no
/// cache folder runs every job without one, and the daemon said so once when
/// it started.
fn without_cache(job: &Job, cache_root: bool, slot: bool) -> Option<NoCache> {
    if !cache_root || slot || access_of(job).is_none() {
        return None;
    }
    Some(match repository_id(job) {
        None => NoCache::IdUnknown,
        Some(_) => NoCache::EverySlotTaken,
    })
}

/// The repository a job opens, and so the secrets it holds; a fake job
/// opens none.
fn access_of(job: &Job) -> Option<&RepositoryAccess> {
    match job {
        Job::Fake(_) => None,
        Job::Identify(access)
        | Job::Probe(access)
        | Job::Init(access)
        | Job::Ls(access)
        | Job::Stats(access) => Some(access),
        Job::Backup(job) => Some(&job.repository),
        Job::Forget(job) => Some(&job.repository),
        Job::Rewrite(job) => Some(&job.repository),
        Job::Prune(job) | Job::PrunePlan(job) => Some(&job.repository),
        Job::Check(job) => Some(&job.repository),
        Job::Browse(job) => Some(&job.repository),
        Job::Restore(job) | Job::RestorePlan(job) => Some(&job.repository),
    }
}

/// Refuses a job whose secrets, with the proxy credentials of the worker's
/// `environment`, come to more than a redactor masks: its redactor would
/// mask every text whole, so a worker would run with nothing it says
/// readable (§10.2). They are counted before any redactor is built, the
/// job's own first, then each proxy variable's, and the refusal names the
/// variable at which they passed the cap, or none when the job's own did.
fn refuse_too_much_to_mask(
    job: &Job,
    environment: &[(OsString, OsString)],
) -> Result<(), EngineError> {
    let too_much = |field: Option<&str>| EngineError::SecretsTooLargeToMask {
        field: field.map(str::to_owned),
        limit: MAX_MASKED_BYTES,
    };
    let mut masked = MaskedSecrets::default();
    if let Some(access) = access_of(job) {
        masked
            .repository(access.known_secrets())
            .map_err(|_| too_much(None))?;
    }
    for (variable, credentials) in environment::proxy_credentials_by_variable(environment) {
        credentials
            .iter()
            .try_for_each(|credential| masked.secret(credential))
            .map_err(|_| too_much(Some(variable)))?;
    }
    Ok(())
}

/// What masks the job's secrets: the worker's stderr is logged through it,
/// and so are the events. `extra` is what else must not show, the proxy
/// credentials of the worker's environment. A fake job has no secrets of its
/// own.
fn job_redactor(job: &Job, extra: Vec<SecretValue>) -> Redactor {
    let Some(access) = access_of(job) else {
        return Redactor::new(&extra);
    };
    let resolved: Vec<SecretValue> = access.secrets.iter().cloned().chain(extra).collect();
    Redactor::for_repository(RepositorySecrets {
        resolved: &resolved,
        ..access.known_secrets()
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    use api::{ErrorCode, WarningCode};
    use app::ports::{
        EngineEvent, EngineResult, FakeEnd, FakeJob, ProgressEvent, ProgressKind as AppProgressKind,
    };
    use domain::Uuid;
    use domain::execution::{FailureReason, Warning, WarningKind};

    use super::*;
    use crate::client::test_support::*;
    use crate::client::translate::translate;
    use crate::job::{JobKind, Level, ProgressKind, WorkerEvent};

    /// The worker cuts a long text after masking it, but it can't mask the
    /// proxy credentials of its environment, which only the daemon knows: a
    /// credential its cut went through leaves nothing of itself (I4).
    #[tokio::test]
    async fn a_proxy_credential_the_worker_cut_through_leaves_nothing_of_itself() {
        capture_the_worker_log();
        let redactor = job_redactor(
            &fake_job(),
            environment::proxy_credentials(&[(
                OsString::from("HTTPS_PROXY"),
                OsString::from("http://demo:CANARY-proxy@proxy.demo:3128"),
            )]),
        );
        let cut = "proxy said http://demo:CANARY-pr…";
        let masked = "proxy said http://***…".to_owned();
        assert_eq!(
            translate(
                WorkerEvent::ProgressStart {
                    key: 1,
                    kind: ProgressKind::Spinner,
                    prefix: cut.to_owned(),
                },
                &redactor
            ),
            Some(EngineEvent::Progress(ProgressEvent::Begin {
                key: 1,
                kind: AppProgressKind::Spinner,
                step: None,
            }))
        );
        assert_eq!(
            translate(
                WorkerEvent::ProgressTitle {
                    key: 1,
                    title: cut.to_owned(),
                },
                &redactor
            ),
            Some(EngineEvent::Progress(ProgressEvent::Title {
                key: 1,
                step: None,
            }))
        );
        assert_eq!(
            translate(
                WorkerEvent::Warning {
                    item: Some(cut.to_owned()),
                    during: "connecting".to_owned(),
                    code: WarningCode::Unknown,
                    detail: cut.to_owned(),
                },
                &redactor
            ),
            Some(EngineEvent::Warning(Warning {
                kind: WarningKind::Other,
                item: Some(masked.clone()),
                during: "connecting".to_owned(),
                detail: masked.clone(),
            }))
        );
        assert_eq!(
            translate(failed_saying(ErrorCode::OperationFailed, cut), &redactor),
            Some(EngineEvent::Failed(FailureReason::EngineFailed {
                detail: masked.clone(),
            }))
        );
        let log_line = WorkerEvent::Log {
            level: Level::Info,
            message: format!("cut log: {cut}"),
        };
        assert_eq!(translate(log_line, &redactor), None);
        let line = logged_soon(log::Level::Info, |text| text.starts_with("cut log:"))
            .await
            .expect("the worker's log line never reached the log");
        assert_eq!(line, format!("cut log: {masked}"));
    }

    /// A job that runs without a cache says why in the daemon's log, and
    /// one that has a slot, opens no repository, or has no cache folder to
    /// get one from (said once when the daemon started) says nothing.
    #[test]
    fn a_job_without_a_cache_is_told_why() {
        let unknown = Job::Ls(access());
        let mut named = access();
        named.id = Some(RepositoryId::parse(&"07".repeat(32)).unwrap());
        let known = Job::Ls(named);
        assert_eq!(
            without_cache(&unknown, true, false),
            Some(NoCache::IdUnknown)
        );
        assert_eq!(
            without_cache(&known, true, false),
            Some(NoCache::EverySlotTaken)
        );
        assert_eq!(without_cache(&known, true, true), None);
        assert_eq!(without_cache(&unknown, false, false), None);
        assert_eq!(without_cache(&known, false, false), None);
        assert_eq!(without_cache(&fake_job(), true, false), None);
    }

    /// A worker inherits no descriptor that another start in the daemon has
    /// open: it starts through the spawn gate, so it waits while a pipe
    /// that is not yet close-on-exec is open, as another worker's, a hook's
    /// or a password command's pipes are for a moment where the platform
    /// has no `pipe2` (#234). A worker that held another's stdin would keep
    /// it from the end of its job until it ended itself. The script exits 4
    /// if it inherited a descriptor, 3 if not.
    #[tokio::test]
    async fn a_worker_inherits_no_pipe_another_start_has_open() {
        let pipe = testkit::inheritable::InheritablePipe::open();
        let [read, write] = pipe.numbers();
        // No pipeline and no `$(…)`: either makes a pipe the test could find.
        let script = concat!(
            "found=no; for n; do if [ -e /dev/fd/$n ]; then found=yes; fi; done\n",
            "cat >/dev/null; if [ $found = yes ]; then exit 4; fi; exit 3"
        );
        let engine = WorkerEngine::new("sh", ["-c", script, "sh", &read, &write]);
        let RunningJob { mut events, .. } = run(&engine).await;
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            }))
        );
    }

    /// A worker waits for the spawn gate off the runtime's threads: a start
    /// whose `exec` stalls holds the gate, and a run that waited for it on a
    /// runtime thread would stop every other task of that thread meanwhile.
    /// Here the gate is let go by a task of the same one-thread runtime, so
    /// it can be only while the run waits elsewhere.
    #[tokio::test]
    async fn a_worker_waits_for_the_gate_off_the_runtime() {
        let (release, stalled) = testkit::inheritable::stalled_start(Duration::from_secs(2));
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; exit 3"]);
        let let_go = async {
            tokio::task::yield_now().await;
            let _ = release.send(());
        };
        let (RunningJob { mut events, .. }, ()) = tokio::join!(run(&engine), let_go);
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            }))
        );
        assert!(
            stalled.join().unwrap(),
            "the run waited for the gate on the runtime's thread"
        );
    }

    /// The proxy variables pass the worker's scrubbed environment, so a
    /// program the worker starts can use them, and what a proxy answers can
    /// echo them: on stderr and in an event.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_credentials_of_a_proxy_url_are_masked_in_stderr_and_in_events() {
        capture_the_worker_log();
        let warning = WorkerEvent::Warning {
            item: None,
            during: "connecting".to_owned(),
            code: WarningCode::Unknown,
            detail: "proxy asked for CANARY-proxy".to_owned(),
        };
        let failure = failed_saying(
            ErrorCode::OperationFailed,
            "refused by http://demo:CANARY-proxy@proxy.demo:3128",
        );
        let events = answered(
            fake_job(),
            behind_a_proxy(),
            "proxy echo: http://demo:CANARY-proxy@proxy.demo:3128",
            &[line_of(&warning), line_of(&failure)],
        )
        .await;
        assert_eq!(
            events,
            [
                EngineEvent::Warning(Warning {
                    kind: WarningKind::Other,
                    item: None,
                    during: "connecting".to_owned(),
                    detail: "proxy asked for ***".to_owned(),
                }),
                EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "refused by http://***@proxy.demo:3128".to_owned()
                }),
            ]
        );
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("proxy echo:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "proxy echo: http://***@proxy.demo:3128");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_jobs_own_secrets_and_the_proxy_credentials_are_both_masked() {
        capture_the_worker_log();
        let failure = failed_saying(
            ErrorCode::OperationFailed,
            "hunter2-demo then CANARY-proxy then CANARY-resolved",
        );
        let events = answered(
            identify("hunter2-demo", &["CANARY-resolved"]),
            behind_a_proxy(),
            "both: hunter2-demo CANARY-proxy CANARY-resolved",
            &[line_of(&failure)],
        )
        .await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineFailed {
                detail: "*** then *** then ***".to_owned()
            })]
        );
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("both:"))
            .await
            .expect("the worker's stderr never reached the log");
        assert_eq!(line, "both: *** *** ***");
    }

    /// What a stand-in worker saw, written to files: its environment, its
    /// arguments and its stdin.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_starts_with_a_scrubbed_environment_and_still_gets_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let script = format!(
            "cat > '{dir}/spec.json'; env > '{dir}/env.txt'; echo \"$0 $*\" > '{dir}/args.txt'; \
             echo '{result}'",
            dir = dir.path().display(),
            result = result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_environment([
            (OsString::from("PATH"), path),
            (OsString::from("HOME"), OsString::from("/home/demo")),
            (
                OsString::from("AWS_SECRET_ACCESS_KEY"),
                OsString::from("CANARY-aws"),
            ),
            (
                OsString::from("RUSTIC_PASSWORD"),
                OsString::from("CANARY-rustic"),
            ),
            (OsString::from("DEMO_B2_KEY"), OsString::from("CANARY-b2")),
        ]);
        let job = identify("demo-password", &[]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, job)
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        let env = read("env.txt");
        assert!(env.contains("HOME=/home/demo"), "HOME is missing: {env}");
        assert!(env.lines().any(|line| line.starts_with("PATH=")), "{env}");
        for leaked in [
            "AWS_SECRET_ACCESS_KEY",
            "RUSTIC_PASSWORD",
            "DEMO_B2_KEY",
            "CANARY",
        ] {
            assert!(!env.contains(leaked), "{leaked} reached the worker: {env}");
        }
        let spec: serde_json::Value = serde_json::from_str(&read("spec.json")).unwrap();
        assert_eq!(spec["job"]["password"], "demo-password");
        assert!(!read("args.txt").contains("demo-password"));
    }

    /// A hot/cold repository: each half has a secret of its own.
    fn hot_cold_access() -> RepositoryAccess {
        let options = |pairs: [(&str, &str); 2]| -> BTreeMap<String, String> {
            pairs
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect()
        };
        RepositoryAccess {
            repository: "opendal:azblob".to_owned(),
            repo_hot: Some("rest:https://demo:HOT-URL-PASSWORD@hot.example:8000/hot".to_owned()),
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: options([
                ("secret_access_key", "HOT-ACCESS-KEY"),
                ("region", "hot-region"),
            ]),
            options_cold: options([
                ("sas_token", "COLD-SAS-TOKEN"),
                ("container", "cold-container"),
            ]),
            secrets: Vec::new(),
            id: None,
            entry: None,
        }
    }

    /// A worker can only open both halves of a repository if its job says
    /// what they are.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_is_given_the_hot_repository_and_the_hot_and_cold_options_of_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "cat > '{dir}/spec.json'; echo '{result}'",
            dir = dir.path().display(),
            result = result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let access = hot_cold_access();
        let events = engine
            .run(op, Job::Identify(access.clone()))
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let spec: JobSpec =
            serde_json::from_slice(&std::fs::read(dir.path().join("spec.json")).unwrap()).unwrap();
        let JobKind::Identify(repository) = spec.job else {
            panic!("the worker was not given an identify job: {:?}", spec.job);
        };
        assert_eq!(repository.repository, access.repository);
        assert_eq!(repository.repo_hot, access.repo_hot);
        assert_eq!(repository.options_hot, access.options_hot);
        assert_eq!(repository.options_cold, access.options_cold);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_secrets_of_both_halves_are_masked_in_what_the_worker_writes_to_stderr() {
        capture_the_worker_log();
        let script = format!(
            "cat >/dev/null; \
             echo 'redaction two: HOT-URL-PASSWORD HOT-ACCESS-KEY COLD-SAS-TOKEN cold-container' >&2; \
             echo '{}'",
            result_line()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let op = OperationId::from_uuid(Uuid::from_u128(1));
        let events = engine
            .run(op, Job::Identify(hot_cold_access()))
            .await
            .unwrap()
            .events
            .collect::<Vec<_>>()
            .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let line = logged_soon(log::Level::Warn, |text| text.starts_with("redaction two:"))
            .await
            .expect("the worker's stderr never reached the log");
        // Only what is a secret goes; the name of the cold container shows.
        assert_eq!(line, "redaction two: *** *** *** cold-container");
    }

    #[tokio::test]
    async fn a_job_spec_past_the_cap_is_refused_before_a_worker_starts() {
        let job = Job::Fake(FakeJob {
            warnings: vec![domain::execution::Warning {
                kind: domain::execution::WarningKind::Other,
                item: None,
                during: "backup".to_owned(),
                detail: "x".repeat(2 * MAX_MESSAGE_BYTES),
            }],
            steps: 0,
            step_delay: Duration::ZERO,
            end: FakeEnd::Succeed,
            deadline_budget: None,
        });
        // A program that can't start, so only the check before spawning can
        // produce the cap's message.
        let engine = WorkerEngine::new("/nonexistent/ladderback-worker", ["worker"]);
        let error = engine
            .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
            .await
            .err()
            .expect("a spec past the cap is refused");
        assert_matches!(
            &error,
            EngineError::Unavailable { detail } if detail.contains(&MAX_MESSAGE_BYTES.to_string()),
            "the error should name the cap: {error:?}"
        );
    }

    /// The job's redactor masks its secrets and the proxy credentials of the
    /// worker's environment. Up to the cap a worker is started; past it none
    /// is, since its redactor would mask every text whole (I4, technical
    /// design §10.2): the refusal is typed, with the cap and the proxy
    /// variable whose password took the secrets past it, or no setting when
    /// the job's own secrets are past it.
    #[tokio::test]
    async fn a_job_whose_secrets_and_proxy_credentials_pass_the_cap_is_refused_before_a_worker_starts()
     {
        use domain::secrets::{MAX_MASKED_BYTES, MAX_SECRET_BYTES};
        let password = "hunter2-demo";
        let of = |total: usize| -> Vec<String> {
            let count = total.div_ceil(MAX_SECRET_BYTES);
            (0..count)
                .map(|at| {
                    let letter = char::from(b'a' + u8::try_from(at).unwrap());
                    letter
                        .to_string()
                        .repeat(total / count + usize::from(at < total % count))
                })
                .collect()
        };
        // A program that can't start: only the check before spawning
        // refuses with the cap.
        let start = |secrets: Vec<String>, environment: Vec<(&'static str, &'static str)>| {
            let engine = WorkerEngine::new("/nonexistent/ladderback-worker", ["worker"])
                .with_environment(environment);
            async move {
                let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
                engine
                    .run(
                        OperationId::from_uuid(Uuid::from_u128(1)),
                        identify(password, &secrets),
                    )
                    .await
                    .err()
                    .expect("the program can't start")
            }
        };
        let rest = MAX_MASKED_BYTES - password.len();
        let at_the_cap = start(of(rest), vec![("NO_PROXY", "demo:p")]).await;
        assert_matches!(
            &at_the_cap,
            EngineError::Unavailable { .. },
            "a worker should have been started: {at_the_cap:?}"
        );
        let past = start(
            of(rest),
            vec![
                ("HTTP_PROXY", "http://proxy.demo:3128"),
                ("HTTPS_PROXY", "http://demo:p@proxy.demo:3128"),
            ],
        )
        .await;
        assert_eq!(
            past,
            EngineError::SecretsTooLargeToMask {
                field: Some("HTTPS_PROXY".to_owned()),
                limit: MAX_MASKED_BYTES,
            }
        );
        let own = start(of(rest + 1), Vec::new()).await;
        assert_eq!(
            own,
            EngineError::SecretsTooLargeToMask {
                field: None,
                limit: MAX_MASKED_BYTES,
            }
        );
    }

    /// The daemon writes a worker its job before anything else, and a worker
    /// that never reads it leaves that write blocked once the pipe is full.
    /// The start timeout ends the wait, and the worker with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_never_reads_its_job_is_killed_when_the_start_timeout_passes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!("echo $$ >'{}'; exec sleep 30", pid_file.display());
        // Long enough for a loaded machine to start the shell, which the
        // kill must not beat to its first command.
        let timeout = Duration::from_secs(1);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(timeout);
        let began = Instant::now();
        let answer = run_within(Duration::from_secs(5), &engine, job_larger_than_a_pipe()).await;
        let took = began.elapsed();
        assert_eq!(answer.err(), Some(EngineError::StartTimedOut));
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "gave up before the start timeout: {took:?}"
        );
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the worker that never read its job outlived the start timeout"
        );
    }

    /// The start timeout bounds the write, not the worker's pace: one that
    /// takes a while to start reading, but reads in time, runs normally.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_reads_its_job_late_but_within_the_start_timeout_still_runs() {
        let script = format!("sleep 0.3; cat >/dev/null; echo '{}'", result_line());
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(5));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(15), &engine, job_larger_than_a_pipe())
            .await
            .expect("a worker that read its job in time was timed out");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_eq!(
            events,
            [EngineEvent::Succeeded(EngineResult::Fake {
                steps_completed: 0
            })]
        );
    }

    /// A write that fails, here to a worker that is gone, is not a timeout.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_is_gone_before_it_reads_its_job_is_unavailable_not_timed_out() {
        let engine =
            WorkerEngine::new("sh", ["-c", "exit 0"]).with_start_timeout(Duration::from_secs(30));
        let answer = run_within(Duration::from_secs(10), &engine, job_larger_than_a_pipe()).await;
        let error = answer.err();
        assert_matches!(error, Some(EngineError::Unavailable { .. }));
    }

    /// What the worker started is killed with it, as at every other end of a
    /// worker: dropping the worker alone would leave an rclone running.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_also_kills_the_processes_the_worker_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        let script = format!(
            "sleep 30 & echo $! >'{}'; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(1));
        let answer = run_within(Duration::from_secs(5), &engine, job_larger_than_a_pipe()).await;
        assert_eq!(answer.err(), Some(EngineError::StartTimedOut));
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the start timeout"
        );
    }

    /// A worker that timed out at start never took its job, and the daemon's
    /// own record of its cache slot must let go too: the next worker of the
    /// repository gets the same slot, and not one more, until the slots run
    /// out and workers run without a cache.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_cache_slot_of_a_worker_that_timed_out_at_start_is_free_for_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let first_ran = dir.path().join("first-worker-ran");
        let spec_file = dir.path().join("spec.json");
        // The first worker never reads its job. The one after it keeps the
        // job it is given, and reports a result.
        let script = format!(
            "if [ -e '{first}' ]; then cat >'{spec}'; echo '{result}'; \
             else touch '{first}'; exec sleep 30; fi",
            first = first_ran.display(),
            spec = spec_file.display(),
            result = result_line()
        );
        let cache_root = dir.path().join("cache");
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_cache_root(&cache_root)
            .with_start_timeout(Duration::from_secs(1));
        let id = repository("ab");
        let identify = |repository: String| {
            Job::Identify(RepositoryAccess {
                repository,
                repo_hot: None,
                password: SecretValue::new("demo"),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
                secrets: Vec::new(),
                id: Some(id.clone()),
                entry: None,
            })
        };

        let timed_out = run_within(
            Duration::from_secs(5),
            &engine,
            identify("x".repeat(900_000)),
        )
        .await;
        assert_eq!(timed_out.err(), Some(EngineError::StartTimedOut));

        let RunningJob {
            events,
            control: _control,
        } = run_within(
            Duration::from_secs(5),
            &engine,
            identify("/home/demo/repo".to_owned()),
        )
        .await
        .expect("the job after the timed out one was refused");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)], "{events:?}");
        let spec: JobSpec = serde_json::from_slice(&std::fs::read(&spec_file).unwrap()).unwrap();
        assert_eq!(
            spec.cache_dir,
            Some(cache_dir(&cache_root, &id, 0)),
            "the slot of the worker that timed out stayed taken"
        );
    }

    /// A probe opens with no cache (decision 80): it takes no slot, even for
    /// an access that names the repository, and its worker gets the access
    /// it probes.
    #[test]
    fn a_probe_takes_no_cache_slot_and_carries_its_location() {
        let access = RepositoryAccess {
            repository: "/srv/lan".to_owned(),
            repo_hot: None,
            password: SecretValue::new("demo"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: Some(RepositoryId::parse(&"ab".repeat(32)).unwrap()),
            entry: None,
        };
        let probe = Job::Probe(access.clone());
        assert_eq!(repository_id(&probe), None);
        assert_eq!(access_of(&probe), Some(&access));
        let Ok(JobKind::Probe(spec)) = job_kind(probe) else {
            panic!("a probe is a probe job");
        };
        assert_eq!(spec.repository, "/srv/lan");
    }
}
