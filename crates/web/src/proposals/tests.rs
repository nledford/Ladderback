use api::{
    ErrorCode, ForgetPlanDto, ForgetSideDto, MsgArg, PlanSettingDto, ProposalDto,
    ProposalReviewDto, ProposalStatusDto, SummaryLineDto,
};
use l10n::msg;

use super::*;

fn proposal(id: &str, status: ProposalStatusDto, proposed_by: Option<&str>) -> ProposalDto {
    ProposalDto {
        id: id.to_owned(),
        proposed_by: proposed_by.map(str::to_owned),
        status,
        created_at: "2026-11-16T09:00:00Z".to_owned(),
        expires_at: "2026-11-18T09:00:00Z".to_owned(),
        changes: Vec::new(),
        rationale: "Nightly backups.".to_owned(),
        decided_at: None,
        reasons: Vec::new(),
        warnings: Vec::new(),
    }
}

fn review(needs_forget_preview: bool) -> ProposalReviewDto {
    ProposalReviewDto {
        proposal: proposal("abcdefghijkl", ProposalStatusDto::Pending, None),
        summary: Vec::new(),
        diff: "+[[plans]]\n".to_owned(),
        revision: "1".repeat(64),
        diff_hash: "2".repeat(64),
        changed_since_drafted: false,
        needs_forget_preview,
        rejected: 2,
        short_id: "abcdef".to_owned(),
    }
}

#[test]
fn the_page_lists_only_the_pending_proposals_in_the_daemons_order() {
    let listed = pending(vec![
        proposal("newest", ProposalStatusDto::Pending, None),
        proposal("applied", ProposalStatusDto::Applied, None),
        proposal("applying", ProposalStatusDto::Applying, None),
        proposal("stale", ProposalStatusDto::Stale, None),
        proposal("oldest", ProposalStatusDto::Pending, None),
    ]);
    let ids: Vec<&str> = listed.iter().map(|one| one.id.as_str()).collect();
    assert_eq!(ids, ["newest", "oldest"]);
}

#[test]
fn a_proposals_button_names_it_and_who_proposed_it() {
    assert_eq!(
        review_label(&proposal(
            "abc",
            ProposalStatusDto::Pending,
            Some("assistant")
        )),
        (
            msg::UI_PROPOSALS_REVIEW,
            vec![
                ("proposal", MsgArg::Text("abc".to_owned())),
                ("proposer", MsgArg::Text("assistant".to_owned())),
            ]
        )
    );
    assert_eq!(
        review_label(&proposal("abc", ProposalStatusDto::Pending, None)),
        (
            msg::UI_PROPOSALS_REVIEW_NO_TOKEN,
            vec![("proposal", MsgArg::Text("abc".to_owned()))]
        )
    );
}

#[test]
fn the_routes_of_a_proposal_are_under_its_encoded_id() {
    assert_eq!(review_path("abc"), "/api/v1/proposals/abc/review");
    assert_eq!(approve_path("abc"), "/api/v1/proposals/abc/approve");
    assert_eq!(reject_path("a/b"), "/api/v1/proposals/a%2Fb/reject");
    assert_eq!(
        forget_preview_path("a/b"),
        "/api/v1/proposals/a%2Fb/forget-preview"
    );
}

/// Focus returns to the button whose `id` this is once its proposal is
/// decided, so each proposal's must be its own and usable as an HTML `id`.
#[test]
fn each_proposals_review_button_has_an_id_of_its_own() {
    let first = review_id("0192abcd");
    let second = review_id("0192abce");
    assert_ne!(first, second);
    for id in [&first, &second] {
        assert!(id.starts_with(|c: char| c.is_ascii_alphabetic()), "{id}");
        assert!(!id.contains(char::is_whitespace), "{id}");
    }
    assert!(first.ends_with("0192abcd"), "{first}");
}

