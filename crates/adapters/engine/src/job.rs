//! The worker protocol. Only the daemon and the worker speak it, and both
//! link this crate, so it isn't part of the published `api`.
//!
//! The daemon writes one [`JobSpec`] as JSON to the worker's stdin and closes
//! it. The worker answers with one [`WorkerEvent`] per line on stdout, and
//! ends the stream with a [`WorkerEvent::Result`] or a [`WorkerEvent::Failed`].
//! The job carries the repository password and every secret its settings
//! resolved to, so it travels over stdin only: never as an argument, in the
//! environment or in a file (invariant I4).

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use api::{ErrorCode, MsgArg, WarningCode};
use domain::planning::masked_options;
use domain::secrets::{REDACTED, Redactor, RepositorySecrets, SecretValue};
use domain::{AgentId, OperationId, PlanId, RepositoryId, SignedDuration, SnapshotId, Timestamp};
use serde::{Deserialize, Serialize};

/// Exit codes of a worker beyond 0 (the job ended and reported its result)
/// and 1 (the job failed and said why).
pub mod exit_code {
    /// The deadline watchdog stopped the job once its deadline had passed.
    pub const DEADLINE_PASSED: i32 = 75;
    /// SIGTERM stopped the worker; the shell convention for that is 128 + 15.
    pub const TERMINATED: i32 = 143;
    /// The event stream could not be written, because the daemon is gone
    /// (sysexits' `EX_IOERR`).
    pub const EVENTS_UNWRITABLE: i32 = 74;
}

/// The longest message either side accepts: the job spec on the worker's
/// stdin, and each event line on its stdout. Bytes from another process
/// never size an allocation unbounded (technical design §13.5), and a
/// message past the cap fails the job.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// How much of one message to read: one byte past the cap, so a message of
/// exactly [`MAX_MESSAGE_BYTES`] is told from one that is too long.
pub(crate) const MAX_MESSAGE_READ: u64 = MAX_MESSAGE_BYTES as u64 + 1;
/// The one job a worker runs, read from its stdin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    pub op_id: OperationId,
    /// Where rustic keeps its cache for this worker; `None` runs without one.
    pub cache_dir: Option<PathBuf>,
    /// The longest the deadline watchdog sleeps between two looks at the
    /// wall clock. A machine that was suspended notices a passed deadline
    /// within this time of waking.
    pub watchdog_interval_ms: u64,
    pub job: JobKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobKind {
    Fake(FakeJobSpec),
    Identify(RepositorySpec),
    /// Initialises a repository where nothing is yet, with default key
    /// options. It refuses when one already exists there.
    Init(RepositorySpec),
    Backup(BackupJobSpec),
    /// Removes exactly the snapshots the job names: the selection was made
    /// by the owner before the job ran (I6).
    Forget(ForgetJobSpec),
    /// Rewrites exactly the snapshots the job names with the tags given:
    /// new snapshot files over the same trees, the originals removed. The
    /// selection was made by the owner before the job ran (§7.3, step 3).
    Rewrite(RewriteJobSpec),
    Prune(PruneJobSpec),
    Check(CheckJobSpec),
    /// Lists the repository's snapshots. A read, never an operation.
    Ls(RepositorySpec),
}

/// A repository and what it takes to open it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySpec {
    /// rustic's `repository` string: a path, `rest:…`, `opendal:…`, `rclone:…`.
    /// Next to a `repo_hot`, it is the cold half.
    pub repository: String,
    /// rustic's `repo-hot`: the hot half of a hot/cold repository.
    pub repo_hot: Option<String>,
    #[serde(with = "secret")]
    pub password: SecretValue,
    /// Backend options, as rustic's `[repository.options]`: both halves get
    /// them.
    pub options: BTreeMap<String, String>,
    /// rustic's `options-hot`: the options of the hot half only.
    pub options_hot: BTreeMap<String, String>,
    /// rustic's `options-cold`: the options of the cold half only.
    pub options_cold: BTreeMap<String, String>,
    /// What the `${…}` references in the repository's settings resolved to
    /// (the password is not among them). They are masked wherever they show,
    /// whatever the option they sit in is called.
    #[serde(with = "secrets")]
    pub secrets: Vec<SecretValue>,
}

impl RepositorySpec {
    /// What hides this repository's secrets wherever the job reports text.
    pub(crate) fn redactor(&self) -> Redactor {
        Redactor::for_repository(RepositorySecrets {
            repository: &self.repository,
            repo_hot: self.repo_hot.as_deref(),
            password: &self.password,
            options: &self.options,
            options_hot: &self.options_hot,
            options_cold: &self.options_cold,
            resolved: &self.secrets,
        })
    }
}

