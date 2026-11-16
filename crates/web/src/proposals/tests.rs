use api::{
    ErrorCode, MsgArg, PlanSettingDto, ProposalDto, ProposalReviewDto, ProposalStatusDto,
    SummaryLineDto,
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
