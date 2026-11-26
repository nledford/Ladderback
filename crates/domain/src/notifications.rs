//! Notifications (technical design §11): **rules** over the domain events,
//! each sending the events it names to one **channel**, in the words of its
//! **template** or in the default text. A generic subdomain: it reads what
//! Execution reports and decides nothing about it.
//!
//! The rules here are pure: which events exist and what they are called,
//! which rule an event matches, what a template may say and what it renders
//! to. Resolving a channel's secrets and sending are `app`'s, through the
//! `SecretResolver` and `Notifier` ports. Nothing a rule, a channel or a
//! template holds can put a secret into a message (I4): a channel's address
//! and token are [`SecretRef`]s, never text of the configuration, and a
//! template reads only the facts of an [`Event`].

mod template;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;

pub use template::{MAX_TEMPLATE_BYTES, Template};

use crate::execution::{FailureReason, OperationKind, OperationStatus, Target, WarningKind};
use crate::fleet::Hostname;
use crate::planning::{InvalidValue, ValueKind, Violation, is_relative_program};
use crate::secrets::SecretRef;
use crate::{OperationId, PlanId, RepositoryAlias};

/// What happened, as a rule names it. Each is one of the events §11 lists
/// that the agent emits today; an event that a later part of the agent
/// emits (a missed backup) becomes a variant here, and every rule, template
/// and channel takes it with no other change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    /// A backup started running.
    BackupStarted,
    /// A backup ended well, with no warning.
    BackupSucceeded,
    /// A backup ended well, with warnings: files it couldn't read, a hook
    /// that warned. Told instead of [`Self::BackupSucceeded`], never beside it.
    BackupWarning,
    /// A backup failed.
    BackupFailed,
    /// A backup failed, and the agent retries it: the scheduler planned
    /// one of the plan's retries (§5, decision 76).
    BackupRetrying,
    ForgetFailed,
    PruneFailed,
    CheckFailed,
    /// A restore of a snapshot's entry failed (§10.4): its success and its
    /// start are not told.
    RestoreFailed,
    /// A paired machine's maintenance of a repository has been paused for
    /// want of a lease from its hub for 14 days (§7.3): told once per
    /// pause.
    RepoMaintenancePaused,
    /// An approved machine has been offline for 15 minutes, as its hub
    /// sees it (§7.2; §16, decision 168): told once per time it goes
    /// offline.
    MachineOffline,
    /// A machine the hub alerted offline came back: told once per alert.
    MachineOnline,
}

/// How an operation ended, as far as a notification tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    SucceededWithWarnings,
    Failed,
}

/// Which ping a healthchecks channel sends for an event: the check's own
/// URL with `/start`, as it is, or with `/fail`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ping {
    Start,
    Success,
    Fail,
}

impl EventKind {
    /// Every event, in the order rules list them.
    pub const ALL: [Self; 12] = [
        Self::BackupStarted,
        Self::BackupSucceeded,
        Self::BackupWarning,
        Self::BackupFailed,
        Self::BackupRetrying,
        Self::ForgetFailed,
        Self::PruneFailed,
        Self::CheckFailed,
        Self::RestoreFailed,
        Self::RepoMaintenancePaused,
        Self::MachineOffline,
        Self::MachineOnline,
    ];