#[test]
fn each_summary_line_names_its_plan_and_a_change_lists_its_settings() {
    let plan = || vec![("plan", MsgArg::Text("home".to_owned()))];
    assert_eq!(
        summary_line(&SummaryLineDto::AddsPlan {
            plan: "home".to_owned()
        }),
        ((msg::UI_PROPOSAL_ADDS_PLAN, plan()), Vec::new())
    );
    assert_eq!(
        summary_line(&SummaryLineDto::RemovesPlan {
            plan: "home".to_owned()
        }),
        ((msg::UI_PROPOSAL_REMOVES_PLAN, plan()), Vec::new())
    );
    assert_eq!(
        summary_line(&SummaryLineDto::ChangesPlan {
            plan: "home".to_owned(),
            settings: vec![PlanSettingDto::Schedule, PlanSettingDto::Retention],
        }),
        (
            (msg::UI_PROPOSAL_CHANGES_PLAN, plan()),
            vec![
                msg::UI_PROPOSAL_SETTING_SCHEDULE,
                msg::UI_PROPOSAL_SETTING_RETENTION
            ]
        )
    );
}

#[test]
fn every_setting_has_a_name_of_its_own() {
    let settings = [
        PlanSettingDto::Repository,
        PlanSettingDto::Host,
        PlanSettingDto::Label,
        PlanSettingDto::Sources,
        PlanSettingDto::Tags,
        PlanSettingDto::Schedule,
        PlanSettingDto::Overlap,
        PlanSettingDto::Retry,
        PlanSettingDto::Filter,
        PlanSettingDto::Retention,
    ];
    let names: std::collections::HashSet<_> = settings.into_iter().map(setting_name).collect();
    assert_eq!(names.len(), settings.len());
}

#[test]
fn approve_sends_what_the_page_showed_with_the_confirmation() {
    let approved = approval(&review(false), "abcdef".to_owned()).expect("approve is offered");
    assert_eq!(
        (
            approved.revision.as_str(),
            approved.diff_hash.as_str(),
            approved.confirmation.as_str()
        ),
        ("1".repeat(64).as_str(), "2".repeat(64).as_str(), "abcdef")
    );
}

#[test]
fn a_proposal_that_could_delete_snapshots_is_only_rejected_until_the_forget_preview() {
    assert!(can_approve(&review(false)));
    assert!(!can_approve(&review(true)));
    assert_eq!(approval(&review(true), "abcdef".to_owned()), None);
}

#[test]
fn reject_sends_a_reason_for_each_change_with_blank_ones_given_none() {
    let sent = rejection(&[
        "  Back up weekly instead ".to_owned(),
        "   ".to_owned(),
        String::new(),
    ]);
    assert_eq!(sent.reasons, ["Back up weekly instead", "", ""]);
}

#[test]
fn a_wrong_confirmation_is_said_in_the_field_by_what_it_asked_for() {
    assert_eq!(
        field_error(ErrorCode::StepUpFailed, Asks::ShortId, "abcdef"),
        Some((
            msg::UI_PROPOSAL_WRONG_ID,
            vec![("length", MsgArg::Count(6))]
        ))
    );
    assert_eq!(
        field_error(ErrorCode::StepUpFailed, Asks::Password, "abcdef"),
        Some((msg::UI_PROPOSAL_WRONG_PASSWORD, Vec::new()))
    );
    assert_eq!(
        field_error(ErrorCode::ProposalChanged, Asks::Password, "abcdef"),
        None
    );
}

#[test]
fn the_proposers_rejections_are_counted_beside_the_proposal() {
    assert_eq!(
        rejected_before(&review(false)),
        (
            msg::UI_PROPOSAL_REJECTED_BEFORE,
            vec![("count", MsgArg::Count(2))]
        )
    );
}

#[test]
fn a_reasons_field_is_named_for_its_change_from_1() {
    assert_eq!(
        reason_label(1),
        (msg::UI_PROPOSAL_REASON, vec![("change", MsgArg::Count(1))])
    );
}

