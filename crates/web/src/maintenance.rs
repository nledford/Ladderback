//! The decisions behind the Snapshots page's maintenance (technical design
//! §4.4): the request each button sends, what a preview tells a person before
//! they confirm, how many rows a list shows, which group can be adopted and
//! how a run's end is awaited and announced. Every answer is data; the page
//! words it from the catalog. Host-tested here; the DOM glue that sends the
//! requests is `routes.rs`.

use std::time::Duration;

use api::{
    AdoptionFromDto, ErrorCode, MaintenancePreviewDto, MaintenanceRequestDto, OrphanedGroupDto,
    SnapshotDto,
};

use crate::announcer::Politeness;
use crate::backend::{ended, reconnect_backoff};

/// What a maintenance run does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Forget,
    Prune,
    Check,
    Adopt,
}

impl Kind {
    /// The kind text a request carries.
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Forget => "forget",
            Self::Prune => "prune",
            Self::Check => "check",
            Self::Adopt => "adopt",
        }
    }

    /// The kind a request's text names; the exact lower-case text only.
    #[allow(
        dead_code,
        reason = "the inverse of `wire`, which the host tests pin; the page names a kind and never reads one"
    )]
    pub(crate) fn of(text: &str) -> Option<Kind> {
        match text {
            "forget" => Some(Self::Forget),
            "prune" => Some(Self::Prune),
            "check" => Some(Self::Check),
            "adopt" => Some(Self::Adopt),
            _ => None,
        }
    }
}

/// The request that starts or previews `kind`: its kind text and nothing
/// else. An adoption names its plan and group, so it has [`adoption`].
pub(crate) fn request(kind: Kind) -> MaintenanceRequestDto {
    MaintenanceRequestDto {
        kind: kind.wire().to_owned(),
        into: None,
        from: None,
        selection: None,
    }
}

/// The request that adopts the group `from` into the plan `into`.
pub(crate) fn adoption(into: &str, from: AdoptionFromDto) -> MaintenanceRequestDto {
    MaintenanceRequestDto {
        into: Some(into.to_owned()),
        from: Some(from),
        ..request(Kind::Adopt)
    }
}

/// The route that reads a repository's history, grouped by owner.
pub(crate) fn groups_path(alias: &str) -> String {
    format!("/api/v1/repositories/{alias}/groups")
}

/// The route that previews a maintenance run.
pub(crate) fn preview_path(alias: &str) -> String {
    format!("/api/v1/repositories/{alias}/maintenance/preview")
}

/// The route that starts a maintenance run.
pub(crate) fn start_path(alias: &str) -> String {
    format!("/api/v1/repositories/{alias}/maintenance")
}

/// The route that reads a run's operation: its status says how far it is.
pub(crate) fn operation_path(operation: &str) -> String {
    format!("/api/v1/operations/{operation}")
}

/// The route that asks a run to stop.
pub(crate) fn cancel_path(operation: &str) -> String {
    format!("/api/v1/operations/{operation}/cancel")
}

/// Whether a refusal of a preview or a start says that what the page shows
/// is out of date: another run holds the repository, or the history changed
/// after the review. The page reads the history again, so that what it shows
/// is what the next review would be about.
pub(crate) fn rereads(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::MaintenanceBusy | ErrorCode::MaintenanceSelectionChanged
    )
}

/// How long a pack stays marked before a later prune deletes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wait {
    Days(u64),
    Hours(u64),
}

impl Wait {
    /// Whole days when `hours` is a multiple of 24 that is at least 24,
    /// otherwise hours.
    pub(crate) fn of(hours: u64) -> Wait {
        if hours >= 24 && hours.is_multiple_of(24) {
            Wait::Days(hours / 24)
        } else {
            Wait::Hours(hours)
        }
    }
}

/// One thing a preview tells a person, with no text of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fact {
    /// A forget removes `remove` of the `total` snapshots of `plan`.
    Removes {
        plan: String,
        remove: u64,
        total: u64,
    },
    /// A forget leaves this much history alone (I6).
    LeavesAlone {
        orphaned_groups: u64,
        orphaned_snapshots: u64,
        unmanaged_groups: u64,
        unmanaged_snapshots: u64,
    },
    /// An adoption rewrites `snapshots` into the plan `into`.
    Adopts { into: String, snapshots: u64 },
    /// A prune's dry run.
    Prunes {
        marked: u64,
        deleted: u64,
        recovered: u64,
        bytes: u64,
        wait: Wait,
    },
    /// A check's reach.
    Checks { read_data: bool },
}

/// What the confirmation button commits to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confirm {
    Forget { snapshots: u64 },
    Adopt { snapshots: u64 },
    Prune,
    Check,
}

/// A preview read for a person: what to show and what confirming sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub(crate) kind: Kind,
    /// The request that was previewed.
    pub(crate) request: MaintenanceRequestDto,
    pub(crate) facts: Vec<Fact>,
    /// The snapshots the preview names, for a forget or an adoption: the
    /// plans that lose some, or the plan that takes them.
    pub(crate) listings: Vec<Listing>,
    /// What confirming commits to; `None` when there is nothing to do.
    pub(crate) confirm: Option<Confirm>,
    /// The token the preview named, for a forget or an adoption.
    pub(crate) selection: Option<String>,
}