    /// The event's name, as a rule, a template and a webhook's body say it:
    /// `backup.failed`.
    pub fn name(self) -> &'static str {
        match self {
            Self::BackupStarted => "backup.started",
            Self::BackupSucceeded => "backup.succeeded",
            Self::BackupWarning => "backup.warning",
            Self::BackupFailed => "backup.failed",
            Self::BackupRetrying => "backup.retrying",
            Self::ForgetFailed => "forget.failed",
            Self::PruneFailed => "prune.failed",
            Self::CheckFailed => "check.failed",
            Self::RestoreFailed => "restore.failed",
            Self::RepoMaintenancePaused => "repo.maintenance_paused",
            Self::MachineOffline => "machine.offline",
            Self::MachineOnline => "machine.online",
        }
    }

    /// The event `text` names, or [`ValueKind::NotificationEvent`].
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == text)
            .ok_or_else(|| InvalidValue::new(ValueKind::NotificationEvent, text))
    }

    /// The ping a healthchecks channel sends for this event.
    pub fn ping(self) -> Ping {
        match self {
            Self::BackupStarted => Ping::Start,
            Self::BackupSucceeded | Self::BackupWarning | Self::MachineOnline => Ping::Success,
            Self::BackupFailed
            | Self::BackupRetrying
            | Self::ForgetFailed
            | Self::PruneFailed
            | Self::CheckFailed
            | Self::RestoreFailed
            | Self::RepoMaintenancePaused
            | Self::MachineOffline => Ping::Fail,
        }
    }

    /// The event an operation of `kind` makes when it starts: only a backup's
    /// start is told.
    pub fn started(kind: OperationKind) -> Option<Self> {
        (kind == OperationKind::Backup).then_some(Self::BackupStarted)
    }

    /// The event an operation of `kind` makes when it ends with `outcome`. A
    /// backup tells each ending; forget, prune, check and restore only a
    /// failure.
    pub fn ended(kind: OperationKind, outcome: Outcome) -> Option<Self> {
        match (kind, outcome) {
            (OperationKind::Backup, Outcome::Succeeded) => Some(Self::BackupSucceeded),
            (OperationKind::Backup, Outcome::SucceededWithWarnings) => Some(Self::BackupWarning),
            (OperationKind::Backup, Outcome::Failed) => Some(Self::BackupFailed),
            (OperationKind::Forget, Outcome::Failed) => Some(Self::ForgetFailed),
            (OperationKind::Prune, Outcome::Failed) => Some(Self::PruneFailed),
            (OperationKind::Check, Outcome::Failed) => Some(Self::CheckFailed),
            (OperationKind::Restore, Outcome::Failed) => Some(Self::RestoreFailed),
            _ => None,
        }
    }
}

/// One thing that happened, with the facts a notification may tell: never
/// a secret, a location or a free-text detail from the engine (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    /// The operation it tells of; none for a repository's paused
    /// maintenance, which no operation is.
    pub operation: Option<OperationId>,
    /// The plan of a backup.
    pub plan: Option<PlanId>,
    /// The repository, by the alias this agent gives it.
    pub repository: Option<RepositoryAlias>,
    /// When the operation started, for a start, or ended; for a retry, when
    /// the retry runs; for paused maintenance, since when it is paused.
    pub at: Timestamp,
    /// The codes the event tells (§9): the failure's stored code
    /// (`operation-failed`), for a failure whose reason the operation
    /// records and for the failure a retry follows; for a backup that ended
    /// with warnings, the code of each kind of warning it kept
    /// (`hook-warning`), each once, in the order the first of each came, as
    /// the history names them (#508). Empty for any other event.
    pub codes: Vec<&'static str>,
    /// Which retry a `backup.retrying` tells of.
    pub attempt: Option<Attempt>,
    /// The machine, by its hostname on the hub, of a machine's event.
    pub machine: Option<Hostname>,
}

/// Which of a plan's retries a `backup.retrying` tells of: the `number`th
/// of its `of` attempts (decision 76).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub number: u32,
    pub of: u32,
}

/// The facts of an operation an event is made from: what the operation log
/// keeps of it, and the repository it ran on by this agent's alias, when the
/// agent knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject<'a> {
    pub operation: OperationId,
    pub kind: OperationKind,
    pub target: &'a Target,
    pub repository: Option<&'a RepositoryAlias>,
    pub status: &'a OperationStatus,
    /// The kinds of the warnings the log keeps of the operation, in the
    /// order the first of each came.
    pub warnings: &'a [WarningKind],
}

impl Event {
    /// The event `subject` makes when it starts, at `at`.
    pub fn started(subject: Subject<'_>, at: Timestamp) -> Option<Self> {
        let kind = EventKind::started(subject.kind)?;
        Some(Self::of(kind, subject, Vec::new(), at))
    }

