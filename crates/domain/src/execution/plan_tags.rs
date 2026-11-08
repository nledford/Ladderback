//! Snapshot ownership: the tags that make a snapshot belong to a plan, and
//! the parent a plan's next backup builds on (technical design §7.3).

use crate::snapshot::SnapshotRef;
use crate::{AgentId, PlanId, SnapshotId};

/// The prefix of every tag Steading reserves for itself.
pub const RESERVED_TAG_PREFIX: &str = "steading-";
/// The tag prefix naming the plan that made a snapshot.
pub const PLAN_TAG_PREFIX: &str = "steading-plan:";
/// The tag prefix naming the agent that made a snapshot.
pub const AGENT_TAG_PREFIX: &str = "steading-agent:";

/// A tag the user adds to a plan's snapshots. It never starts with
/// [`RESERVED_TAG_PREFIX`], in any case, so it can't make a snapshot look like
/// another plan's and be forgotten by that plan (I6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserTag(String);

impl UserTag {
    pub fn new(tag: impl Into<String>) -> Result<Self, ReservedTag> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A tag that uses Steading's reserved prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservedTag {
    pub tag: String,
}

impl std::fmt::Display for ReservedTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ReservedTag {}

/// `steading-plan:<plan>` and `steading-agent:<agent>`: every Steading backup
/// carries both, and they make the snapshot managed by that plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanTags {
    plan: PlanId,
    agent: AgentId,
}

impl PlanTags {
    pub fn new(plan: PlanId, agent: AgentId) -> Self {
        Self { plan, agent }
    }

    pub fn plan(&self) -> &PlanId {
        &self.plan
    }

    pub fn agent(&self) -> AgentId {
        self.agent
    }

    /// The two tags, plan first.
    pub fn tags(&self) -> [String; 2] {
        todo!()
    }

    /// Whether `snapshot` carries both tags.
    pub fn mark(&self, snapshot: &SnapshotRef) -> bool {
        todo!()
    }

    /// The parent for this plan's next backup: the newest snapshot carrying
    /// both tags, so changing the plan's sources, label or the hostname keeps
    /// its history attached. `None` when there is none yet (the first backup,
    /// or history from plain rustic); the engine then falls back to rustic's
    /// own choice, the newest snapshot with the same host, label and paths.
    pub fn parent<'a>(
        &self,
        snapshots: impl IntoIterator<Item = &'a SnapshotRef>,
    ) -> Option<SnapshotId> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Timestamp, Uuid};

    fn agent(n: u128) -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(n))
    }

    fn tags() -> PlanTags {
        PlanTags::new(PlanId::parse("home").unwrap(), agent(1))
    }

    fn snapshot(id: u8, second: i64, tags: &[&str]) -> SnapshotRef {
        SnapshotRef {
            id: SnapshotId::parse(&format!("{id:02x}").repeat(32)).unwrap(),
            time: Timestamp::from_second(1_800_000_000 + second).unwrap(),
            host: "laptop".to_owned(),
            label: String::new(),
            paths: vec!["/home/demo".to_owned()],
            tags: tags.iter().map(|&t| t.to_owned()).collect(),
        }
    }

    const PLAN: &str = "steading-plan:home";
    const AGENT: &str = "steading-agent:00000000-0000-0000-0000-000000000001";

    #[test]
    fn a_plan_tags_its_snapshots_with_plan_and_agent() {
        assert_eq!(tags().tags(), [PLAN.to_owned(), AGENT.to_owned()]);
    }

    #[test]
    fn a_user_tag_cannot_use_the_reserved_prefix() {
        for tag in ["daily", "steading", "my-steading-plan:home", "steadingx"] {
            assert_eq!(
                UserTag::new(tag).map(|t| t.as_str().to_owned()),
                Ok(tag.to_owned())
            );
        }
        for tag in [
            "steading-plan:other",
            "steading-agent:x",
            "steading-",
            "Steading-Plan:other",
        ] {
            assert_eq!(
                UserTag::new(tag),
                Err(ReservedTag {
                    tag: tag.to_owned()
                }),
                "{tag}"
            );
        }
    }

    #[test]
    fn a_snapshot_belongs_to_a_plan_only_with_both_tags() {
        assert!(tags().mark(&snapshot(1, 0, &["daily", AGENT, PLAN])));
        assert!(!tags().mark(&snapshot(1, 0, &[PLAN])));
        assert!(!tags().mark(&snapshot(1, 0, &[AGENT])));
        assert!(!tags().mark(&snapshot(1, 0, &["steading-plan:home-old", AGENT])));
        let other_agent = PlanTags::new(PlanId::parse("home").unwrap(), agent(2));
        assert!(!other_agent.mark(&snapshot(1, 0, &[PLAN, AGENT])));
    }

    #[test]
    fn the_parent_is_the_newest_snapshot_of_the_plan() {
        let snapshots = [
            snapshot(1, 10, &[PLAN, AGENT]),
            snapshot(2, 30, &[PLAN, AGENT]),
            snapshot(3, 20, &[PLAN, AGENT]),
            snapshot(4, 40, &[PLAN]),
            snapshot(5, 50, &[]),
        ];
        assert_eq!(tags().parent(&snapshots), Some(snapshots[1].id.clone()));
    }

    #[test]
    fn without_a_snapshot_of_the_plan_rustic_chooses_the_parent() {
        let snapshots = [snapshot(4, 40, &[PLAN]), snapshot(5, 50, &[])];
        assert_eq!(tags().parent(&snapshots), None);
        assert_eq!(tags().parent(&[]), None);
    }

    #[test]
    fn snapshots_made_in_the_same_second_choose_the_parent_by_id() {
        let snapshots = [
            snapshot(9, 30, &[PLAN, AGENT]),
            snapshot(7, 30, &[PLAN, AGENT]),
        ];
        assert_eq!(tags().parent(&snapshots), Some(snapshots[0].id.clone()));
        let reversed = [snapshots[1].clone(), snapshots[0].clone()];
        assert_eq!(tags().parent(&reversed), Some(snapshots[0].id.clone()));
    }
}