impl Review {
    /// The review of `preview`, the answer to `request`.
    pub(crate) fn of(request: &MaintenanceRequestDto, preview: &MaintenancePreviewDto) -> Review {
        let request = request.clone();
        match preview {
            MaintenancePreviewDto::Forget {
                plans,
                left_alone,
                selection,
            } => {
                let mut facts: Vec<Fact> = plans
                    .iter()
                    .map(|plan| Fact::Removes {
                        plan: plan.plan.clone(),
                        remove: plan.remove,
                        total: plan.total,
                    })
                    .collect();
                if left_alone.orphaned.snapshots > 0 || left_alone.unmanaged.snapshots > 0 {
                    facts.push(Fact::LeavesAlone {
                        orphaned_groups: left_alone.orphaned.groups,
                        orphaned_snapshots: left_alone.orphaned.snapshots,
                        unmanaged_groups: left_alone.unmanaged.groups,
                        unmanaged_snapshots: left_alone.unmanaged.snapshots,
                    });
                }
                let removing = plans
                    .iter()
                    .fold(0, |total: u64, plan| total.saturating_add(plan.remove));
                Review {
                    kind: Kind::Forget,
                    request,
                    facts,
                    listings: plans
                        .iter()
                        .filter_map(|plan| Listing::of(&plan.plan, plan.remove, &plan.listed))
                        .collect(),
                    confirm: (removing > 0).then_some(Confirm::Forget {
                        snapshots: removing,
                    }),
                    selection: Some(selection.clone()),
                }
            }
            MaintenancePreviewDto::Adopt {
                into,
                count,
                listed,
                selection,
            } => Review {
                kind: Kind::Adopt,
                request,
                facts: vec![Fact::Adopts {
                    into: into.clone(),
                    snapshots: *count,
                }],
                listings: Listing::of(into, *count, listed).into_iter().collect(),
                confirm: (*count > 0).then_some(Confirm::Adopt { snapshots: *count }),
                selection: Some(selection.clone()),
            },
            MaintenancePreviewDto::Prune {
                packs_marked,
                packs_deleted,
                packs_recovered,
                bytes_deleted,
                keep_delete_hours,
            } => Review {
                kind: Kind::Prune,
                request,
                facts: vec![Fact::Prunes {
                    marked: *packs_marked,
                    deleted: *packs_deleted,
                    recovered: *packs_recovered,
                    bytes: *bytes_deleted,
                    wait: Wait::of(*keep_delete_hours),
                }],
                listings: Vec::new(),
                confirm: (*packs_marked > 0
                    || *packs_deleted > 0
                    || *packs_recovered > 0
                    || *bytes_deleted > 0)
                    .then_some(Confirm::Prune),
                selection: None,
            },
            MaintenancePreviewDto::Check { read_data } => Review {
                kind: Kind::Check,
                request,
                facts: vec![Fact::Checks {
                    read_data: *read_data,
                }],
                listings: Vec::new(),
                confirm: Some(Confirm::Check),
                selection: None,
            },
        }
    }

    /// The request that confirming sends: the previewed request carrying the
    /// preview's token for a forget or an adoption, and none for a prune or
    /// a check.
    pub(crate) fn start_body(&self) -> MaintenanceRequestDto {
        let selection = match self.kind {
            Kind::Forget | Kind::Adopt => self.selection.clone(),
            Kind::Prune | Kind::Check => None,
        };
        MaintenanceRequestDto {
            selection,
            ..self.request.clone()
        }
    }

    /// Whether there is nothing to confirm.
    pub(crate) fn nothing_to_do(&self) -> bool {
        self.confirm.is_none()
    }
}

/// The snapshots a preview names under one plan: the ones a forget removes
/// from it, or the ones an adoption rewrites into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) plan: String,
    /// The snapshots the preview listed, in its order.
    pub(crate) snapshots: Vec<SnapshotDto>,
    /// How many more the run acts on than the preview lists.
    pub(crate) more: u64,
}

impl Listing {
    /// What a preview names for `plan`, of `count` snapshots it acts on of
    /// which it listed `listed`; `None` when there is nothing to name.
    fn of(plan: &str, count: u64, listed: &[SnapshotDto]) -> Option<Listing> {
        let rows = Rows::of(count, listed.len());
        (rows.shown > 0 || rows.more > 0).then(|| Listing {
            plan: plan.to_owned(),
            snapshots: listed.to_vec(),
            more: rows.more,
        })
    }
}

/// How many rows of a list are shown and how many are not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rows {
    pub(crate) shown: usize,
    pub(crate) more: u64,
}

impl Rows {
    /// The rows of a list of `listed` items out of `count`.
    pub(crate) fn of(count: u64, listed: usize) -> Rows {
        Rows {
            shown: listed,
            more: count.saturating_sub(u64::try_from(listed).unwrap_or(u64::MAX)),
        }
    }

