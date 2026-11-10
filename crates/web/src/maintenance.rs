//! The decisions behind the Snapshots page's maintenance (technical design
//! §4.4): the request each button sends, what a preview tells a person before
//! they confirm, how many rows a list shows, and how a run's end is
//! announced. Every answer is data; `routes.rs` words it from the catalog.
//! Host-tested here; the DOM glue that sends the requests is `routes.rs`.

use api::{AdoptionFromDto, MaintenancePreviewDto, MaintenanceRequestDto};

use crate::announcer::Politeness;

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
    #[allow(clippy::todo, reason = "red test: the wire texts come next")]
    pub(crate) fn wire(self) -> &'static str {
        todo!("the request's kind text")
    }

    /// The kind a request's text names; the exact lower-case text only.
    #[allow(clippy::todo, reason = "red test: the wire texts come next")]
    pub(crate) fn of(text: &str) -> Option<Kind> {
        let _ = text;
        todo!("the kind a text names")
    }
}

/// The request that starts or previews `kind`: its kind text and nothing
/// else. An adoption names its plan and group, so it has [`adoption`].
#[allow(clippy::todo, reason = "red test: the request builders come next")]
pub(crate) fn request(kind: Kind) -> MaintenanceRequestDto {
    let _ = kind;
    todo!("a request with only a kind")
}

/// The request that adopts the group `from` into the plan `into`.
#[allow(clippy::todo, reason = "red test: the request builders come next")]
pub(crate) fn adoption(into: &str, from: AdoptionFromDto) -> MaintenanceRequestDto {
    let _ = (into, from);
    todo!("an adoption request")
}

/// The route that reads a repository's history, grouped by owner.
#[allow(clippy::todo, reason = "red test: the route paths come next")]
pub(crate) fn groups_path(alias: &str) -> String {
    let _ = alias;
    todo!("the groups route")
}

/// The route that previews a maintenance run.
#[allow(clippy::todo, reason = "red test: the route paths come next")]
pub(crate) fn preview_path(alias: &str) -> String {
    let _ = alias;
    todo!("the preview route")
}

/// The route that starts a maintenance run.
#[allow(clippy::todo, reason = "red test: the route paths come next")]
pub(crate) fn start_path(alias: &str) -> String {
    let _ = alias;
    todo!("the start route")
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
    #[allow(clippy::todo, reason = "red test: the wait's unit comes next")]
    pub(crate) fn of(hours: u64) -> Wait {
        let _ = hours;
        todo!("days or hours")
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
    /// What confirming commits to; `None` when there is nothing to do.
    pub(crate) confirm: Option<Confirm>,
    /// The token the preview named, for a forget or an adoption.
    pub(crate) selection: Option<String>,
}

impl Review {
    /// The review of `preview`, the answer to `request`.
    #[allow(clippy::todo, reason = "red test: the review's facts come next")]
    pub(crate) fn of(request: &MaintenanceRequestDto, preview: &MaintenancePreviewDto) -> Review {
        let _ = (request, preview);
        todo!("the facts and confirmation of a preview")
    }

    /// The request that confirming sends: the previewed request carrying the
    /// preview's token for a forget or an adoption, and none for a prune or
    /// a check.
    #[allow(clippy::todo, reason = "red test: the start body comes next")]
    pub(crate) fn start_body(&self) -> MaintenanceRequestDto {
        todo!("the request that starts the run")
    }

    /// Whether there is nothing to confirm.
    #[allow(clippy::todo, reason = "red test: the confirmation comes next")]
    pub(crate) fn nothing_to_do(&self) -> bool {
        todo!("no confirmation")
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
    #[allow(clippy::todo, reason = "red test: the row counts come next")]
    pub(crate) fn of(count: u64, listed: usize) -> Rows {
        let _ = (count, listed);
        todo!("shown and more")
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
    #[allow(clippy::todo, reason = "red test: the announcement comes next")]
    pub(crate) fn of(kind: Kind, status: &str) -> End {
        let _ = (kind, status);
        todo!("how the end is announced")
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
}