fn snapshot(id: char) -> SnapshotDto {
    SnapshotDto {
        id: id.to_string().repeat(64),
        time: "2026-11-15T09:00:00Z".to_owned(),
        offset: None,
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: Vec::new(),
        tags_truncated: false,
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
    }
}

/// The plan `plan`, which owns `total` snapshots and whose retention lets
/// `remove` go, of which `listed` are named.
fn forgets(plan: &str, total: u64, remove: u64, listed: &[char]) -> ForgetPlanDto {
    ForgetPlanDto {
        plan: plan.to_owned(),
        total,
        remove,
        listed: listed.iter().copied().map(snapshot).collect(),
    }
}

/// A side that is maintained with the plans `plans`, or else not
/// maintained.
fn side(plans: Option<Vec<ForgetPlanDto>>) -> ForgetSideDto {
    plans.map_or(ForgetSideDto::NotMaintained, |plans| {
        ForgetSideDto::Forgets { plans }
    })
}

fn compared(
    before: Option<Vec<ForgetPlanDto>>,
    after: Option<Vec<ForgetPlanDto>>,
    forgets_more: Vec<ForgetPlanDto>,
    forgets_less: Vec<ForgetPlanDto>,
) -> RepositoryForgetDto {
    compared_sides(side(before), side(after), forgets_more, forgets_less)
}

fn compared_sides(
    before: ForgetSideDto,
    after: ForgetSideDto,
    forgets_more: Vec<ForgetPlanDto>,
    forgets_less: Vec<ForgetPlanDto>,
) -> RepositoryForgetDto {
    RepositoryForgetDto::Compared {
        repository: "demo".to_owned(),
        before,
        after,
        forgets_more,
        forgets_less,
    }
}

fn plan_line(message: MessageId, plan: &str, remove: u64, total: u64) -> Said {
    (
        message,
        vec![
            ("plan", MsgArg::Text(plan.to_owned())),
            ("remove", MsgArg::Count(remove)),
            ("total", MsgArg::Count(total)),
        ],
    )
}

fn caption(message: MessageId, plan: &str, count: u64) -> Said {
    (
        message,
        vec![
            ("plan", MsgArg::Text(plan.to_owned())),
            ("count", MsgArg::Count(count)),
        ],
    )
}

/// Each side says what retention forgets of each of its plans, before
/// then after, and each difference captions the snapshots it names, those
/// forgotten only after first; what the daemon didn't list is counted.
#[test]
fn a_forget_preview_says_each_side_by_plan_then_names_what_differs() {
    let shown = repository_shown(&compared(
        Some(vec![
            forgets("home", 4, 1, &['a']),
            forgets("docs", 2, 0, &[]),
        ]),
        Some(vec![forgets("home", 4, 3, &['a', 'b'])]),
        vec![forgets("home", 4, 2, &['b'])],
        vec![forgets("docs", 2, 1, &['c'])],
    ));
    assert_eq!(
        shown,
        RepositoryShown::Compared {
            repository: "demo".to_owned(),
            lines: vec![
                plan_line(msg::UI_PROPOSAL_FORGET_BEFORE, "home", 1, 4),
                plan_line(msg::UI_PROPOSAL_FORGET_BEFORE, "docs", 0, 2),
                plan_line(msg::UI_PROPOSAL_FORGET_AFTER, "home", 3, 4),
            ],
            listings: vec![
                ForgetListing {
                    caption: caption(msg::UI_PROPOSAL_FORGETS_MORE, "home", 2),
                    snapshots: vec![snapshot('b')],
                    more: Some((msg::UI_SNAPSHOTS_MORE, vec![("count", MsgArg::Count(1))])),
                },
                ForgetListing {
                    caption: caption(msg::UI_PROPOSAL_FORGETS_LESS, "docs", 1),
                    snapshots: vec![snapshot('c')],
                    more: None,
                },
            ],
        }
    );
}