    /// Whether there is nothing to show and nothing more to say.
    pub(crate) fn is_empty(&self) -> bool {
        self.shown == 0 && self.more == 0
    }
}

/// How an orphaned group names the plan and agent that owned it: by both
/// tags, by one, or by neither. A group is orphaned for a tag it carries, so
/// neither is not expected, but the wire can say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner<'a> {
    Both { agent: &'a str, plan: &'a str },
    Plan(&'a str),
    Agent(&'a str),
    Neither,
}

impl<'a> Owner<'a> {
    pub(crate) fn of(group: &'a OrphanedGroupDto) -> Owner<'a> {
        match (group.agent.as_deref(), group.plan.as_deref()) {
            (Some(agent), Some(plan)) => Owner::Both { agent, plan },
            (None, Some(plan)) => Owner::Plan(plan),
            (Some(agent), None) => Owner::Agent(agent),
            (None, None) => Owner::Neither,
        }
    }
}

/// The request that adopts the orphaned `group` into the plan `into`. The
/// daemon finds the group by both its tags, so a group that lacks one cannot
/// be named, and has none.
pub(crate) fn adopt_orphaned(
    group: &OrphanedGroupDto,
    into: &str,
) -> Option<MaintenanceRequestDto> {
    match Owner::of(group) {
        Owner::Both { agent, plan } => Some(adoption(
            into,
            AdoptionFromDto::Orphaned {
                agent: agent.to_owned(),
                plan: plan.to_owned(),
            },
        )),
        Owner::Plan(_) | Owner::Agent(_) | Owner::Neither => None,
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Finished,
    Cancelled,
    Failed,
}

/// What an end of a maintenance run is announced as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct End {
    pub(crate) kind: Kind,
    pub(crate) outcome: Outcome,
    pub(crate) politeness: Politeness,
}

impl End {
    /// The end of a `kind` run whose operation status is `status`: only a
    /// failure is assertive.
    pub(crate) fn of(kind: Kind, status: &str) -> End {
        let outcome = match status {
            "failed" => Outcome::Failed,
            "cancelled" => Outcome::Cancelled,
            _ => Outcome::Finished,
        };
        let politeness = match outcome {
            Outcome::Failed => Politeness::Assertive,
            Outcome::Cancelled | Outcome::Finished => Politeness::Polite,
        };
        End {
            kind,
            outcome,
            politeness,
        }
    }
}

/// How long to wait before reading a running maintenance's status again,
/// after its `reads`-th read, counting from 1: a quarter of a second at
/// first, so a run that ends in a moment is told at once, then every second,
/// then every five, so a prune that takes an hour costs the daemon a few
/// hundred reads and not thousands.
pub(crate) fn poll_delay(reads: u32) -> Duration {
    if reads <= QUARTER_SECOND_READS {
        Duration::from_millis(250)
    } else if reads <= SECOND_READS {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(5)
    }
}

/// The last read a quarter of a second follows, and the last read a second
/// follows.
const QUARTER_SECOND_READS: u32 = 20;
const SECOND_READS: u32 = 80;

/// How many reads of a run's status may fail one after the other before the
/// wait gives up: with the back-off between them (1, 2, 4, 8, 16 and 30
/// seconds), a minute of a daemon that does not answer.
const MAX_MISSES: u32 = 7;

/// What the wait for a run does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Read again after this long.
    Again(Duration),
    /// The run is over.
    Ended(End),
    /// The daemon has not answered for too long to say how the run went.
    GaveUp,
}

/// The wait for one run to end: it counts the reads and the misses, and
/// says after each what comes next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Watch {
    kind: Kind,
    reads: u32,
    misses: u32,
}

impl Watch {
    pub(crate) fn new(kind: Kind) -> Watch {
        Watch {
            kind,
            reads: 0,
            misses: 0,
        }
    }