/// Everything that can hold a secret is masked: the password, the password
/// in the URL of either half, every secret the job carries, and the value of
/// every option of the three sets (`secret_access_key` and the like). What
/// helps with diagnosing stays: the rest of the URLs and the options' names.
/// The secrets show as one mask each, so how many there are is visible and
/// what they are is not.
impl fmt::Debug for RepositorySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redactor = self.redactor();
        let repository = redactor.redact(&self.repository);
        let repo_hot = self.repo_hot.as_deref().map(|hot| redactor.redact(hot));
        let secrets: Vec<&str> = self.secrets.iter().map(|_| REDACTED).collect();
        f.debug_struct("RepositorySpec")
            .field("repository", &repository)
            .field("repo_hot", &repo_hot)
            .field("password", &self.password)
            .field("options", &masked_options(&self.options))
            .field("options_hot", &masked_options(&self.options_hot))
            .field("options_cold", &masked_options(&self.options_cold))
            .field("secrets", &secrets)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupJobSpec {
    pub repo: RepositorySpec,
    pub sources: Vec<String>,
    /// With `agent`, the plan tags and the parent choice (§7.3).
    pub plan: PlanId,
    pub agent: AgentId,
    pub extra_tags: Vec<String>,
    pub label: Option<String>,
    /// Counted from the start of the index read; once it passes, the worker
    /// stops (I3).
    pub deadline_budget: SignedDuration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgetJobSpec {
    pub repo: RepositorySpec,
    /// The snapshots to remove, each a hex snapshot ID.
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteJobSpec {
    pub repo: RepositorySpec,
    /// The snapshots to rewrite, each a hex snapshot ID.
    pub ids: Vec<String>,
    /// The `ladderback-*` tags the rewritten snapshots carry instead of the
    /// ones they had. The user's own tags stay.
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneJobSpec {
    pub repo: RepositorySpec,
    pub keep_delete: SignedDuration,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckJobSpec {
    pub repo: RepositorySpec,
    pub read_data: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeJobSpec {
    pub steps: u32,
    pub step_delay_ms: u64,
    pub warnings: Vec<FakeWarning>,
    pub end: FakeEndSpec,
    /// Counted from the start of the job.
    pub deadline_budget: Option<SignedDuration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeWarning {
    pub code: WarningCode,
    pub item: Option<String>,
    pub during: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum FakeEndSpec {
    Succeed,
    Fail { detail: String },
    Exit { after_steps: u32, exit_code: u8 },
}

/// One line of the worker's stdout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    /// The job began its work. Nothing but a log line precedes it, and
    /// `index_read_at` is when the worker started reading the repository
    /// index, which is what a deadline counts from.
    Started {
        op_id: OperationId,
        pid: u32,
        index_read_at: Timestamp,
    },
    ProgressStart {
        key: u32,
        kind: ProgressKind,
        prefix: String,
    },
    ProgressLength {
        key: u32,
        len: u64,
    },
    /// Increments are coalesced: a few messages a second per key at most.
    ProgressInc {
        key: u32,
        inc: u64,
    },
    ProgressTitle {
        key: u32,
        title: String,
    },
    ProgressFinish {
        key: u32,
    },
    Warning {
        item: Option<String>,
        during: String,
        code: WarningCode,
        detail: String,
    },
    /// Never localized (§8.4).
    Log {
        level: Level,
        message: String,
    },
    Result(OpResult),
    Failed {
        code: ErrorCode,
        args: Vec<(String, MsgArg)>,
        detail: Option<String>,
    },
}

impl WorkerEvent {
    /// The last line of a stream: after it the worker writes nothing more.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Result(_) | Self::Failed { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressKind {
    Spinner,
    Counter,
    Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OpResult {
    Fake {
        steps_completed: u32,
    },
    Repository {
        id: RepositoryId,
    },
    Backup(BackupResult),
    Forget(ForgetResult),
    Rewrite(RewriteResult),
    Prune(PruneResult),
    Check(CheckResult),
    /// A repository's snapshots, newest first.
    Snapshots {
        snapshots: Vec<SnapshotResult>,
    },
}

/// One snapshot of a listing, as the worker reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResult {
    pub id: SnapshotId,
    pub time: Timestamp,
    pub hostname: String,
    pub label: String,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    /// The snapshot has more paths than `paths` holds, or one was cut short.
    #[serde(default)]
    pub paths_truncated: bool,
    /// The snapshot has more tags than `tags` holds, or one was cut short.
    #[serde(default)]
    pub tags_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupResult {
    pub repository: RepositoryId,
    pub snapshot: SnapshotId,
    /// The parent rustic recorded in the snapshot, not the one asked for.
    pub parent: Option<SnapshotId>,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub data_added: u64,
    pub data_added_packed: u64,
    pub error_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneResult {
    pub repository: RepositoryId,
    pub packs_marked: u64,
    pub packs_deleted: u64,
    pub packs_recovered: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgetResult {
    pub repository: RepositoryId,
    /// The snapshots removed, as the job was asked to remove them.
    pub removed: Vec<SnapshotId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteResult {
    pub repository: RepositoryId,
    /// The snapshots the rewrite created, as new IDs over the same trees.
    pub created: Vec<SnapshotId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub repository: RepositoryId,
    pub errors: u64,
    pub warnings: u64,
}

/// A secret travels as the plain string it is: the protocol is the one place
/// that must, and its only reader is the worker's stdin.
mod secret {
    use domain::secrets::SecretValue;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        secret: &SecretValue,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(secret.expose())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SecretValue, D::Error> {
        String::deserialize(deserializer).map(SecretValue::new)
    }
}

/// A list of secrets, each a plain string like [`secret`].
mod secrets {
    use domain::secrets::SecretValue;
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        secrets: &[SecretValue],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(secrets.len()))?;
        for secret in secrets {
            seq.serialize_element(secret.expose())?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<SecretValue>, D::Error> {
        Vec::<String>::deserialize(deserializer)
            .map(|values| values.into_iter().map(SecretValue::new).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> RepositorySpec {
        RepositorySpec {
            repository: "/backups/home".to_owned(),
            repo_hot: None,
            password: SecretValue::new("hunter2"),
            options: BTreeMap::from([("retry".to_owned(), "3".to_owned())]),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
        }
    }

    fn spec(job: JobKind) -> JobSpec {
        JobSpec {
            op_id: OperationId::from_uuid(domain::Uuid::from_u128(7)),
            cache_dir: Some(PathBuf::from("/cache/ladderback/abc/0")),
            watchdog_interval_ms: 5_000,
            job,
        }
    }

    fn round_trip(spec: &JobSpec) -> JobSpec {
        serde_json::from_slice(&serde_json::to_vec(spec).unwrap()).unwrap()
    }

    #[test]
    fn every_kind_of_job_round_trips() {
        let backup = BackupJobSpec {
            repo: repository(),
            sources: vec!["/home/demo".to_owned()],
            plan: PlanId::parse("home").unwrap(),
            agent: AgentId::from_uuid(domain::Uuid::from_u128(1)),
            extra_tags: vec!["weekly".to_owned()],
            label: Some("laptop".to_owned()),
            deadline_budget: SignedDuration::from_hours(6),
        };
        for job in [
            JobKind::Identify(repository()),
            JobKind::Backup(backup),
            JobKind::Prune(PruneJobSpec {
                repo: repository(),
                keep_delete: SignedDuration::from_hours(144),
            }),
            JobKind::Check(CheckJobSpec {
                repo: repository(),
                read_data: true,
            }),
        ] {
            let spec = spec(job);
            assert_eq!(round_trip(&spec), spec);
        }
    }

    /// A spec without the options of the hot and cold halves is refused, not
    /// read as one with none: an old daemon's spec says nothing of a hot half,
    /// and reading it so would open a hot/cold repository as a cold-only one.
    /// A missing `repo_hot` alone is `None`, as serde reads any optional field.
    #[test]
    fn a_spec_without_the_hot_and_cold_fields_is_refused() {
        let mut json = serde_json::to_value(spec(JobKind::Identify(repository()))).unwrap();
        let job = json["job"].as_object_mut().unwrap();
        for field in ["repo_hot", "options_hot", "options_cold"] {
            assert!(job.remove(field).is_some(), "{field} is not in the spec");
        }
        assert!(serde_json::from_value::<JobSpec>(json).is_err());
    }

    #[test]
    fn a_deadline_budget_keeps_zero_negative_and_fractions_of_a_second() {
        for budget in [
            SignedDuration::ZERO,
            SignedDuration::from_hours(-3),
            SignedDuration::from_millis(15),
            SignedDuration::from_secs(1) + SignedDuration::from_millis(500),
        ] {
            let job = FakeJobSpec {
                steps: 1,
                step_delay_ms: 0,
                warnings: Vec::new(),
                end: FakeEndSpec::Succeed,
                deadline_budget: Some(budget),
            };
            let spec = spec(JobKind::Fake(job));
            assert_eq!(round_trip(&spec), spec, "budget {budget:?}");
        }
    }

    #[test]
    fn the_password_travels_as_a_plain_string_and_stays_out_of_debug() {
        let spec = spec(JobKind::Identify(repository()));
        let json: serde_json::Value = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["job"]["password"], "hunter2");
        assert!(!format!("{spec:?}").contains("hunter2"));
    }

    #[test]
    fn debug_output_hides_the_password_the_url_credentials_and_the_option_values() {
        let spec = spec(JobKind::Backup(BackupJobSpec {
            repo: RepositorySpec {
                repository: "rest:https://demo:s3cr3t@nas:8000/backups".to_owned(),
                repo_hot: None,
                password: SecretValue::new("hunter2"),
                options: BTreeMap::from([
                    ("secret_access_key".to_owned(), "AKIAEXAMPLEKEY".to_owned()),
                    ("retry".to_owned(), "3".to_owned()),
                ]),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
                secrets: Vec::new(),
            },
            sources: vec!["/home/demo".to_owned()],
            plan: PlanId::parse("home").unwrap(),
            agent: AgentId::from_uuid(domain::Uuid::from_u128(1)),
            extra_tags: Vec::new(),
            label: None,
            deadline_budget: SignedDuration::from_hours(6),
        }));
        let text = format!("{spec:?}");
        for secret in ["hunter2", "s3cr3t", "AKIAEXAMPLEKEY"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        // What helps with diagnosing stays.
        for kept in ["nas:8000", "secret_access_key", "retry", "/home/demo"] {
            assert!(text.contains(kept), "{kept} is missing: {text}");
        }
    }

    #[test]
    fn a_spec_with_secrets_round_trips_and_they_travel_as_plain_strings() {
        let mut repo = repository();
        repo.secrets = vec![SecretValue::new("tok-1"), SecretValue::new("tok-2")];
        let spec = spec(JobKind::Identify(repo));
        assert_eq!(round_trip(&spec), spec);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            json["job"]["secrets"],
            serde_json::json!(["tok-1", "tok-2"])
        );
    }

    #[test]
    fn debug_output_hides_the_secrets_but_counts_them() {
        let mut repo = repository();
        repo.repository = "rest:https://nas:8000/CANARY-ref/backups".to_owned();
        repo.secrets = vec![
            SecretValue::new("CANARY-ref"),
            SecretValue::new("CANARY-two"),
        ];
        let text = format!("{:?}", spec(JobKind::Identify(repo)));
        for secret in ["CANARY-ref", "CANARY-two"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert!(
            text.contains("nas:8000") && text.contains("backups"),
            "{text}"
        );
        assert!(text.contains(r#"secrets: ["***", "***"]"#), "{text}");
    }

    #[test]
    fn debug_output_hides_the_secrets_of_the_hot_and_cold_halves_and_keeps_what_names_them() {
        let mut repo = repository();
        repo.repo_hot = Some("rest:https://demo:hot-s3cr3t@hot.example:8000/hot".to_owned());
        repo.options_hot = BTreeMap::from([
            ("secret_access_key".to_owned(), "AKIA-HOT-KEY".to_owned()),
            ("region".to_owned(), "eu-hot-1".to_owned()),
        ]);
        repo.options_cold = BTreeMap::from([
            ("sas_token".to_owned(), "sv=COLD-TOKEN".to_owned()),
            ("region".to_owned(), "eu-cold-1".to_owned()),
        ]);
        repo.secrets = vec![SecretValue::new("canary-in-the-hot-url")];
        repo.repo_hot = repo
            .repo_hot
            .map(|hot| format!("{hot}/canary-in-the-hot-url"));
        let text = format!("{:?}", spec(JobKind::Identify(repo)));
        for secret in [
            "hot-s3cr3t",
            "AKIA-HOT-KEY",
            "sv=COLD-TOKEN",
            "eu-hot-1",
            "eu-cold-1",
            "canary-in-the-hot-url",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        for kept in [
            "rest:https://demo:",
            "@hot.example:8000/hot",
            "options_hot",
            "options_cold",
            "secret_access_key",
            "sas_token",
            "region",
        ] {
            assert!(text.contains(kept), "{kept} is gone: {text}");
        }
    }

    #[test]
    fn a_result_names_its_operation() {
        let result = WorkerEvent::Result(OpResult::Prune(PruneResult {
            repository: RepositoryId::parse(&"ab".repeat(32)).unwrap(),
            packs_marked: 3,
            packs_deleted: 2,
            packs_recovered: 1,
        }));
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["event"], "result");
        assert_eq!(json["op"], "prune");
        assert_eq!(serde_json::from_value::<WorkerEvent>(json).unwrap(), result);
    }

    #[test]
    fn only_a_result_or_a_failure_ends_the_stream() {
        let failed = WorkerEvent::Failed {
            code: ErrorCode::OperationFailed,
            args: Vec::new(),
            detail: None,
        };
        let result = WorkerEvent::Result(OpResult::Fake { steps_completed: 0 });
        let log = WorkerEvent::Log {
            level: Level::Info,
            message: String::new(),
        };
        assert!(failed.is_final());
        assert!(result.is_final());
        assert!(!log.is_final());
    }
}