    /// The event `subject` makes when it ends with `outcome`, at `at`. A
    /// failure's code is the one the operation's status records, and a
    /// failure the log hasn't recorded has none; a success with warnings
    /// tells the code of each kind of warning, each once. A backup that never
    /// ran makes no event when nothing is for the person to do: one that gave
    /// way to a newer run of its plan, whose newer run tells its own end
    /// (#1245), and one whose plan was deleted, or now backs up to another
    /// repository, while it waited for its keyring, the person's own change
    /// (#1293).
    pub fn ended(subject: Subject<'_>, outcome: Outcome, at: Timestamp) -> Option<Self> {
        if matches!(
            subject.status,
            OperationStatus::Failed {
                reason: FailureReason::Superseded
                    | FailureReason::PlanRemoved
                    | FailureReason::PlanMoved
            }
        ) {
            return None;
        }
        let kind = EventKind::ended(subject.kind, outcome)?;
        let codes = match outcome {
            Outcome::Failed => failed(subject.status),
            Outcome::SucceededWithWarnings => {
                let mut codes = Vec::new();
                for code in subject.warnings.iter().map(|warning| warning.code()) {
                    if !codes.contains(&code) {
                        codes.push(code);
                    }
                }
                codes
            }
            Outcome::Succeeded => Vec::new(),
        };
        Some(Self::of(kind, subject, codes, at))
    }

    /// What `{code}` and a webhook's `code` say: the event's codes,
    /// separated by `, `, or nothing when it has none.
    pub fn code(&self) -> Option<String> {
        (!self.codes.is_empty()).then(|| self.codes.join(", "))
    }

    /// The event a retry of `subject`, the backup that failed, makes when it
    /// is planned: its `attempt`, and `at`, when it runs. The code is the
    /// failure's, as the operation's status records it. Only a backup is
    /// retried.
    pub fn retrying(subject: Subject<'_>, attempt: Attempt, at: Timestamp) -> Option<Self> {
        (subject.kind == OperationKind::Backup).then(|| Self {
            attempt: Some(attempt),
            ..Self::of(
                EventKind::BackupRetrying,
                subject,
                failed(subject.status),
                at,
            )
        })
    }

    /// The event of a paired machine's maintenance of `repository`, by its
    /// alias here, paused for want of a lease since `since`.
    pub fn maintenance_paused(repository: RepositoryAlias, since: Timestamp) -> Self {
        Self {
            kind: EventKind::RepoMaintenancePaused,
            operation: None,
            plan: None,
            repository: Some(repository),
            at: since,
            codes: Vec::new(),
            attempt: None,
            machine: None,
        }
    }

    /// The event of the approved machine `machine` offline since `since`
    /// for [`OFFLINE_ALERT_AFTER`](crate::fleet::OFFLINE_ALERT_AFTER).
    pub fn machine_offline(machine: Hostname, since: Timestamp) -> Self {
        Self::of_machine(EventKind::MachineOffline, machine, since)
    }

    /// The event of the machine `machine`, alerted offline, back online
    /// at `at`.
    pub fn machine_online(machine: Hostname, at: Timestamp) -> Self {
        Self::of_machine(EventKind::MachineOnline, machine, at)
    }

    /// A machine's event: no operation, plan, repository or code.
    fn of_machine(kind: EventKind, machine: Hostname, at: Timestamp) -> Self {
        Self {
            kind,
            operation: None,
            plan: None,
            repository: None,
            at,
            codes: Vec::new(),
            attempt: None,
            machine: Some(machine),
        }
    }

    fn of(kind: EventKind, subject: Subject<'_>, codes: Vec<&'static str>, at: Timestamp) -> Self {
        Self {
            kind,
            operation: Some(subject.operation),
            plan: subject.target.plan.clone(),
            repository: subject.repository.cloned(),
            at,
            codes,
            attempt: None,
            machine: None,
        }
    }
}

/// Names a channel within one agent's config: the key of its
/// `[notifications.channels.<name>]` table. A slug, as a plan's ID is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelName(String);