    /// What comes after a read of the run's status: `None` is a read that
    /// got no answer.
    pub(crate) fn after(&mut self, status: Option<&str>) -> Next {
        match status {
            Some(status) if ended(status) => Next::Ended(End::of(self.kind, status)),
            Some(_) => {
                self.misses = 0;
                self.reads = self.reads.saturating_add(1);
                Next::Again(poll_delay(self.reads))
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                if self.misses >= MAX_MISSES {
                    Next::GaveUp
                } else {
                    Next::Again(reconnect_backoff(self.misses))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{ForgetPlanDto, LeftAloneDto, SnapshotDto, TallyDto};

    fn snapshot(n: u8) -> SnapshotDto {
        SnapshotDto::new(
            format!("{n:02x}").repeat(32),
            "2026-05-01T10:00:00Z".to_owned(),
            "laptop".to_owned(),
            String::new(),
            vec!["ladderback-plan:home".to_owned()],
            vec!["/home".to_owned()],
        )
        .expect("a valid snapshot")
    }

    fn plan(name: &str, total: u64, remove: u64) -> ForgetPlanDto {
        ForgetPlanDto {
            plan: name.to_owned(),
            total,
            remove,
            listed: (0..remove.min(3))
                .map(|n| snapshot(u8::try_from(n).unwrap_or(0)))
                .collect(),
        }
    }

    fn left(
        orphaned_groups: u64,
        orphaned_snapshots: u64,
        unmanaged_groups: u64,
        unmanaged_snapshots: u64,
    ) -> LeftAloneDto {
        LeftAloneDto {
            orphaned: TallyDto {
                groups: orphaned_groups,
                snapshots: orphaned_snapshots,
            },
            unmanaged: TallyDto {
                groups: unmanaged_groups,
                snapshots: unmanaged_snapshots,
            },
        }
    }

    fn forget(plans: Vec<ForgetPlanDto>, left_alone: LeftAloneDto) -> MaintenancePreviewDto {
        MaintenancePreviewDto::Forget {
            plans,
            left_alone,
            selection: "0123456789abcdef".to_owned(),
        }
    }

    fn prune(marked: u64, deleted: u64, recovered: u64, bytes: u64) -> MaintenancePreviewDto {
        MaintenancePreviewDto::Prune {
            packs_marked: marked,
            packs_deleted: deleted,
            packs_recovered: recovered,
            bytes_deleted: bytes,
            keep_delete_hours: 144,
        }
    }

    fn plain(kind: Kind) -> MaintenanceRequestDto {
        MaintenanceRequestDto {
            kind: kind.wire().to_owned(),
            into: None,
            from: None,
            selection: None,
        }
    }

    #[test]
    fn each_kind_names_its_wire_text() {
        assert_eq!(Kind::Forget.wire(), "forget");
        assert_eq!(Kind::Prune.wire(), "prune");
        assert_eq!(Kind::Check.wire(), "check");
        assert_eq!(Kind::Adopt.wire(), "adopt");
    }

    #[test]
    fn each_kind_round_trips_through_its_wire_text() {
        for kind in [Kind::Forget, Kind::Prune, Kind::Check, Kind::Adopt] {
            assert_eq!(Kind::of(kind.wire()), Some(kind));
        }
    }

    #[test]
    fn a_text_that_is_not_exactly_a_kind_names_none() {
        for text in ["Forget", " forget", "forget ", "", "restore"] {
            assert_eq!(Kind::of(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_plain_request_carries_only_its_kind() {
        let request = request(Kind::Prune);
        assert_eq!(
            request,
            MaintenanceRequestDto {
                kind: "prune".to_owned(),
                into: None,
                from: None,
                selection: None,
            }
        );
    }

    #[test]
    fn an_adoption_request_names_its_plan_and_group() {
        let from = AdoptionFromDto::Orphaned {
            agent: "ladderback-agent:other".to_owned(),
            plan: "ladderback-plan:old".to_owned(),
        };
        assert_eq!(
            adoption("home", from.clone()),
            MaintenanceRequestDto {
                kind: "adopt".to_owned(),
                into: Some("home".to_owned()),
                from: Some(from),
                selection: None,
            }
        );
    }

    #[test]
    fn the_three_routes_are_under_the_repository() {
        assert_eq!(groups_path("main"), "/api/v1/repositories/main/groups");
        assert_eq!(
            preview_path("main"),
            "/api/v1/repositories/main/maintenance/preview"
        );
        assert_eq!(start_path("main"), "/api/v1/repositories/main/maintenance");
    }

    #[test]
    fn a_forget_review_lists_each_plan_in_order_even_one_removing_nothing() {
        let preview = forget(
            vec![plan("home", 9, 4), plan("work", 2, 0)],
            left(0, 0, 0, 0),
        );
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.kind, Kind::Forget);
        assert_eq!(
            review.facts,
            vec![
                Fact::Removes {
                    plan: "home".to_owned(),
                    remove: 4,
                    total: 9
                },
                Fact::Removes {
                    plan: "work".to_owned(),
                    remove: 0,
                    total: 2
                },
            ]
        );
    }

    #[test]
    fn a_forget_review_says_what_it_leaves_alone_once_with_its_tallies() {
        let preview = forget(vec![plan("home", 9, 4)], left(2, 7, 1, 3));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(
            review.facts.last(),
            Some(&Fact::LeavesAlone {
                orphaned_groups: 2,
                orphaned_snapshots: 7,
                unmanaged_groups: 1,
                unmanaged_snapshots: 3,
            })
        );
        assert_eq!(review.facts.len(), 2);
    }

    #[test]
    fn a_forget_review_leaves_alone_a_single_orphaned_snapshot() {
        let preview = forget(vec![plan("home", 9, 4)], left(1, 1, 0, 0));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.facts.len(), 2);
    }

    #[test]
    fn a_forget_review_leaves_alone_a_single_unmanaged_snapshot() {
        let preview = forget(vec![plan("home", 9, 4)], left(0, 0, 1, 1));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.facts.len(), 2);
    }

    #[test]
    fn a_forget_review_without_history_left_alone_has_no_such_fact() {
        let preview = forget(vec![plan("home", 9, 4)], left(0, 0, 0, 0));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.facts.len(), 1);
    }

    #[test]
    fn a_forget_confirmation_counts_the_snapshots_of_every_plan() {
        let preview = forget(
            vec![plan("home", 9, 4), plan("work", 5, 2), plan("misc", 1, 0)],
            left(0, 0, 0, 0),
        );
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.confirm, Some(Confirm::Forget { snapshots: 6 }));
        assert!(!review.nothing_to_do());
    }

    #[test]
    fn a_forget_removing_one_snapshot_in_total_can_be_confirmed() {
        let preview = forget(
            vec![plan("home", 9, 0), plan("work", 5, 1)],
            left(0, 0, 0, 0),
        );
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.confirm, Some(Confirm::Forget { snapshots: 1 }));
    }

    #[test]
    fn a_forget_removing_nothing_has_nothing_to_confirm() {
        let preview = forget(
            vec![plan("home", 9, 0), plan("work", 5, 0)],
            left(3, 8, 1, 2),
        );
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.confirm, None);
        assert!(review.nothing_to_do());
    }

    #[test]
    fn a_forget_review_echoes_the_previews_selection() {
        let preview = forget(vec![plan("home", 9, 4)], left(0, 0, 0, 0));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(review.selection.as_deref(), Some("0123456789abcdef"));
        assert_eq!(review.request, plain(Kind::Forget));
    }

    #[test]
    fn an_adoption_review_counts_what_it_rewrites_into_the_plan() {
        let request = adoption("home", AdoptionFromDto::Unmanaged);
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 12,
            listed: vec![snapshot(1)],
            selection: "fedcba9876543210".to_owned(),
        };
        let review = Review::of(&request, &preview);
        assert_eq!(review.kind, Kind::Adopt);
        assert_eq!(
            review.facts,
            vec![Fact::Adopts {
                into: "home".to_owned(),
                snapshots: 12
            }]
        );
        assert_eq!(review.confirm, Some(Confirm::Adopt { snapshots: 12 }));
        assert_eq!(review.selection.as_deref(), Some("fedcba9876543210"));
    }