#[test]
fn a_proposal_that_forgets_neither_more_nor_less_says_so() {
    let home = || Some(vec![forgets("home", 3, 1, &['a'])]);
    let RepositoryShown::Compared {
        lines, listings, ..
    } = repository_shown(&compared(home(), home(), Vec::new(), Vec::new()))
    else {
        panic!("compared");
    };
    assert_eq!(
        lines.last(),
        Some(&(msg::UI_PROPOSAL_FORGET_SAME, Vec::new()))
    );
    assert!(listings.is_empty());
    // Something forgotten less is a change: no such line.
    let RepositoryShown::Compared { lines, .. } = repository_shown(&compared(
        home(),
        Some(Vec::new()),
        Vec::new(),
        vec![forgets("home", 3, 1, &['a'])],
    )) else {
        panic!("compared");
    };
    assert!(
        !lines.contains(&(msg::UI_PROPOSAL_FORGET_SAME, Vec::new())),
        "{lines:?}"
    );
}

/// A side this agent doesn't maintain the repository under, or where no
/// plan owns snapshots, forgets nothing and says why; maintained on
/// neither side, the repository says so once.
#[test]
fn a_side_that_forgets_nothing_says_why() {
    let lines =
        |before, after| match repository_shown(&compared(before, after, Vec::new(), Vec::new())) {
            RepositoryShown::Compared { lines, .. } => lines,
            refused => panic!("compared, not {refused:?}"),
        };
    let said = |message| (message, Vec::new());
    assert_eq!(
        lines(None, None),
        [said(msg::UI_PROPOSAL_FORGET_NOT_MAINTAINED)]
    );
    assert_eq!(
        lines(None, Some(Vec::new())),
        [
            said(msg::UI_PROPOSAL_FORGET_BEFORE_NOT_MAINTAINED),
            said(msg::UI_PROPOSAL_FORGET_AFTER_NO_SNAPSHOTS),
            said(msg::UI_PROPOSAL_FORGET_SAME),
        ]
    );
    assert_eq!(
        lines(Some(Vec::new()), None),
        [
            said(msg::UI_PROPOSAL_FORGET_BEFORE_NO_SNAPSHOTS),
            said(msg::UI_PROPOSAL_FORGET_AFTER_NOT_MAINTAINED),
            said(msg::UI_PROPOSAL_FORGET_SAME),
        ]
    );
}

/// A side whose forget would be refused, because a plan's policy keeps
/// nothing, forgets nothing and says which plan.
#[test]
fn a_side_whose_forget_would_be_refused_says_which_plan_keeps_nothing() {
    let refused = || ForgetSideDto::KeepsNothing {
        plan: "home".to_owned(),
    };
    let home = || ForgetSideDto::Forgets {
        plans: vec![forgets("home", 3, 2, &['a', 'b'])],
    };
    let lines = |before, after| match repository_shown(&compared_sides(
        before,
        after,
        Vec::new(),
        Vec::new(),
    )) {
        RepositoryShown::Compared { lines, .. } => lines,
        refused => panic!("compared, not {refused:?}"),
    };
    let keeps_nothing = |message| (message, vec![("plan", MsgArg::Text("home".to_owned()))]);
    assert_eq!(
        lines(refused(), home())[0],
        keeps_nothing(msg::UI_PROPOSAL_FORGET_BEFORE_KEEPS_NOTHING)
    );
    assert_eq!(
        lines(home(), refused())[1],
        keeps_nothing(msg::UI_PROPOSAL_FORGET_AFTER_KEEPS_NOTHING)
    );
}

#[test]
fn a_repository_that_could_not_be_read_is_said_by_its_refusal() {
    let error = ErrorDto::new(ErrorCode::RepositoryReadTimedOut, Vec::new(), None);
    assert_eq!(
        repository_shown(&RepositoryForgetDto::Refused {
            repository: "demo".to_owned(),
            error: error.clone(),
        }),
        RepositoryShown::Refused {
            repository: "demo".to_owned(),
            error,
        }
    );
}