impl ChannelName {
    pub fn parse(value: &str) -> Result<Self, InvalidValue> {
        slug(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Names a rule within one agent's config: the key of its
/// `[notifications.rules.<name>]` table. A slug, as a plan's ID is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleName(String);

impl RuleName {
    pub fn parse(value: &str) -> Result<Self, InvalidValue> {
        slug(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a channel is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    /// An HTTP POST of a JSON body to the channel's URL.
    Webhook,
    /// A message published to an ntfy topic: an HTTP POST of the text to the
    /// topic's URL.
    Ntfy,
    /// A healthchecks.io-style check: an HTTP POST to its ping URL, with
    /// `/start` or `/fail` for those pings.
    Healthchecks,
}

impl ChannelKind {
    pub const ALL: [Self; 3] = [Self::Webhook, Self::Ntfy, Self::Healthchecks];

    /// The kind's name in the configuration: `webhook`, `ntfy`,
    /// `healthchecks`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::Ntfy => "ntfy",
            Self::Healthchecks => "healthchecks",
        }
    }

    /// The kind `text` names, or [`ValueKind::ChannelKind`].
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == text)
            .ok_or_else(|| InvalidValue::new(ValueKind::ChannelKind, text))
    }

    /// Whether a channel of this kind sends credentials with a message: a
    /// webhook and ntfy do, a ping URL is its own credential.
    pub fn takes_auth(self) -> bool {
        match self {
            Self::Webhook | Self::Ntfy => true,
            Self::Healthchecks => false,
        }
    }
}

/// The name a channel signs in with by HTTP Basic authentication: not a
/// secret, so it is written in the configuration as text. At most
/// [`MAX_USERNAME_BYTES`] bytes, not empty, and with no `:` (which ends a
/// Basic name) and no control character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Username(String);

/// The longest [`Username`], in bytes.
pub const MAX_USERNAME_BYTES: usize = 128;

impl Username {
    /// The name `text` is, or [`ValueKind::ChannelUsername`].
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let plain = !text.is_empty()
            && text.len() <= MAX_USERNAME_BYTES
            && !text.chars().any(|c| c == ':' || c.is_control());
        if plain {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidValue::new(ValueKind::ChannelUsername, text))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a channel signs in: a bearer token, or a name and a password by
/// HTTP Basic authentication. The token and the password are secrets, read
/// when a notification is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auth {
    Bearer(SecretRef),
    Basic {
        username: Username,
        password: SecretRef,
    },
}

/// Where a channel sends: its kind, the secret that holds its URL, and how
/// it signs in, for a kind that does. A URL of a webhook, a topic or a
/// check is a credential of its own (whoever knows it can post to it), so
/// it is a secret too, read when a notification is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    kind: ChannelKind,
    url: SecretRef,
    auth: Option<Auth>,
}

/// Why [`Channel::new`] refused a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelError {
    /// The URL, the token or the password is written in the configuration
    /// as text. A channel names where its secrets are: a command, the
    /// keyring, a file, a variable.
    InlineSecret,
    /// Credentials for a kind that sends none.
    AuthNotTaken,
    /// The command that prints the secret starts its program by a relative
    /// path, which the daemon's working directory would decide, as a
    /// repository's `password-command` would.
    RelativeProgram(ChannelSecret),
}

/// Which of a channel's secrets: its URL, its token or its password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelSecret {
    Url,
    Token,
    Password,
}

impl Channel {
    pub fn new(
        kind: ChannelKind,
        url: SecretRef,
        auth: Option<Auth>,
    ) -> Result<Self, ChannelError> {
        let auth_secret = auth.as_ref().map(|auth| match auth {
            Auth::Bearer(token) => (ChannelSecret::Token, token),
            Auth::Basic { password, .. } => (ChannelSecret::Password, password),
        });
        let secrets = [(ChannelSecret::Url, &url)].into_iter().chain(auth_secret);
        for (which, secret) in secrets {
            if secret.inline().is_some() {
                return Err(ChannelError::InlineSecret);
            }
            if let SecretRef::Command(command) = secret
                && is_relative_program(command.program())
            {
                return Err(ChannelError::RelativeProgram(which));
            }
        }
        if auth.is_some() && !kind.takes_auth() {
            return Err(ChannelError::AuthNotTaken);
        }
        Ok(Self { kind, url, auth })
    }

    pub fn kind(&self) -> ChannelKind {
        self.kind
    }

    pub fn url(&self) -> &SecretRef {
        &self.url
    }

