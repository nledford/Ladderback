//! A plan: what to back up, into which repository, when, and how long to keep
//! it (technical design §3.1, §6.1).
//!
//! Left out on purpose, with the `SnapshotOptions` fields of rustic they
//! would map onto: `description`, `description-from`, `time`, `delete-never`,
//! `delete-after` and `command`. Ladderback sets a snapshot's label, tags and
//! host from the plan, and the rest from the run.

use jiff::SignedDuration;

use crate::execution::plan_tags::UserTag;
use crate::{PlanId, RepositoryAlias};

use super::{
    AgentSettings, Hooks, Hostname, InvalidValue, NeverMatchingGlob, RetentionPolicy, Schedule,
    Source, SourceFilter, ValueKind, format_duration,
};

/// How a failed backup is tried again (§5): up to `attempts` more times,
/// waiting `backoff` before the first and doubling the wait each time, to at
/// most an hour. Retries stop at the next scheduled run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    attempts: u32,
    backoff: SignedDuration,
}

impl RetryPolicy {
    /// The shortest back-off: a retry that starts at once is a loop.
    pub const MIN_BACKOFF: SignedDuration = SignedDuration::from_secs(1);

    /// The longest back-off. The wait doubles to at most this (§5), so a
    /// first wait above it would already be cut.
    pub const MAX_BACKOFF: SignedDuration = SignedDuration::from_hours(1);

    /// The most attempts. A plan that never runs on a schedule has no next
    /// run to stop its retries, so the count is what ends them.
    pub const MAX_ATTEMPTS: u32 = 100;

    /// `attempts` is at least 1 and at most
    /// [`MAX_ATTEMPTS`](Self::MAX_ATTEMPTS); `backoff` is at least
    /// [`MIN_BACKOFF`](Self::MIN_BACKOFF) and at most
    /// [`MAX_BACKOFF`](Self::MAX_BACKOFF).
    pub fn new(attempts: i64, backoff: SignedDuration) -> Result<Self, InvalidValue> {
        let counted = u32::try_from(attempts)
            .ok()
            .filter(|attempts| (1..=Self::MAX_ATTEMPTS).contains(attempts))
            .ok_or_else(|| InvalidValue::new(ValueKind::Attempts, attempts.to_string()))?;
        if !(Self::MIN_BACKOFF..=Self::MAX_BACKOFF).contains(&backoff) {
            return Err(InvalidValue::new(
                ValueKind::Duration,
                format_duration(backoff),
            ));
        }
        Ok(Self {
            attempts: counted,
            backoff,
        })
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn backoff(&self) -> SignedDuration {
        self.backoff
    }
}

/// The overlap policy (§5): what a plan does when its next run comes due
/// while its last run is still going. It skips that run, the default, or
/// queues it to start once the last one ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlap {
    #[default]
    Skip,
    Queue,
}

impl Overlap {
    /// Reads `skip` or `queue`, in lower case.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        match text {
            "skip" => Ok(Self::Skip),
            "queue" => Ok(Self::Queue),
            _ => Err(InvalidValue::new(ValueKind::Overlap, text)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Queue => "queue",
        }
    }
}

/// One plan of an agent's config. Each part has been checked by its own type;
/// what only the whole configuration can check (a unique ID, a repository
/// that exists, at least one source) is checked by
/// [`AgentConfig`](super::AgentConfig).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub id: PlanId,
    /// The alias of the repository it backs up into.
    pub repository: RepositoryAlias,
    /// The host name its snapshots carry, when it isn't the agent's.
    pub host: Option<Hostname>,
    /// The label its snapshots carry.
    pub label: Option<String>,
    /// What it backs up, in the order it was written.
    pub sources: Vec<Source>,
    /// Tags its snapshots carry besides the `ladderback-*` ones, which are
    /// added when it runs.
    pub tags: Vec<UserTag>,
    /// When it runs. A plan without one runs only when asked to.
    pub schedule: Option<Schedule>,
    /// What it does when a run comes due while its last one is still going.
    pub overlap: Overlap,
    pub retry: Option<RetryPolicy>,
    pub filter: SourceFilter,
    pub retention: RetentionPolicy,
    pub hooks: Hooks,
}

