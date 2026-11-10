//! What the Snapshots page says (technical design §4.4, §8.4): for each thing
//! a review or a run has to tell, the catalog message that tells it and the
//! values its variables read. The page asks the localizer for the text;
//! which message, and with which arguments, is decided here, on the host.

use api::{AdoptionFromDto, MaintenanceRequestDto, MsgArg, OrphanedGroupDto, UnmanagedGroupDto};
use l10n::{MessageId, msg};

use crate::maintenance::{self, Confirm, End, Fact, Kind, Outcome, Owner, Review, Wait};

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
pub(crate) fn running(kind: Kind) -> Words {
    words(
        msg::UI_MAINTENANCE_RUNNING,
        vec![("kind", text(kind.wire()))],
    )
}

/// How a run's end is told, by what ran and how it ended.
pub(crate) fn end(end: &End) -> Words {
    let id = match end.outcome {
        Outcome::Finished => msg::UI_MAINTENANCE_FINISHED,
        Outcome::Cancelled => msg::UI_MAINTENANCE_CANCELLED,
        Outcome::Failed => msg::UI_MAINTENANCE_FAILED,
    };
    words(id, vec![("kind", text(end.kind.wire()))])
}

/// The heading of an orphaned group, by the tags it carries.
pub(crate) fn owner(owner: Owner<'_>) -> Words {
    match owner {
        Owner::Both { agent, plan } => words(
            msg::UI_ORPHANED_BOTH,
            vec![("plan", text(plan)), ("agent", text(agent))],
        ),
        Owner::Plan(plan) => words(msg::UI_ORPHANED_PLAN, vec![("plan", text(plan))]),
        Owner::Agent(agent) => words(msg::UI_ORPHANED_AGENT, vec![("agent", text(agent))]),
        Owner::Neither => words(msg::UI_ORPHANED_NEITHER, Vec::new()),
    }
}

/// The heading of an unmanaged group: its host, and its label when it has
/// one.
pub(crate) fn unmanaged(group: &UnmanagedGroupDto) -> Words {
    if group.label.is_empty() {
        words(
            msg::UI_UNMANAGED_UNLABELED,
            vec![("hostname", text(&group.hostname))],
        )
    } else {
        words(
            msg::UI_UNMANAGED_LABELED,
            vec![
                ("hostname", text(&group.hostname)),
                ("label", text(&group.label)),
            ],
        )
    }
}

/// The paths an unmanaged group backed up, as one list.
pub(crate) fn paths(paths: &[String]) -> Words {
    words(
        msg::UI_UNMANAGED_PATHS,
        vec![("paths", text(&paths.join(", ")))],
    )
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

/// The button that adopts an unmanaged group, named by its heading `group`,
/// into the plan `into`, with its request.
pub(crate) fn adopt_unmanaged(group: &str, into: &str) -> Adoption {
    Adoption {
        label: words(
            msg::UI_ADOPT_UNMANAGED,
            vec![("group", text(group)), ("into", text(into))],
        ),
        request: maintenance::adoption(into, AdoptionFromDto::Unmanaged),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::announcer::Politeness;
    use api::SnapshotDto;

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
    fn an_orphaned_group_is_headed_by_the_tags_it_carries() {
        assert_eq!(
            owner(Owner::Both {
                agent: "agent-a",
                plan: "old"
            }),
            said(
                msg::UI_ORPHANED_BOTH,
                &[("plan", name("old")), ("agent", name("agent-a"))]
            )
        );
        assert_eq!(
            owner(Owner::Plan("old")),
            said(msg::UI_ORPHANED_PLAN, &[("plan", name("old"))])
        );
        assert_eq!(
            owner(Owner::Agent("agent-a")),
            said(msg::UI_ORPHANED_AGENT, &[("agent", name("agent-a"))])
        );
        assert_eq!(owner(Owner::Neither), said(msg::UI_ORPHANED_NEITHER, &[]));
    }

    #[test]
    fn an_unmanaged_group_with_a_label_is_headed_by_its_host_and_label() {
        assert_eq!(
            unmanaged(&unmanaged_group("docs")),
            said(
                msg::UI_UNMANAGED_LABELED,
                &[("hostname", name("laptop")), ("label", name("docs"))]
            )
        );
    }

    #[test]
    fn an_unmanaged_group_without_a_label_is_headed_by_its_host_alone() {
        assert_eq!(
            unmanaged(&unmanaged_group("")),
            said(msg::UI_UNMANAGED_UNLABELED, &[("hostname", name("laptop"))])
        );
    }

    #[test]
    fn the_paths_of_an_unmanaged_group_are_one_list() {
        assert_eq!(
            paths(&["/home".to_owned(), "/etc".to_owned()]),
            said(msg::UI_UNMANAGED_PATHS, &[("paths", name("/home, /etc"))])
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
    fn an_unmanaged_group_is_adopted_by_a_button_naming_its_heading_and_the_plan() {
        let adoption = adopt_unmanaged("Host laptop, no label", "home");
        assert_eq!(
            adoption.label,
            said(
                msg::UI_ADOPT_UNMANAGED,
                &[
                    ("group", name("Host laptop, no label")),
                    ("into", name("home")),
                ]
            )
        );
        assert_eq!(
            adoption.request,
            maintenance::adoption("home", AdoptionFromDto::Unmanaged)
        );
    }
}
