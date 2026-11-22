//! Outbound ports. Object-safe, used as `Arc<dyn Port>` (technical design §3.2).

mod admin_password;
mod agent_registry;
mod api_token_store;
mod change_codec;
mod clock;
mod config_drafts;
mod config_store;
mod engine;
mod event_bus;
mod full_disk_access;
mod hooks;
mod hub_connector;
mod identity;
mod keyring;
mod notifier;
mod operation_store;
mod platform;
mod profiles;
mod proposal_store;
mod random;
mod recent_log;
mod releases;
mod retention;
mod schedule_store;
mod secrets;
mod service;
mod session_store;
mod signing;
mod sources;
mod stats_store;
mod this_machine;
mod tls_keys;
mod walk;

pub use admin_password::{AdminPassword, AdminPasswordError};
pub use agent_registry::AgentRegistry;
pub use api_token_store::{Added, ApiTokenStore, PermissionsSet};
pub use change_codec::{ChangeCodec, CodecError};
pub use clock::Clock;
pub use config_drafts::{ConfigDraft, ConfigDrafts, SavedOver};
pub use config_store::{
    CadenceProblem, ConfigError, ConfigStore, NotInPlace, PasswordProblem, Position, Problem,
    RecordError, config_unusable,
};
pub use engine::{
    BackupJob, BackupSummary, BrowseJob, CheckJob, CheckSummary, DumpAnswer, DumpHeader, DumpJob,
    Engine, EngineError, EngineEvent, EngineResult, FakeEnd, FakeJob, FindJob, FolderAnswer,
    ForgetJob, ForgetSummary, Job, JobControl, MAX_JOB_IDS, OperationStep, ProbeAnswer,
    ProgressEvent, ProgressKind, PruneJob, PrunePlanSummary, PruneSummary, RepositoryAccess,
    RestoreJob, RestorePlanSummary, RestoreSummary, RestoreTarget, RewriteJob, RewriteSummary,
    RunningJob, SavedEntry, SnapshotSummary,
};
pub use event_bus::{EVENT_BUFFER, EventBus};
pub use full_disk_access::FullDiskAccessProbe;
pub use hooks::{
    HOOK_TIMEOUT, HookContext, HookOutcome, HookOutput, HookPhase, HookRunner, HookSetup,
    HookStatus, MAX_HOOK_OUTPUT_BYTES,
};
pub use hub_connector::{HubConnector, HubLink, LinkError};
pub use identity::{Attempt, Identity, IdentityError};
pub use keyring::{Kept, Keyring, KeyringActivation, KeyringError};
pub use notifier::{
    Credentials, DELIVERY_TIMEOUT, Destination, Message, Notification, Notifier, NotifyError,
};
pub use operation_store::{
    DEFAULT_PAGE_SIZE, HistoryPage, HistoryQuery, MAX_PAGE_SIZE, OperationDetail, OperationRecord,
    OperationStore, PageSize, Recovered, Start, StoreError, maintained_repository,
};
pub use platform::{Network, NetworkLink, PlatformError, PowerSupply};
pub use profiles::{
    ExportCommands, ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileNotice,
    ProfileProblem, ProfileReader, ProfileWriter, RenderedProfile,
};
pub use proposal_store::{Admission, ProposalStore, StoredProposal};
pub use random::{Random, RandomUnavailable};
pub use recent_log::RecentLogStore;
pub use releases::{RELEASE_TIMEOUT, ReleaseFeed, ReleaseFeedError};
pub use retention::{Retention, RetentionError};
pub use schedule_store::{PendingRetry, PlannedRun, ReleaseCheck, ScheduleStore};
pub use secrets::{KeyringLock, Resolved, SecretResolver};
pub use service::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};
pub use session_store::SessionStore;
pub use signing::{Signing, SigningError};
pub use sources::{NormalizeSources, SourcesError};
pub use stats_store::StatsStore;
pub use this_machine::{MachineError, ThisMachine};
pub use tls_keys::{TlsKeyError, TlsKeys};
pub use walk::{GiveWay, WalkError, WalkLimits, WalkSources, WalkStop, Walked};