impl Plan {
    /// A plan that backs up `sources` into `repository`, with nothing else
    /// set: no host, label or tag of its own, no schedule, no retry, nothing
    /// left out and nothing forgotten.
    pub fn new(id: PlanId, repository: RepositoryAlias, sources: Vec<Source>) -> Self {
        Self {
            id,
            repository,
            host: None,
            label: None,
            sources,
            tags: Vec::new(),
            schedule: None,
            overlap: Overlap::Skip,
            retry: None,
            filter: SourceFilter::default(),
            retention: RetentionPolicy::default(),
            hooks: Hooks::default(),
        }
    }

    /// The snapshot host (§3.1, §7.3): the plan's own `host`, else the
    /// agent's `hostname`. `None` when neither names one, and the engine
    /// then records the machine's own name, as rustic does.
    pub fn snapshot_host<'a>(&'a self, agent: &'a AgentSettings) -> Option<&'a Hostname> {
        self.host.as_ref().or(agent.hostname.as_ref())
    }

    /// The globs of its source filter that can never match (#510): rustic
    /// reads a glob with a slash in the middle from the root of the file
    /// system, and one written from the source reaches none of the folders
    /// the backup walks. `walked` is the plan's sources as the backup walks
    /// them, in the order written: each with its links followed where it
    /// can be resolved, as written where it can't (it isn't there yet); a
    /// `.`, a `..` and a repeated `/` in them are read here as rustic reads
    /// them. `globs`, then `iglobs`, each in the order written.
    pub fn never_matching_globs<'a>(
        &'a self,
        walked: &'a [Source],
    ) -> impl Iterator<Item = NeverMatchingGlob<'a>> {
        self.filter.never_matching(&self.sources, walked)
    }

    /// Whether it may have a glob that can never match: it has sources and
    /// a glob or iglob read from the root. Only then is its disk looked at
    /// for [`Self::never_matching_globs`].
    pub fn may_have_never_matching_globs(&self) -> bool {
        !self.sources.is_empty() && self.filter.reads_from_root()
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::planning::MAX_DURATION;

    fn id(text: &str) -> PlanId {
        PlanId::parse(text).unwrap()
    }

    fn alias(text: &str) -> RepositoryAlias {
        RepositoryAlias::parse(text).unwrap()
    }

    fn invalid(kind: ValueKind, value: &str) -> InvalidValue {
        InvalidValue::new(kind, value)
    }

    #[test]
    fn a_retry_is_at_least_one_attempt_and_at_most_100() {
        assert_eq!(RetryPolicy::MAX_ATTEMPTS, 100);
        let backoff = SignedDuration::from_mins(10);
        for attempts in [1, 3, 99, 100] {
            let retry = RetryPolicy::new(attempts, backoff).unwrap();
            assert_eq!(i64::from(retry.attempts()), attempts);
            assert_eq!(retry.backoff(), backoff);
        }
        for attempts in [
            0,
            -1,
            101,
            i64::from(u32::MAX),
            i64::from(u32::MAX) + 1,
            i64::MIN,
            i64::MAX,
        ] {
            assert_eq!(
                RetryPolicy::new(attempts, backoff),
                Err(invalid(ValueKind::Attempts, &attempts.to_string())),
                "{attempts}"
            );
        }
    }

    #[test]
    fn a_back_off_is_at_least_a_second_and_at_most_an_hour() {
        assert_eq!(RetryPolicy::MIN_BACKOFF, SignedDuration::from_secs(1));
        assert_eq!(RetryPolicy::MAX_BACKOFF, SignedDuration::from_hours(1));
        for backoff in [
            RetryPolicy::MIN_BACKOFF,
            SignedDuration::from_millis(1_001),
            SignedDuration::from_millis(3_599_999),
            RetryPolicy::MAX_BACKOFF,
        ] {
            assert_eq!(RetryPolicy::new(3, backoff).unwrap().backoff(), backoff);
        }
        for backoff in [
            SignedDuration::ZERO,
            SignedDuration::from_millis(999),
            -SignedDuration::from_mins(1),
            SignedDuration::from_millis(3_600_001),
            SignedDuration::from_hours(2),
            MAX_DURATION,
            MAX_DURATION + SignedDuration::from_secs(1),
        ] {
            assert_matches!(
                RetryPolicy::new(3, backoff),
                Err(InvalidValue {
                    kind: ValueKind::Duration,
                    ..
                }),
                "{backoff:?}"
            );
        }
    }

    #[test]
    fn a_retry_with_no_attempts_is_refused_before_its_back_off_is() {
        // Both are wrong: the first is what is reported.
        assert_eq!(
            RetryPolicy::new(0, SignedDuration::ZERO),
            Err(invalid(ValueKind::Attempts, "0"))
        );
    }

    #[test]
    fn a_plan_has_only_what_it_was_given() {
        let source = Source::parse("/home/demo").unwrap();
        let plan = Plan::new(id("home"), alias("nas"), vec![source.clone()]);
        assert_eq!(plan.id.as_str(), "home");
        assert_eq!(plan.repository.as_str(), "nas");
        assert_eq!(plan.sources, [source]);
        assert_eq!(plan.host, None);
        assert_eq!(plan.label, None);
        assert!(plan.tags.is_empty());
        assert_eq!(plan.schedule, None);
        assert_eq!(plan.retry, None);
        assert_eq!(plan.filter, SourceFilter::default());
        assert!(plan.retention.is_empty());
        assert!(plan.hooks.is_empty());
    }

    fn host(text: &str) -> Hostname {
        Hostname::parse(text).unwrap()
    }

    fn plan_with_host(host: Option<&str>) -> Plan {
        let mut plan = Plan::new(
            id("home"),
            alias("nas"),
            vec![Source::parse("/home/demo").unwrap()],
        );
        plan.host = host.map(self::host);
        plan
    }

    fn agent_with_hostname(hostname: Option<&str>) -> AgentSettings {
        AgentSettings {
            hostname: hostname.map(host),
            ..AgentSettings::default()
        }
    }

    #[test]
    fn a_plans_own_host_is_its_snapshot_host_whatever_the_agents() {
        let plan = plan_with_host(Some("demo-laptop"));
        for agent in [None, Some("demo-desktop")] {
            assert_eq!(
                plan.snapshot_host(&agent_with_hostname(agent)),
                Some(&host("demo-laptop")),
                "agent hostname {agent:?}"
            );
        }
    }

    #[test]
    fn a_plan_without_a_host_takes_the_agents_hostname() {
        let plan = plan_with_host(None);
        assert_eq!(
            plan.snapshot_host(&agent_with_hostname(Some("demo-desktop"))),
            Some(&host("demo-desktop"))
        );
    }

    #[test]
    fn without_either_there_is_no_snapshot_host_and_the_engine_chooses() {
        let plan = plan_with_host(None);
        assert_eq!(plan.snapshot_host(&agent_with_hostname(None)), None);
    }

    #[test]
    fn an_overlap_policy_is_skip_or_queue() {
        for (text, overlap) in [("skip", Overlap::Skip), ("queue", Overlap::Queue)] {
            assert_eq!(Overlap::parse(text), Ok(overlap));
            assert_eq!(overlap.as_str(), text);
        }
        for text in ["", "Skip", "QUEUE", "wait", "skip "] {
            assert_eq!(
                Overlap::parse(text),
                Err(invalid(ValueKind::Overlap, text)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_plan_skips_a_run_that_comes_due_while_its_last_is_going_unless_told_otherwise() {
        assert_eq!(Overlap::default(), Overlap::Skip);
        let plan = plan_with_host(None);
        assert_eq!(plan.overlap.as_str(), "skip");
    }
}
