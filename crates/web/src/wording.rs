//! What the Snapshots page says (technical design §4.4, §8.4): for each thing
//! a review or a run has to tell, the catalog message that tells it and the
//! values its variables read. The page asks the localizer for the text;
//! which message, and with which arguments, is decided here, on the host.

use api::{AdoptionFromDto, MaintenanceRequestDto, MsgArg, OrphanedGroupDto, UnmanagedGroupDto};
use l10n::{MessageId, msg};

use crate::maintenance::{
    self, Confirm, End, Fact, Kind, Outcome, Owner, Review, Stuck, Told, Wait,
};

/// A catalog message and what its variables read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Words {
    pub(crate) id: MessageId,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// A button that adopts a group of history, and the request it sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Adoption {
    pub(crate) label: Words,
    pub(crate) request: MaintenanceRequestDto,
}

fn words(id: MessageId, args: Vec<(&'static str, MsgArg)>) -> Words {
    Words { id, args }
}

fn text(value: &str) -> MsgArg {
    MsgArg::Text(value.to_owned())
}

fn number(value: u64) -> MsgArg {
    MsgArg::Count(value)
}

/// The title of a review: the repository a forget, a prune or a check is
/// about, or the plan an adoption's history becomes part of.
pub(crate) fn title(review: &Review, alias: &str) -> Words {
    match review.kind {
        Kind::Forget => words(msg::UI_REVIEW_TITLE_FORGET, vec![("alias", text(alias))]),
        Kind::Prune => words(msg::UI_REVIEW_TITLE_PRUNE, vec![("alias", text(alias))]),
        Kind::Check => words(msg::UI_REVIEW_TITLE_CHECK, vec![("alias", text(alias))]),
        Kind::Adopt => words(
            msg::UI_REVIEW_TITLE_ADOPT,
            vec![(
                "plan",
                text(review.request.into.as_deref().unwrap_or_default()),
            )],
        ),
    }
}

/// The button that closes a review without running anything: Cancel when
/// there is something to confirm, Close when there is nothing to do.
pub(crate) fn dismiss(review: &Review) -> MessageId {
    if review.nothing_to_do() {
        msg::UI_REVIEW_CLOSE
    } else {
        msg::UI_REVIEW_CANCEL
    }
}

/// The button that confirms a review, by what it commits to.
pub(crate) fn confirmation(confirm: Confirm) -> Words {
    match confirm {
        Confirm::Forget { snapshots } => words(
            msg::UI_REVIEW_CONFIRM_FORGET,
            vec![("count", number(snapshots))],
        ),
        Confirm::Adopt { snapshots } => words(
            msg::UI_REVIEW_CONFIRM_ADOPT,
            vec![("count", number(snapshots))],
        ),
        Confirm::Prune => words(msg::UI_REVIEW_CONFIRM_PRUNE, Vec::new()),
        Confirm::Check => words(msg::UI_REVIEW_CONFIRM_CHECK, Vec::new()),
    }
}

/// What a fact of a review says: one sentence, or two for a prune, whose
/// second says how long a marked pack waits.
pub(crate) fn fact(fact: &Fact) -> Vec<Words> {
    match fact {
        Fact::NoPlans => vec![words(msg::UI_REVIEW_NO_PLANS, Vec::new())],
        Fact::Removes {
            plan,
            remove,
            total,
        } => {
            let id = if *remove == 0 {
                msg::UI_REVIEW_REMOVES_NONE
            } else {
                msg::UI_REVIEW_REMOVES
            };
            vec![words(
                id,
                vec![
                    ("plan", text(plan)),
                    ("remove", number(*remove)),
                    ("total", number(*total)),
                ],
            )]
        }
        Fact::LeavesAlone {
            orphaned_snapshots,
            unmanaged_snapshots,
            ..
        } => vec![match (*orphaned_snapshots > 0, *unmanaged_snapshots > 0) {
            (true, false) => words(
                msg::UI_REVIEW_LEAVES_ORPHANED,
                vec![("snapshots", number(*orphaned_snapshots))],
            ),
            (false, true) => words(
                msg::UI_REVIEW_LEAVES_UNMANAGED,
                vec![("snapshots", number(*unmanaged_snapshots))],
            ),
            _ => words(
                msg::UI_REVIEW_LEAVES_BOTH,
                vec![
                    ("orphaned", number(*orphaned_snapshots)),
                    ("unmanaged", number(*unmanaged_snapshots)),
                ],
            ),
        }],
        Fact::Adopts { into, snapshots } => {
            if *snapshots == 0 {
                vec![words(
                    msg::UI_REVIEW_ADOPTS_NONE,
                    vec![("into", text(into))],
                )]
            } else {
                vec![words(
                    msg::UI_REVIEW_ADOPTS,
                    vec![("snapshots", number(*snapshots)), ("into", text(into))],
                )]
            }
        }
        Fact::Prunes {
            marked,
            deleted,
            recovered,
            bytes,
            wait,
        } => vec![
            words(
                msg::UI_REVIEW_PRUNES,
                vec![
                    ("marked", number(*marked)),
                    ("deleted", number(*deleted)),
                    ("recovered", number(*recovered)),
                    ("bytes", MsgArg::Bytes(*bytes)),
                ],
            ),
            match wait {
                Wait::Days(days) => words(
                    msg::UI_REVIEW_PRUNE_WAIT_DAYS,
                    vec![("days", number(*days))],
                ),
                Wait::Hours(hours) => words(
                    msg::UI_REVIEW_PRUNE_WAIT_HOURS,
                    vec![("hours", number(*hours))],
                ),
            },
        ],
        Fact::Checks { read_data } => {
            let id = if *read_data {
                msg::UI_REVIEW_CHECKS_DATA
            } else {
                msg::UI_REVIEW_CHECKS_STRUCTURE
            };
            vec![words(id, Vec::new())]
        }
    }
}

/// The caption of the table of the snapshots a review names under a plan:
/// the ones a forget removes from it, or the ones an adoption rewrites into
/// it.
pub(crate) fn listing_caption(kind: Kind, plan: &str) -> Words {
    let id = match kind {
        Kind::Adopt => msg::UI_REVIEW_ADOPT_CAPTION,
        Kind::Forget | Kind::Prune | Kind::Check => msg::UI_REVIEW_FORGET_CAPTION,
    };
    words(id, vec![("plan", text(plan))])
}

/// What follows a list that leaves `count` snapshots out; nothing when it
/// leaves none out.
pub(crate) fn more(count: u64) -> Option<Words> {
    (count > 0).then(|| words(msg::UI_SNAPSHOTS_MORE, vec![("count", number(count))]))
}

/// What a run that is going on says, by what it runs.
fn running(kind: Kind) -> Words {
    words(
        msg::UI_MAINTENANCE_RUNNING,
        vec![("kind", text(kind.wire()))],
    )
}

/// How a run's end is told, by what ran and how it ended.
fn end(end: &End) -> Words {
    let id = match end.outcome {
        Outcome::Finished => msg::UI_MAINTENANCE_FINISHED,
        Outcome::Found { .. } => todo!(),
        Outcome::Cancelled => msg::UI_MAINTENANCE_CANCELLED,
        Outcome::Failed => msg::UI_MAINTENANCE_FAILED,
    };
    words(id, vec![("kind", text(end.kind.wire()))])
}

/// What follows the list of what a check found when it leaves `count` out;
/// nothing when it leaves none out.
pub(crate) fn check_more(count: u64) -> Option<Words> {
    let _ = count;
    todo!()
}

/// What the page tells a person of a run, or of the request that comes before
/// one, by what it is.
pub(crate) fn told(told: Told) -> Words {
    match told {
        Told::Preparing => words(msg::UI_MAINTENANCE_PREPARING, Vec::new()),
        Told::Starting(kind) => words(
            msg::UI_MAINTENANCE_STARTING,
            vec![("kind", text(kind.wire()))],
        ),
        Told::Running(kind) => running(kind),
        Told::Ended(ended) => end(&ended),
        Told::Unwatched(kind) => words(
            msg::UI_MAINTENANCE_UNWATCHED,
            vec![("kind", text(kind.wire()))],
        ),
    }
}

/// What an orphaned group is told by: which tags it carries, and their text.
fn owner_args(owner: Owner<'_>) -> Vec<(&'static str, MsgArg)> {
    match owner {
        Owner::Both { agent, plan } => vec![
            ("owner", text("both")),
            ("plan", text(plan)),
            ("agent", text(agent)),
        ],
        Owner::Plan(plan) => vec![("owner", text("plan")), ("plan", text(plan))],
        Owner::Agent(agent) => vec![("owner", text("agent")), ("agent", text(agent))],
        Owner::Neither => vec![("owner", text("neither"))],
    }
}

/// The heading of an orphaned group, by the tags it carries.
pub(crate) fn owner(owner: Owner<'_>) -> Words {
    words(msg::UI_ORPHANED_GROUP, owner_args(owner))
}

/// The caption of the table of an orphaned group's snapshots, by the tags it
/// carries.
pub(crate) fn owner_caption(owner: Owner<'_>) -> Words {
    words(msg::UI_ORPHANED_CAPTION, owner_args(owner))
}

/// What an unmanaged group is told by: whether it has a label, its host and
/// its label.
fn unmanaged_args(group: &UnmanagedGroupDto) -> Vec<(&'static str, MsgArg)> {
    vec![
        (
            "labeled",
            text(if group.label.is_empty() { "no" } else { "yes" }),
        ),
        ("hostname", text(&group.hostname)),
        ("label", text(&group.label)),
    ]
}

/// The heading of an unmanaged group: its host, and its label when it has
/// one.
pub(crate) fn unmanaged(group: &UnmanagedGroupDto) -> Words {
    words(msg::UI_UNMANAGED_GROUP, unmanaged_args(group))
}

/// The caption of the table of an unmanaged group's snapshots, by its host and
/// its label.
pub(crate) fn unmanaged_caption(group: &UnmanagedGroupDto) -> Words {
    words(msg::UI_UNMANAGED_CAPTION, unmanaged_args(group))
}

/// The button that adopts the orphaned `group` into the plan `into`, with
/// its request; none for a group that lacks a tag, which the request could
/// not name.
pub(crate) fn adopt_orphaned(group: &OrphanedGroupDto, into: &str) -> Option<Adoption> {
    let request = maintenance::adopt_orphaned(group, into)?;
    let (agent, plan) = (group.agent.as_deref()?, group.plan.as_deref()?);
    Some(Adoption {
        label: words(
            msg::UI_ADOPT_ORPHANED,
            vec![
                ("plan", text(plan)),
                ("agent", text(agent)),
                ("into", text(into)),
            ],
        ),
        request,
    })
}

/// The button that adopts the unmanaged `group`, named by its host and label,
/// into the plan `into`, with its request.
pub(crate) fn adopt_unmanaged(group: &UnmanagedGroupDto, into: &str) -> Adoption {
    let mut args = unmanaged_args(group);
    args.push(("into", text(into)));
    Adoption {
        label: words(msg::UI_ADOPT_UNMANAGED, args),
        request: maintenance::adoption(into, AdoptionFromDto::Unmanaged),
    }
}

/// What an orphaned group says when nothing offers to adopt it, by why.
pub(crate) fn orphaned_stuck(why: Stuck) -> Words {
    match why {
        Stuck::Untagged => words(msg::UI_ORPHANED_UNTAGGED, Vec::new()),
        Stuck::NoPlan => words(msg::UI_ORPHANED_NO_PLAN, Vec::new()),
    }
}

/// What an unmanaged group says when no plan of this agent could take it.
pub(crate) fn unmanaged_stuck() -> Words {
    words(msg::UI_UNMANAGED_NO_PLAN, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::announcer::Politeness;
    use crate::routes::text_with;
    use api::SnapshotDto;
    use l10n::Localizer;

    fn review(kind: Kind, confirm: Option<Confirm>) -> Review {
        Review {
            kind,
            request: maintenance::request(kind),
            facts: Vec::new(),
            listings: Vec::new(),
            confirm,
            selection: None,
        }
    }

    fn orphaned(agent: Option<&str>, plan: Option<&str>) -> OrphanedGroupDto {
        OrphanedGroupDto {
            agent: agent.map(str::to_owned),
            plan: plan.map(str::to_owned),
            count: 0,
            snapshots: Vec::<SnapshotDto>::new(),
            adopt_into: Vec::new(),
        }
    }

    fn unmanaged_group(label: &str) -> UnmanagedGroupDto {
        UnmanagedGroupDto {
            hostname: "laptop".to_owned(),
            label: label.to_owned(),
            paths: vec!["/home".to_owned(), "/etc".to_owned()],
            count: 0,
            snapshots: Vec::<SnapshotDto>::new(),
            adopt_into: Vec::new(),
        }
    }

    fn said(id: MessageId, args: &[(&'static str, MsgArg)]) -> Words {
        Words {
            id,
            args: args.to_vec(),
        }
    }

    fn name(value: &str) -> MsgArg {
        MsgArg::Text(value.to_owned())
    }

    #[test]
    fn the_review_of_a_forget_a_prune_and_a_check_is_titled_by_the_repository() {
        for (kind, id) in [
            (Kind::Forget, msg::UI_REVIEW_TITLE_FORGET),
            (Kind::Prune, msg::UI_REVIEW_TITLE_PRUNE),
            (Kind::Check, msg::UI_REVIEW_TITLE_CHECK),
        ] {
            assert_eq!(
                title(&review(kind, None), "main"),
                said(id, &[("alias", name("main"))]),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_review_of_an_adoption_is_titled_by_the_plan_that_takes_the_history() {
        let mut adoption = review(Kind::Adopt, None);
        adoption.request = maintenance::adoption("home", AdoptionFromDto::Unmanaged);
        assert_eq!(
            title(&adoption, "main"),
            said(msg::UI_REVIEW_TITLE_ADOPT, &[("plan", name("home"))])
        );
    }

    #[test]
    fn a_review_with_something_to_confirm_is_dismissed_by_cancel() {
        let review = review(Kind::Check, Some(Confirm::Check));
        assert_eq!(dismiss(&review), msg::UI_REVIEW_CANCEL);
    }

    #[test]
    fn a_review_with_nothing_to_do_is_dismissed_by_close() {
        assert_eq!(dismiss(&review(Kind::Forget, None)), msg::UI_REVIEW_CLOSE);
    }

    #[test]
    fn a_confirmation_counts_the_snapshots_a_forget_or_an_adoption_commits_to() {
        assert_eq!(
            confirmation(Confirm::Forget { snapshots: 2 }),
            said(
                msg::UI_REVIEW_CONFIRM_FORGET,
                &[("count", MsgArg::Count(2))]
            )
        );
        assert_eq!(
            confirmation(Confirm::Adopt { snapshots: 5 }),
            said(msg::UI_REVIEW_CONFIRM_ADOPT, &[("count", MsgArg::Count(5))])
        );
    }

    #[test]
    fn a_confirmation_of_a_prune_or_a_check_counts_nothing() {
        assert_eq!(
            confirmation(Confirm::Prune),
            said(msg::UI_REVIEW_CONFIRM_PRUNE, &[])
        );
        assert_eq!(
            confirmation(Confirm::Check),
            said(msg::UI_REVIEW_CONFIRM_CHECK, &[])
        );
    }

    #[test]
    fn a_plan_that_loses_snapshots_says_how_many_of_its_own_go() {
        let removes = Fact::Removes {
            plan: "home".to_owned(),
            remove: 2,
            total: 3,
        };
        assert_eq!(
            fact(&removes),
            vec![said(
                msg::UI_REVIEW_REMOVES,
                &[
                    ("plan", name("home")),
                    ("remove", MsgArg::Count(2)),
                    ("total", MsgArg::Count(3)),
                ]
            )]
        );
    }

    #[test]
    fn a_plan_that_loses_nothing_says_so() {
        let removes = Fact::Removes {
            plan: "home".to_owned(),
            remove: 0,
            total: 3,
        };
        assert_eq!(
            fact(&removes),
            vec![said(
                msg::UI_REVIEW_REMOVES_NONE,
                &[
                    ("plan", name("home")),
                    ("remove", MsgArg::Count(0)),
                    ("total", MsgArg::Count(3)),
                ]
            )]
        );
    }

    fn leaves(orphaned: u64, unmanaged: u64) -> Fact {
        Fact::LeavesAlone {
            orphaned_groups: 1,
            orphaned_snapshots: orphaned,
            unmanaged_groups: 1,
            unmanaged_snapshots: unmanaged,
        }
    }

    #[test]
    fn history_left_alone_names_the_kind_it_found() {
        assert_eq!(
            fact(&leaves(4, 0)),
            vec![said(
                msg::UI_REVIEW_LEAVES_ORPHANED,
                &[("snapshots", MsgArg::Count(4))]
            )]
        );
        assert_eq!(
            fact(&leaves(0, 6)),
            vec![said(
                msg::UI_REVIEW_LEAVES_UNMANAGED,
                &[("snapshots", MsgArg::Count(6))]
            )]
        );
    }

    #[test]
    fn history_of_both_kinds_left_alone_counts_each() {
        assert_eq!(
            fact(&leaves(4, 6)),
            vec![said(
                msg::UI_REVIEW_LEAVES_BOTH,
                &[
                    ("orphaned", MsgArg::Count(4)),
                    ("unmanaged", MsgArg::Count(6)),
                ]
            )]
        );
    }

    #[test]
    fn an_adoption_says_how_many_snapshots_become_part_of_the_plan() {
        let adopts = Fact::Adopts {
            into: "home".to_owned(),
            snapshots: 2,
        };
        assert_eq!(
            fact(&adopts),
            vec![said(
                msg::UI_REVIEW_ADOPTS,
                &[("snapshots", MsgArg::Count(2)), ("into", name("home"))]
            )]
        );
    }

    #[test]
    fn an_adoption_of_nothing_says_so() {
        let adopts = Fact::Adopts {
            into: "home".to_owned(),
            snapshots: 0,
        };
        assert_eq!(
            fact(&adopts),
            vec![said(msg::UI_REVIEW_ADOPTS_NONE, &[("into", name("home"))])]
        );
    }

    fn prunes(wait: Wait) -> Fact {
        Fact::Prunes {
            marked: 3,
            deleted: 2,
            recovered: 1,
            bytes: 4096,
            wait,
        }
    }

    #[test]
    fn a_prune_says_what_it_marks_deletes_and_recovers_and_how_long_a_marked_pack_waits_in_days() {
        assert_eq!(
            fact(&prunes(Wait::Days(2))),
            vec![
                said(
                    msg::UI_REVIEW_PRUNES,
                    &[
                        ("marked", MsgArg::Count(3)),
                        ("deleted", MsgArg::Count(2)),
                        ("recovered", MsgArg::Count(1)),
                        ("bytes", MsgArg::Bytes(4096)),
                    ]
                ),
                said(
                    msg::UI_REVIEW_PRUNE_WAIT_DAYS,
                    &[("days", MsgArg::Count(2))]
                ),
            ]
        );
    }

    #[test]
    fn a_prune_whose_wait_is_not_whole_days_counts_it_in_hours() {
        let sentences = fact(&prunes(Wait::Hours(5)));
        assert_eq!(sentences.len(), 2);
        assert_eq!(
            sentences[1],
            said(
                msg::UI_REVIEW_PRUNE_WAIT_HOURS,
                &[("hours", MsgArg::Count(5))]
            )
        );
    }

    #[test]
    fn a_check_says_whether_it_reads_the_data_too() {
        assert_eq!(
            fact(&Fact::Checks { read_data: false }),
            vec![said(msg::UI_REVIEW_CHECKS_STRUCTURE, &[])]
        );
        assert_eq!(
            fact(&Fact::Checks { read_data: true }),
            vec![said(msg::UI_REVIEW_CHECKS_DATA, &[])]
        );
    }

    #[test]
    fn the_snapshots_an_adoption_names_are_captioned_as_adopted_and_the_others_as_forgotten() {
        assert_eq!(
            listing_caption(Kind::Adopt, "home"),
            said(msg::UI_REVIEW_ADOPT_CAPTION, &[("plan", name("home"))])
        );
        for kind in [Kind::Forget, Kind::Prune, Kind::Check] {
            assert_eq!(
                listing_caption(kind, "home"),
                said(msg::UI_REVIEW_FORGET_CAPTION, &[("plan", name("home"))]),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_list_that_leaves_snapshots_out_says_how_many() {
        assert_eq!(
            more(70),
            Some(said(
                msg::UI_SNAPSHOTS_MORE,
                &[("count", MsgArg::Count(70))]
            ))
        );
        assert_eq!(
            more(1),
            Some(said(msg::UI_SNAPSHOTS_MORE, &[("count", MsgArg::Count(1))]))
        );
    }

    #[test]
    fn a_list_that_leaves_none_out_says_nothing() {
        assert_eq!(more(0), None);
    }

    #[test]
    fn a_run_that_goes_on_is_told_by_its_kind() {
        for kind in [Kind::Forget, Kind::Prune, Kind::Check, Kind::Adopt] {
            assert_eq!(
                running(kind),
                said(msg::UI_MAINTENANCE_RUNNING, &[("kind", name(kind.wire()))]),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_run_ends_in_the_message_of_its_outcome_told_by_its_kind() {
        for (outcome, id) in [
            (Outcome::Finished, msg::UI_MAINTENANCE_FINISHED),
            (Outcome::Cancelled, msg::UI_MAINTENANCE_CANCELLED),
            (Outcome::Failed, msg::UI_MAINTENANCE_FAILED),
        ] {
            let ended = End {
                kind: Kind::Prune,
                outcome,
                politeness: Politeness::Polite,
            };
            assert_eq!(
                end(&ended),
                said(id, &[("kind", name("prune"))]),
                "{outcome:?}"
            );
        }
    }

    #[test]
    fn a_check_that_found_damage_says_how_many_places() {
        let ended = End {
            kind: Kind::Check,
            outcome: Outcome::Found {
                errors: 2,
                warnings: 5,
            },
            politeness: Politeness::Assertive,
        };
        assert_eq!(
            end(&ended),
            said(msg::UI_MAINTENANCE_CHECK_DAMAGED, &[("errors", number(2))])
        );
    }

    #[test]
    fn a_check_that_found_only_things_to_tidy_says_how_many() {
        let ended = End {
            kind: Kind::Check,
            outcome: Outcome::Found {
                errors: 0,
                warnings: 3,
            },
            politeness: Politeness::Polite,
        };
        assert_eq!(
            end(&ended),
            said(msg::UI_MAINTENANCE_CHECK_UNTIDY, &[("warnings", number(3))])
        );
    }

    #[test]
    fn a_list_of_findings_that_leaves_some_out_says_how_many() {
        assert_eq!(check_more(0), None);
        assert_eq!(
            check_more(4),
            Some(said(
                msg::UI_MAINTENANCE_CHECK_MORE,
                &[("count", number(4))]
            ))
        );
    }

    #[test]
    fn a_review_that_is_being_prepared_says_so() {
        assert_eq!(
            told(Told::Preparing),
            said(msg::UI_MAINTENANCE_PREPARING, &[])
        );
    }

    #[test]
    fn a_run_that_is_being_started_is_told_by_its_kind() {
        for kind in [Kind::Forget, Kind::Prune, Kind::Check, Kind::Adopt] {
            assert_eq!(
                told(Told::Starting(kind)),
                said(msg::UI_MAINTENANCE_STARTING, &[("kind", name(kind.wire()))]),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_run_the_page_stopped_watching_is_told_to_go_on_by_its_kind() {
        for kind in [Kind::Forget, Kind::Prune, Kind::Check, Kind::Adopt] {
            assert_eq!(
                told(Told::Unwatched(kind)),
                said(
                    msg::UI_MAINTENANCE_UNWATCHED,
                    &[("kind", name(kind.wire()))]
                ),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_run_that_goes_on_or_ended_is_told_as_it_is_by_itself() {
        assert_eq!(told(Told::Running(Kind::Check)), running(Kind::Check));
        let ended = End {
            kind: Kind::Forget,
            outcome: Outcome::Failed,
            politeness: Politeness::Assertive,
        };
        assert_eq!(told(Told::Ended(ended)), end(&ended));
    }

    #[test]
    fn a_forget_of_a_repository_no_plan_uses_says_so() {
        assert_eq!(
            fact(&Fact::NoPlans),
            vec![said(msg::UI_REVIEW_NO_PLANS, &[])]
        );
    }

    fn owners() -> Vec<(Owner<'static>, Vec<(&'static str, MsgArg)>)> {
        vec![
            (
                Owner::Both {
                    agent: "agent-a",
                    plan: "old",
                },
                vec![
                    ("owner", name("both")),
                    ("plan", name("old")),
                    ("agent", name("agent-a")),
                ],
            ),
            (
                Owner::Plan("old"),
                vec![("owner", name("plan")), ("plan", name("old"))],
            ),
            (
                Owner::Agent("agent-a"),
                vec![("owner", name("agent")), ("agent", name("agent-a"))],
            ),
            (Owner::Neither, vec![("owner", name("neither"))]),
        ]
    }

    #[test]
    fn an_orphaned_group_is_headed_by_one_message_that_takes_the_tags_it_carries() {
        for (carried, args) in owners() {
            assert_eq!(
                owner(carried),
                said(msg::UI_ORPHANED_GROUP, &args),
                "{carried:?}"
            );
        }
    }

    #[test]
    fn an_orphaned_groups_table_is_captioned_by_one_message_that_takes_the_same_tags() {
        for (carried, args) in owners() {
            assert_eq!(
                owner_caption(carried),
                said(msg::UI_ORPHANED_CAPTION, &args),
                "{carried:?}"
            );
        }
    }

    fn unmanaged_args_of(labeled: &str, label: &str) -> [(&'static str, MsgArg); 3] {
        [
            ("labeled", name(labeled)),
            ("hostname", name("laptop")),
            ("label", name(label)),
        ]
    }

    #[test]
    fn an_unmanaged_group_is_told_by_one_message_that_takes_its_host_and_label() {
        let group = unmanaged_group("docs");
        let args = unmanaged_args_of("yes", "docs");
        assert_eq!(unmanaged(&group), said(msg::UI_UNMANAGED_GROUP, &args));
        assert_eq!(
            unmanaged_caption(&group),
            said(msg::UI_UNMANAGED_CAPTION, &args)
        );
    }

    #[test]
    fn an_unmanaged_group_without_a_label_says_so_through_the_same_messages() {
        let group = unmanaged_group("");
        let args = unmanaged_args_of("no", "");
        assert_eq!(unmanaged(&group), said(msg::UI_UNMANAGED_GROUP, &args));
        assert_eq!(
            unmanaged_caption(&group),
            said(msg::UI_UNMANAGED_CAPTION, &args)
        );
    }

    #[test]
    fn an_orphaned_group_with_both_tags_is_adopted_by_a_button_naming_them() {
        let group = orphaned(Some("agent-a"), Some("old"));
        let adoption = adopt_orphaned(&group, "home");
        assert_eq!(
            adoption,
            Some(Adoption {
                label: said(
                    msg::UI_ADOPT_ORPHANED,
                    &[
                        ("plan", name("old")),
                        ("agent", name("agent-a")),
                        ("into", name("home")),
                    ]
                ),
                request: maintenance::adoption(
                    "home",
                    AdoptionFromDto::Orphaned {
                        agent: "agent-a".to_owned(),
                        plan: "old".to_owned(),
                    },
                ),
            })
        );
    }

    #[test]
    fn an_orphaned_group_that_lacks_a_tag_has_no_button() {
        assert_eq!(adopt_orphaned(&orphaned(None, Some("old")), "home"), None);
        assert_eq!(
            adopt_orphaned(&orphaned(Some("agent-a"), None), "home"),
            None
        );
        assert_eq!(adopt_orphaned(&orphaned(None, None), "home"), None);
    }

    #[test]
    fn a_group_that_cannot_be_adopted_says_why_in_a_message_of_its_own() {
        assert_eq!(
            orphaned_stuck(Stuck::Untagged),
            said(msg::UI_ORPHANED_UNTAGGED, &[])
        );
        assert_eq!(
            orphaned_stuck(Stuck::NoPlan),
            said(msg::UI_ORPHANED_NO_PLAN, &[])
        );
        assert_eq!(unmanaged_stuck(), said(msg::UI_UNMANAGED_NO_PLAN, &[]));
    }

    #[test]
    fn the_reasons_no_group_is_adopted_read_in_english() {
        assert_eq!(
            written(&orphaned_stuck(Stuck::Untagged)),
            "This history lacks the tag of an agent or a plan, so it cannot be adopted."
        );
        assert_eq!(
            written(&orphaned_stuck(Stuck::NoPlan)),
            "No plan of this agent could take this history, so it cannot be adopted."
        );
        assert_eq!(
            written(&unmanaged_stuck()),
            "No plan of this agent backs up this host, label and paths, so this history cannot be adopted."
        );
    }

    #[test]
    fn an_unmanaged_group_is_adopted_by_a_button_that_takes_its_host_label_and_the_plan() {
        let adoption = adopt_unmanaged(&unmanaged_group("docs"), "home");
        assert_eq!(
            adoption.label,
            said(
                msg::UI_ADOPT_UNMANAGED,
                &[
                    ("labeled", name("yes")),
                    ("hostname", name("laptop")),
                    ("label", name("docs")),
                    ("into", name("home")),
                ]
            )
        );
        assert_eq!(
            adoption.request,
            maintenance::adoption("home", AdoptionFromDto::Unmanaged)
        );
    }

    /// What a reader gets: the words as the catalog writes them in American
    /// English. A select whose key the code never sends falls to its default
    /// silently, so the words of each branch are pinned here.
    fn written(words: &Words) -> String {
        text_with(&Localizer::new(&[]), words.id, &words.args)
    }

    #[test]
    fn a_forget_counts_the_snapshots_of_a_plan_in_the_forms_english_has() {
        let removes = |remove, total| {
            let fact = Fact::Removes {
                plan: "home".to_owned(),
                remove,
                total,
            };
            written(&fact_words(&fact))
        };
        assert_eq!(removes(1, 1), "Plan home: its 1 snapshot is forgotten.");
        assert_eq!(
            removes(1, 3),
            "Plan home: 1 of its 3 snapshots is forgotten."
        );
        assert_eq!(
            removes(2, 3),
            "Plan home: 2 of its 3 snapshots are forgotten."
        );
        assert_eq!(
            removes(0, 1),
            "Plan home: nothing is forgotten among its 1 snapshot."
        );
        assert_eq!(
            removes(0, 3),
            "Plan home: nothing is forgotten among its 3 snapshots."
        );
    }

    fn fact_words(single: &Fact) -> Words {
        fact(single).remove(0)
    }

    #[test]
    fn a_list_that_leaves_snapshots_out_says_so_in_a_sentence_of_its_own() {
        let left_out = |count| written(&more(count).expect("snapshots are left out"));
        assert_eq!(left_out(1), "1 more snapshot is not listed.");
        assert_eq!(left_out(70), "70 more snapshots are not listed.");
    }

    #[test]
    fn every_kind_of_orphaned_group_reads_as_its_own_heading_and_caption() {
        let read = |carried| (written(&owner(carried)), written(&owner_caption(carried)));
        let both = Owner::Both {
            agent: "agent-a",
            plan: "old",
        };
        assert_eq!(
            read(both),
            (
                "Plan old of agent agent-a".to_owned(),
                "Snapshots of plan old of agent agent-a".to_owned()
            )
        );
        assert_eq!(
            read(Owner::Plan("old")),
            (
                "Plan old of an unknown agent".to_owned(),
                "Snapshots of plan old of an unknown agent".to_owned()
            )
        );
        assert_eq!(
            read(Owner::Agent("agent-a")),
            (
                "An unknown plan of agent agent-a".to_owned(),
                "Snapshots of an unknown plan of agent agent-a".to_owned()
            )
        );
        assert_eq!(
            read(Owner::Neither),
            (
                "History of an unknown agent and plan".to_owned(),
                "Snapshots of an unknown agent and plan".to_owned()
            )
        );
    }

    #[test]
    fn an_unmanaged_group_reads_with_its_label_or_without_one() {
        let read = |label| {
            let group = unmanaged_group(label);
            (
                written(&unmanaged(&group)),
                written(&unmanaged_caption(&group)),
                written(&adopt_unmanaged(&group, "home").label),
            )
        };
        assert_eq!(
            read("docs"),
            (
                "Host laptop, label docs".to_owned(),
                "Snapshots of host laptop, label docs".to_owned(),
                "Adopt history of host laptop, label docs into plan home".to_owned()
            )
        );
        assert_eq!(
            read(""),
            (
                "Host laptop, no label".to_owned(),
                "Snapshots of host laptop, no label".to_owned(),
                "Adopt history of host laptop, no label into plan home".to_owned()
            )
        );
    }

    #[test]
    fn what_the_page_tells_of_a_run_reads_by_what_the_run_is() {
        let read = |kind| {
            (
                written(&told(Told::Starting(kind))),
                written(&told(Told::Running(kind))),
                written(&told(Told::Unwatched(kind))),
            )
        };
        let on = "which goes on. The Operations page shows how it ends.";
        for (kind, starting, running, watching) in [
            (
                Kind::Forget,
                "Starting the forget…",
                "A forget is running.",
                "the forget",
            ),
            (
                Kind::Prune,
                "Starting the prune…",
                "A prune is running.",
                "the prune",
            ),
            (
                Kind::Check,
                "Starting the check…",
                "A check is running.",
                "the check",
            ),
            (
                Kind::Adopt,
                "Starting the adoption…",
                "An adoption is running.",
                "the adoption",
            ),
        ] {
            let unwatched = format!("This page stopped watching {watching}, {on}");
            assert_eq!(
                read(kind),
                (starting.to_owned(), running.to_owned(), unwatched),
                "{kind:?}"
            );
        }
    }
}
