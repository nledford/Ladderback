//! Whether a repository holds another agent's tagged snapshots (technical
//! design §7.3; §16, decisions 62 and 156): what a paired agent tells its
//! hub of each repository it uses, so the hub grants `agent:<id>` only for
//! a repository no other agent backs up to (I1). The agent learns it from
//! its listings of the repository and keeps only the newest such
//! snapshot's time, so it judges the 30 days at each report without
//! listing the repository again, for as long as the listing stands.

use super::other_hosts::OTHER_AGENT_RECENCY;
use super::{OwnerTag, owner_tag};
use crate::execution::plan_tags::AGENT_TAG_PREFIX;
use crate::{AgentId, SignedDuration, Timestamp};

/// How long a listing's answer stands (decision 156): a repository last
/// listed longer ago is listed again before the agent's next sync, and
/// until it is, the agent says it holds another agent's snapshots.
pub const LISTING_LASTS: SignedDuration = SignedDuration::from_hours(24);

/// Whether a listing made at `listed` still stands at `now`: at most
/// [`LISTING_LASTS`] apart, either way, so a clock set back counts as time
/// passed.
pub fn listing_stands(listed: Timestamp, now: Timestamp) -> bool {
    now.duration_since(listed).abs() <= LISTING_LASTS
}

/// The newest snapshot another agent tagged, among one listing of a
/// repository: none when the listing held no such snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OtherAgentsSnapshots {
    newest: Option<Timestamp>,
}

impl OtherAgentsSnapshots {
    /// Among `snapshots`, each its time and its tags, as the agent `agent`
    /// listed them. A snapshot is another agent's when its
    /// `ladderback-agent:` tags name another agent, or disagree, which
    /// `agent`'s tag alone no longer proves was made here. One they name
    /// `agent` in, as the history of an earlier installation once adopted
    /// into a plan of this agent, and one no agent tagged, aren't.
    pub fn of<'a>(
        agent: &AgentId,
        snapshots: impl IntoIterator<Item = (Timestamp, &'a [String])>,
    ) -> Self {
        let own = agent.as_uuid().hyphenated().to_string();
        let newest = snapshots
            .into_iter()
            .filter(|(_, tags)| match owner_tag(tags, AGENT_TAG_PREFIX) {
                OwnerTag::One(tagger) => tagger != own,
                OwnerTag::Disagree => true,
                OwnerTag::Absent => false,
            })
            .map(|(time, _)| time)
            .max();
        Self { newest }
    }

    /// Whether one of them is recent at `now`: made in the
    /// `OTHER_AGENT_RECENCY` up to `now`, or later, by a clock ahead of
    /// this one.
    pub fn recent_at(&self, now: Timestamp) -> bool {
        self.newest.is_some_and(|time| recent(time, now))
    }
}

/// Whether a snapshot another agent tagged at `time` is recent at `now`
/// (decision 62): in the [`OTHER_AGENT_RECENCY`] up to `now`, inclusive, or
/// after it. A `now` too near the start of time to go back from counts
/// every snapshot before it.
pub(super) fn recent(time: Timestamp, now: Timestamp) -> bool {
    let since = now
        .checked_sub(OTHER_AGENT_RECENCY)
        .unwrap_or(Timestamp::MIN);
    time >= since
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::plan_tags::PLAN_TAG_PREFIX;
    use crate::{SignedDuration, Uuid};

    fn agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(1))
    }

    fn another_agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(2))
    }

    fn now() -> Timestamp {
        Timestamp::from_second(1_800_000_000).expect("a time")
    }

    fn days(count: i64) -> SignedDuration {
        SignedDuration::from_hours(count * 24)
    }

    /// The tags `agent`'s plan `home` gives its snapshots.
    fn tags_of(agent: &AgentId) -> Vec<String> {
        vec![
            format!("{PLAN_TAG_PREFIX}home"),
            format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
        ]
    }

    /// Whether `listing`, each snapshot's time and tags, held another
    /// agent's recent snapshot for [`agent`] at [`now`].
    fn recent(listing: &[(Timestamp, Vec<String>)]) -> bool {
        OtherAgentsSnapshots::of(
            &agent(),
            listing.iter().map(|(time, tags)| (*time, tags.as_slice())),
        )
        .recent_at(now())
    }

    #[test]
    fn another_agents_snapshot_is_recent_for_exactly_30_days_and_while_it_lies_ahead() {
        let nanosecond = SignedDuration::from_nanos(1);
        for (made, counts) in [
            (now() - days(1), true),
            (now() - days(30), true),
            (now() - days(30) - nanosecond, false),
            (now() - days(400), false),
            (now() + days(2), true),
        ] {
            assert_eq!(
                recent(&[(made, tags_of(&another_agent()))]),
                counts,
                "{made}"
            );
        }
    }

    #[test]
    fn a_snapshot_this_agent_tagged_or_no_agent_tagged_is_no_other_agents() {
        assert!(!recent(&[(now(), tags_of(&agent()))]));
        assert!(!recent(&[(now(), vec!["daily".to_owned()])]));
        assert!(!recent(&[(now(), Vec::new())]));
        assert!(!recent(&[]));
    }

    #[test]
    fn a_snapshot_whose_agent_tags_disagree_is_another_agents_even_naming_this_one() {
        let mut tags = tags_of(&agent());
        tags.push(format!(
            "{AGENT_TAG_PREFIX}{}",
            another_agent().as_uuid().hyphenated()
        ));
        assert!(recent(&[(now() - days(3), tags)]));
    }

    #[test]
    fn the_newest_of_another_agents_snapshots_decides_whatever_the_listings_order() {
        let old = (now() - days(90), tags_of(&another_agent()));
        let new = (now() - days(3), tags_of(&another_agent()));
        let own = (now(), tags_of(&agent()));
        assert!(recent(&[old.clone(), new.clone(), own.clone()]));
        assert!(recent(&[new, old.clone(), own.clone()]));
        assert!(!recent(&[old, own]));
    }

    #[test]
    fn a_listing_is_judged_at_each_now_it_is_asked_at() {
        let listed = OtherAgentsSnapshots::of(
            &agent(),
            [(now() - days(29), tags_of(&another_agent()).as_slice())],
        );
        assert!(listed.recent_at(now()));
        assert!(listed.recent_at(now() + days(1)));
        assert!(!listed.recent_at(now() + days(2)));
    }

    #[test]
    fn a_now_near_the_start_of_time_counts_every_earlier_snapshot() {
        let listed = OtherAgentsSnapshots::of(
            &agent(),
            [(Timestamp::MIN, tags_of(&another_agent()).as_slice())],
        );
        assert!(listed.recent_at(Timestamp::MIN));
    }
}

#[cfg(test)]
mod standing {
    use super::*;

    #[test]
    fn a_listing_stands_for_exactly_a_day_either_way_on_the_clock() {
        let listed = Timestamp::from_second(1_800_000_000).expect("a time");
        let day = SignedDuration::from_hours(24);
        let nanosecond = SignedDuration::from_nanos(1);
        for (now, stands) in [
            (listed, true),
            (listed + day, true),
            (listed + day + nanosecond, false),
            (listed - day, true),
            (listed - day - nanosecond, false),
        ] {
            assert_eq!(listing_stands(listed, now), stands, "{now}");
        }
    }
}