    #[test]
    fn an_adoption_of_nothing_has_nothing_to_confirm() {
        let request = adoption("home", AdoptionFromDto::Unmanaged);
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 0,
            listed: Vec::new(),
            selection: "fedcba9876543210".to_owned(),
        };
        let review = Review::of(&request, &preview);
        assert_eq!(review.confirm, None);
        assert!(review.nothing_to_do());
    }

    #[test]
    fn an_adoption_of_one_snapshot_can_be_confirmed() {
        let request = adoption("home", AdoptionFromDto::Unmanaged);
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 1,
            listed: vec![snapshot(1)],
            selection: "fedcba9876543210".to_owned(),
        };
        let review = Review::of(&request, &preview);
        assert_eq!(review.confirm, Some(Confirm::Adopt { snapshots: 1 }));
    }

    #[test]
    fn a_prune_review_carries_the_dry_run_and_how_long_packs_wait() {
        let review = Review::of(&plain(Kind::Prune), &prune(3, 2, 1, 4096));
        assert_eq!(review.kind, Kind::Prune);
        assert_eq!(
            review.facts,
            vec![Fact::Prunes {
                marked: 3,
                deleted: 2,
                recovered: 1,
                bytes: 4096,
                wait: Wait::Days(6),
            }]
        );
        assert_eq!(review.confirm, Some(Confirm::Prune));
        assert_eq!(review.selection, None);
    }

    #[test]
    fn a_prune_with_only_marked_packs_can_be_confirmed() {
        let review = Review::of(&plain(Kind::Prune), &prune(1, 0, 0, 0));
        assert_eq!(review.confirm, Some(Confirm::Prune));
    }

    #[test]
    fn a_prune_with_only_deleted_packs_can_be_confirmed() {
        let review = Review::of(&plain(Kind::Prune), &prune(0, 1, 0, 0));
        assert_eq!(review.confirm, Some(Confirm::Prune));
    }

    #[test]
    fn a_prune_with_only_recovered_packs_can_be_confirmed() {
        let review = Review::of(&plain(Kind::Prune), &prune(0, 0, 1, 0));
        assert_eq!(review.confirm, Some(Confirm::Prune));
    }

    #[test]
    fn a_prune_with_only_bytes_to_delete_can_be_confirmed() {
        let review = Review::of(&plain(Kind::Prune), &prune(0, 0, 0, 1));
        assert_eq!(review.confirm, Some(Confirm::Prune));
    }

    #[test]
    fn a_prune_that_changes_nothing_has_nothing_to_confirm() {
        let review = Review::of(&plain(Kind::Prune), &prune(0, 0, 0, 0));
        assert_eq!(review.confirm, None);
        assert!(review.nothing_to_do());
    }

    #[test]
    fn a_check_review_reports_its_reach_and_is_always_confirmable() {
        for read_data in [false, true] {
            let preview = MaintenancePreviewDto::Check { read_data };
            let review = Review::of(&plain(Kind::Check), &preview);
            assert_eq!(review.kind, Kind::Check);
            assert_eq!(review.facts, vec![Fact::Checks { read_data }]);
            assert_eq!(review.confirm, Some(Confirm::Check));
            assert_eq!(review.selection, None);
            assert!(!review.nothing_to_do());
        }
    }

    #[test]
    fn a_forget_starts_with_the_selection_the_preview_named() {
        let preview = forget(vec![plan("home", 9, 4)], left(0, 0, 0, 0));
        let review = Review::of(&plain(Kind::Forget), &preview);
        let body = review.start_body();
        assert_eq!(body.kind, "forget");
        assert_eq!(body.selection.as_deref(), Some("0123456789abcdef"));
    }

    #[test]
    fn an_adoption_starts_with_its_selection_and_keeps_its_plan_and_group() {
        let from = AdoptionFromDto::Orphaned {
            agent: "ladderback-agent:other".to_owned(),
            plan: "ladderback-plan:old".to_owned(),
        };
        let request = adoption("home", from.clone());
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 2,
            listed: Vec::new(),
            selection: "fedcba9876543210".to_owned(),
        };
        let body = Review::of(&request, &preview).start_body();
        assert_eq!(body.kind, "adopt");
        assert_eq!(body.into.as_deref(), Some("home"));
        assert_eq!(body.from, Some(from));
        assert_eq!(body.selection.as_deref(), Some("fedcba9876543210"));
    }

    #[test]
    fn a_prune_starts_without_a_selection_even_when_the_review_holds_one() {
        let mut review = Review::of(&plain(Kind::Prune), &prune(1, 1, 1, 1));
        review.selection = Some("0123456789abcdef".to_owned());
        let body = review.start_body();
        assert_eq!(body.kind, "prune");
        assert_eq!(body.selection, None);
    }

    #[test]
    fn a_check_starts_without_a_selection_even_when_the_review_holds_one() {
        let preview = MaintenancePreviewDto::Check { read_data: false };
        let mut review = Review::of(&plain(Kind::Check), &preview);
        review.selection = Some("0123456789abcdef".to_owned());
        let body = review.start_body();
        assert_eq!(body.kind, "check");
        assert_eq!(body.selection, None);
    }

    #[test]
    fn a_wait_of_whole_days_is_counted_in_days() {
        assert_eq!(Wait::of(24), Wait::Days(1));
        assert_eq!(Wait::of(48), Wait::Days(2));
        assert_eq!(Wait::of(144), Wait::Days(6));
    }

    #[test]
    fn a_wait_that_is_not_whole_days_is_counted_in_hours() {
        assert_eq!(Wait::of(0), Wait::Hours(0));
        assert_eq!(Wait::of(1), Wait::Hours(1));
        assert_eq!(Wait::of(23), Wait::Hours(23));
        assert_eq!(Wait::of(25), Wait::Hours(25));
        assert_eq!(Wait::of(36), Wait::Hours(36));
    }

    #[test]
    fn rows_not_listed_are_counted_as_more() {
        let rows = Rows::of(120, 50);
        assert_eq!(rows.shown, 50);
        assert_eq!(rows.more, 70);
    }

    #[test]
    fn a_list_of_every_row_has_no_more() {
        let rows = Rows::of(3, 3);
        assert_eq!((rows.shown, rows.more), (3, 0));
        let rows = Rows::of(0, 0);
        assert_eq!((rows.shown, rows.more), (0, 0));
    }

    #[test]
    fn one_row_not_listed_is_one_more() {
        let rows = Rows::of(4, 3);
        assert_eq!((rows.shown, rows.more), (3, 1));
    }

    #[test]
    fn a_list_longer_than_its_count_has_no_more() {
        let rows = Rows::of(2, 5);
        assert_eq!((rows.shown, rows.more), (5, 0));
    }

    #[test]
    fn a_failed_run_ends_assertively() {
        let end = End::of(Kind::Prune, "failed");
        assert_eq!(end.kind, Kind::Prune);
        assert_eq!(end.outcome, Outcome::Failed);
        assert_eq!(end.politeness, Politeness::Assertive);
    }

    #[test]
    fn a_cancelled_run_ends_politely() {
        let end = End::of(Kind::Check, "cancelled");
        assert_eq!(end.kind, Kind::Check);
        assert_eq!(end.outcome, Outcome::Cancelled);
        assert_eq!(end.politeness, Politeness::Polite);
    }

    #[test]
    fn a_run_that_succeeded_or_ended_with_warnings_or_any_other_status_finished_politely() {
        for status in ["succeeded", "succeeded-with-warnings", "mystery", ""] {
            let end = End::of(Kind::Forget, status);
            assert_eq!(end.kind, Kind::Forget, "{status:?}");
            assert_eq!(end.outcome, Outcome::Finished, "{status:?}");
            assert_eq!(end.politeness, Politeness::Polite, "{status:?}");
        }
    }

    // What a review names, who can be adopted, and how a run is awaited: the
    // decisions the Snapshots page adds to the ones above.

    fn orphaned(agent: Option<&str>, plan: Option<&str>) -> OrphanedGroupDto {
        OrphanedGroupDto {
            agent: agent.map(str::to_owned),
            plan: plan.map(str::to_owned),
            count: 2,
            snapshots: vec![snapshot(1), snapshot(2)],
            adopt_into: vec!["home".to_owned()],
        }
    }

    #[test]
    fn a_forget_review_names_the_snapshots_each_plan_loses_in_the_previews_order() {
        let preview = forget(
            vec![plan("home", 9, 4), plan("work", 2, 0), plan("misc", 5, 2)],
            left(0, 0, 0, 0),
        );
        let review = Review::of(&plain(Kind::Forget), &preview);
        let named: Vec<(&str, usize, u64)> = review
            .listings
            .iter()
            .map(|listing| (listing.plan.as_str(), listing.snapshots.len(), listing.more))
            .collect();
        // `plan()` lists up to three snapshots of the ones a plan removes.
        assert_eq!(named, vec![("home", 3, 1), ("misc", 2, 0)]);
    }

    #[test]
    fn a_listing_keeps_the_snapshots_the_preview_listed() {
        let preview = forget(vec![plan("home", 9, 2)], left(0, 0, 0, 0));
        let review = Review::of(&plain(Kind::Forget), &preview);
        assert_eq!(
            review.listings,
            vec![Listing {
                plan: "home".to_owned(),
                snapshots: vec![snapshot(0), snapshot(1)],
                more: 0,
            }]
        );
    }

    #[test]
    fn a_plan_whose_removals_the_preview_did_not_list_is_still_named() {
        let mut unlisted = plan("home", 9, 4);
        unlisted.listed.clear();
        let review = Review::of(
            &plain(Kind::Forget),
            &forget(vec![unlisted], left(0, 0, 0, 0)),
        );
        assert_eq!(
            review.listings,
            vec![Listing {
                plan: "home".to_owned(),
                snapshots: Vec::new(),
                more: 4,
            }]
        );
    }

    #[test]
    fn an_adoption_review_names_the_snapshots_it_rewrites_into_the_plan() {
        let request = adoption("home", AdoptionFromDto::Unmanaged);
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 120,
            listed: vec![snapshot(1), snapshot(2)],
            selection: "fedcba9876543210".to_owned(),
        };
        let review = Review::of(&request, &preview);
        assert_eq!(
            review.listings,
            vec![Listing {
                plan: "home".to_owned(),
                snapshots: vec![snapshot(1), snapshot(2)],
                more: 118,
            }]
        );
    }

    #[test]
    fn an_adoption_of_nothing_names_nothing() {
        let request = adoption("home", AdoptionFromDto::Unmanaged);
        let preview = MaintenancePreviewDto::Adopt {
            into: "home".to_owned(),
            count: 0,
            listed: Vec::new(),
            selection: "fedcba9876543210".to_owned(),
        };
        assert_eq!(Review::of(&request, &preview).listings, Vec::new());
    }

    #[test]
    fn a_prune_and_a_check_name_no_snapshots() {
        assert_eq!(
            Review::of(&plain(Kind::Prune), &prune(3, 2, 1, 4096)).listings,
            Vec::new()
        );
        let check = MaintenancePreviewDto::Check { read_data: true };
        assert_eq!(Review::of(&plain(Kind::Check), &check).listings, Vec::new());
    }

    #[test]
    fn an_orphaned_group_is_owned_by_the_tags_it_carries() {
        assert_eq!(
            Owner::of(&orphaned(Some("agent-a"), Some("old"))),
            Owner::Both {
                agent: "agent-a",
                plan: "old"
            }
        );
        assert_eq!(Owner::of(&orphaned(None, Some("old"))), Owner::Plan("old"));
        assert_eq!(
            Owner::of(&orphaned(Some("agent-a"), None)),
            Owner::Agent("agent-a")
        );
        assert_eq!(Owner::of(&orphaned(None, None)), Owner::Neither);
    }

    #[test]
    fn an_orphaned_group_with_both_tags_is_adopted_by_naming_them() {
        let group = orphaned(Some("agent-a"), Some("old"));
        assert_eq!(
            adopt_orphaned(&group, "home"),
            Some(adoption(
                "home",
                AdoptionFromDto::Orphaned {
                    agent: "agent-a".to_owned(),
                    plan: "old".to_owned(),
                }
            ))
        );
    }

    #[test]
    fn an_orphaned_group_that_lacks_a_tag_cannot_be_named_so_it_has_no_adoption() {
        assert_eq!(adopt_orphaned(&orphaned(None, Some("old")), "home"), None);
        assert_eq!(
            adopt_orphaned(&orphaned(Some("agent-a"), None), "home"),
            None
        );
        assert_eq!(adopt_orphaned(&orphaned(None, None), "home"), None);
    }

    #[test]
    fn the_status_of_a_run_is_read_every_quarter_of_a_second_at_first() {
        for reads in [0, 1, 20] {
            assert_eq!(poll_delay(reads), Duration::from_millis(250), "{reads}");
        }
    }

    #[test]
    fn the_status_of_a_run_is_then_read_every_second() {
        for reads in [21, 50, 80] {
            assert_eq!(poll_delay(reads), Duration::from_secs(1), "{reads}");
        }
    }

    #[test]
    fn the_status_of_a_long_run_is_read_every_five_seconds() {
        for reads in [81, 1_000, u32::MAX] {
            assert_eq!(poll_delay(reads), Duration::from_secs(5), "{reads}");
        }
    }

    #[test]
    fn a_run_that_is_still_going_is_read_again_after_the_poll_delay() {
        let mut watch = Watch::new(Kind::Prune);
        for status in ["queued", "running"] {
            assert_eq!(
                watch.after(Some(status)),
                Next::Again(Duration::from_millis(250)),
                "{status}"
            );
        }
    }

    #[test]
    fn the_wait_slows_down_as_the_reads_pile_up() {
        let mut watch = Watch::new(Kind::Prune);
        for _ in 0..20 {
            watch.after(Some("running"));
        }
        assert_eq!(
            watch.after(Some("running")),
            Next::Again(Duration::from_secs(1))
        );
    }

    #[test]
    fn a_run_that_ended_is_announced_as_its_kind_and_status_say() {
        for (status, outcome, politeness) in [
            ("succeeded", Outcome::Finished, Politeness::Polite),
            (
                "succeeded-with-warnings",
                Outcome::Finished,
                Politeness::Polite,
            ),
            ("cancelled", Outcome::Cancelled, Politeness::Polite),
            ("failed", Outcome::Failed, Politeness::Assertive),
        ] {
            let mut watch = Watch::new(Kind::Check);
            assert_eq!(
                watch.after(Some(status)),
                Next::Ended(End {
                    kind: Kind::Check,
                    outcome,
                    politeness
                }),
                "{status}"
            );
        }
    }

    #[test]
    fn a_read_that_got_no_answer_is_tried_again_after_a_growing_pause() {
        let mut watch = Watch::new(Kind::Forget);
        let pauses: Vec<Next> = (0..6).map(|_| watch.after(None)).collect();
        assert_eq!(
            pauses,
            [1, 2, 4, 8, 16, 30].map(|seconds| Next::Again(Duration::from_secs(seconds)))
        );
    }

    #[test]
    fn the_wait_gives_up_when_the_daemon_stays_silent() {
        let mut watch = Watch::new(Kind::Forget);
        for _ in 0..6 {
            assert!(matches!(watch.after(None), Next::Again(_)));
        }
        assert_eq!(watch.after(None), Next::GaveUp);
    }

    #[test]
    fn an_answer_forgives_the_reads_that_went_before_it() {
        let mut watch = Watch::new(Kind::Forget);
        for _ in 0..6 {
            watch.after(None);
        }
        assert!(matches!(watch.after(Some("running")), Next::Again(_)));
        assert_eq!(
            watch.after(None),
            Next::Again(Duration::from_secs(1)),
            "the misses start over"
        );
    }

    #[test]
    fn the_reads_that_got_no_answer_do_not_slow_the_poll() {
        let mut watch = Watch::new(Kind::Forget);
        for _ in 0..10 {
            watch.after(None);
            watch.after(Some("running"));
        }
        // The eleventh answered read: still within the quarter-second pace,
        // as it would not be if the ten misses counted as reads.
        assert_eq!(
            watch.after(Some("running")),
            Next::Again(Duration::from_millis(250))
        );
    }

    #[test]
    fn a_run_is_read_and_cancelled_through_its_own_routes() {
        assert_eq!(operation_path("0a1b"), "/api/v1/operations/0a1b");
        assert_eq!(cancel_path("0a1b"), "/api/v1/operations/0a1b/cancel");
    }

    #[test]
    fn a_refusal_that_says_the_page_is_out_of_date_rereads_the_history() {
        assert!(rereads(ErrorCode::MaintenanceBusy));
        assert!(rereads(ErrorCode::MaintenanceSelectionChanged));
    }

    #[test]
    fn a_refusal_about_the_request_itself_leaves_the_history_as_it_is() {
        for code in [
            ErrorCode::RetentionKeepsNothing,
            ErrorCode::AdoptionNoMatch,
            ErrorCode::AdoptionAmbiguous,
            ErrorCode::OperationFailed,
            ErrorCode::RepositoryReadTimedOut,
        ] {
            assert!(!rereads(code), "{code:?}");
        }
    }

    #[test]
    fn a_list_with_nothing_shown_and_nothing_more_is_empty() {
        assert!(Rows::of(0, 0).is_empty());
    }

    #[test]
    fn a_list_with_rows_or_with_more_to_say_is_not_empty() {
        assert!(!Rows::of(3, 3).is_empty());
        assert!(!Rows::of(3, 0).is_empty());
        assert!(!Rows::of(0, 2).is_empty());
    }
}