    pub fn auth(&self) -> Option<&Auth> {
        self.auth.as_ref()
    }
}

/// A notification rule: the events it sends, the channel it sends them to,
/// the plans it is for (every plan when it names none), and the template
/// its messages are written with (the default text when it has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    events: BTreeSet<EventKind>,
    channel: ChannelName,
    plans: BTreeSet<PlanId>,
    template: Option<Template>,
}

impl Rule {
    /// A rule sends at least one event; one that names none is
    /// [`ValueKind::NotificationEvent`].
    pub fn new(
        events: BTreeSet<EventKind>,
        channel: ChannelName,
        plans: BTreeSet<PlanId>,
        template: Option<Template>,
    ) -> Result<Self, InvalidValue> {
        if events.is_empty() {
            return Err(InvalidValue::new(ValueKind::NotificationEvent, ""));
        }
        Ok(Self {
            events,
            channel,
            plans,
            template,
        })
    }

    pub fn events(&self) -> &BTreeSet<EventKind> {
        &self.events
    }

    pub fn channel(&self) -> &ChannelName {
        &self.channel
    }

    pub fn plans(&self) -> &BTreeSet<PlanId> {
        &self.plans
    }

    pub fn template(&self) -> Option<&Template> {
        self.template.as_ref()
    }

    /// Whether the rule sends `event`: it names the event, and names no plan
    /// or the event's plan. An event without a plan (a prune) is sent only
    /// by a rule that names none.
    pub fn matches(&self, event: &Event) -> bool {
        self.events.contains(&event.kind)
            && (self.plans.is_empty()
                || event
                    .plan
                    .as_ref()
                    .is_some_and(|plan| self.plans.contains(plan)))
    }
}

/// An agent's notification rules and the channels they send to. Every rule
/// names a channel the configuration has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notifications {
    channels: BTreeMap<ChannelName, Channel>,
    rules: BTreeMap<RuleName, Rule>,
}

/// A rule that sends an event, with the channel it sends it to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matched<'a> {
    pub rule: &'a RuleName,
    pub template: Option<&'a Template>,
    pub channel_name: &'a ChannelName,
    pub channel: &'a Channel,
}

impl Notifications {
    /// The rules and channels, unless a rule names a channel that isn't
    /// there: [`Violation::UnknownChannel`], for the first such rule in name
    /// order.
    pub fn new(
        channels: BTreeMap<ChannelName, Channel>,
        rules: BTreeMap<RuleName, Rule>,
    ) -> Result<Self, Violation> {
        let unknown = rules
            .iter()
            .find(|(_, rule)| !channels.contains_key(rule.channel()));
        if let Some((name, rule)) = unknown {
            return Err(Violation::UnknownChannel {
                rule: name.clone(),
                channel: rule.channel().clone(),
            });
        }
        Ok(Self { channels, rules })
    }

    pub fn channels(&self) -> &BTreeMap<ChannelName, Channel> {
        &self.channels
    }

    pub fn rules(&self) -> &BTreeMap<RuleName, Rule> {
        &self.rules
    }

    /// Whether there are no rules and no channels.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty() && self.rules.is_empty()
    }

    /// Each rule that sends `event`, in name order, with its channel.
    pub fn matching<'a>(&'a self, event: &'a Event) -> impl Iterator<Item = Matched<'a>> + 'a {
        self.rules.iter().filter_map(move |(name, rule)| {
            if !rule.matches(event) {
                return None;
            }
            let (channel_name, channel) = self.channels.get_key_value(rule.channel())?;
            Some(Matched {
                rule: name,
                template: rule.template(),
                channel_name,
                channel,
            })
        })
    }
}

/// The code of the failure `status` records, when it records one.
fn failed(status: &OperationStatus) -> Vec<&'static str> {
    match status {
        OperationStatus::Failed { reason } => vec![reason.code()],
        _ => Vec::new(),
    }
}

/// `value` as a channel's or a rule's name, when it is a slug.
fn slug(value: &str) -> Result<String, InvalidValue> {
    if crate::ids::is_slug(value) {
        Ok(value.to_owned())
    } else {
        Err(InvalidValue::new(ValueKind::NotificationName, value))
    }
}
