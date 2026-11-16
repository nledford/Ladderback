//! What the "Pending changes" page shows, sends and says (decision 89,
//! point 6), decided on the host: which proposals it lists, the words of
//! each summary line, whether a proposal can be approved here yet, the
//! requests Approve and Reject send, and what a refusal of the step-up
//! field is said as. `proposals_page.rs` is its glue.

use api::{
    ApproveProposalDto, ErrorCode, MsgArg, PlanSettingDto, ProposalDto, ProposalReviewDto,
    ProposalStatusDto, RejectProposalDto, SummaryLineDto,
};
use l10n::{MessageId, msg};

use crate::components::tree::rules::encoded;
use crate::plans::Said;
use crate::restore::Asks;

/// The route that lists the proposals.
pub(crate) const PROPOSALS_PATH: &str = "/api/v1/proposals";

/// The route of the proposal `id`.
fn proposal_path(id: &str) -> String {
    format!("{PROPOSALS_PATH}/{}", encoded(id))
}

/// The route that answers the proposal `id` as the page shows it.
pub(crate) fn review_path(id: &str) -> String {
    format!("{}/review", proposal_path(id))
}

/// The route that approves the proposal `id`.
pub(crate) fn approve_path(id: &str) -> String {
    format!("{}/approve", proposal_path(id))
}

/// The route that rejects the proposal `id`.
pub(crate) fn reject_path(id: &str) -> String {
    format!("{}/reject", proposal_path(id))
}

/// The proposals a person can still decide, in the order the daemon lists
/// them (newest first): the pending ones. Every other status has ended, or
/// is being applied, and is history.
pub(crate) fn pending(proposals: Vec<ProposalDto>) -> Vec<ProposalDto> {
    proposals
        .into_iter()
        .filter(|proposal| proposal.status == ProposalStatusDto::Pending)
        .collect()
}

/// The button that opens the proposal `proposal`, named for it and for
/// who proposed it: the API token's label, or no token.
pub(crate) fn review_label(proposal: &ProposalDto) -> Said {
    let id = ("proposal", MsgArg::Text(proposal.id.clone()));
    match &proposal.proposed_by {
        Some(label) => (
            msg::UI_PROPOSALS_REVIEW,
            vec![id, ("proposer", MsgArg::Text(label.clone()))],
        ),
        None => (msg::UI_PROPOSALS_REVIEW_NO_TOKEN, vec![id]),
    }
}

/// The `id` of the button that opens the proposal `id`, where focus returns
/// once it is decided.
pub(crate) fn review_id(id: &str) -> String {
    format!("proposal-review-{id}")
}

/// A summary line's sentence, and for a changed plan the names of the
/// settings it changes, listed under it.
pub(crate) fn summary_line(line: &SummaryLineDto) -> (Said, Vec<MessageId>) {
    let (message, plan, settings) = match line {
        SummaryLineDto::AddsPlan { plan } => (msg::UI_PROPOSAL_ADDS_PLAN, plan, Vec::new()),
        SummaryLineDto::ChangesPlan { plan, settings } => (
            msg::UI_PROPOSAL_CHANGES_PLAN,
            plan,
            settings.iter().copied().map(setting_name).collect(),
        ),
        SummaryLineDto::RemovesPlan { plan } => (msg::UI_PROPOSAL_REMOVES_PLAN, plan, Vec::new()),
    };
    (
        (message, vec![("plan", MsgArg::Text(plan.clone()))]),
        settings,
    )
}

/// The name of a plan's setting, as a summary line lists it.
fn setting_name(setting: PlanSettingDto) -> MessageId {
    match setting {
        PlanSettingDto::Repository => msg::UI_PROPOSAL_SETTING_REPOSITORY,
        PlanSettingDto::Host => msg::UI_PROPOSAL_SETTING_HOST,
        PlanSettingDto::Label => msg::UI_PROPOSAL_SETTING_LABEL,
        PlanSettingDto::Sources => msg::UI_PROPOSAL_SETTING_SOURCES,
        PlanSettingDto::Tags => msg::UI_PROPOSAL_SETTING_TAGS,
        PlanSettingDto::Schedule => msg::UI_PROPOSAL_SETTING_SCHEDULE,
        PlanSettingDto::Overlap => msg::UI_PROPOSAL_SETTING_OVERLAP,
        PlanSettingDto::Retry => msg::UI_PROPOSAL_SETTING_RETRY,
        PlanSettingDto::Filter => msg::UI_PROPOSAL_SETTING_FILTER,
        PlanSettingDto::Retention => msg::UI_PROPOSAL_SETTING_RETENTION,
    }
}

/// How often a person rejected the proposer's proposals, said beside the
/// proposal (decision 89, point 6).
pub(crate) fn rejected_before(review: &ProposalReviewDto) -> Said {
    (
        msg::UI_PROPOSAL_REJECTED_BEFORE,
        vec![("count", MsgArg::Count(review.rejected))],
    )
}

/// Whether the page offers Approve for `review`: not for a proposal that
/// could delete snapshots, which only the forget preview lets a person
/// approve (decision 89, point 8); until then it is only rejected, and the
/// page says why.
pub(crate) fn can_approve(review: &ProposalReviewDto) -> bool {
    !review.needs_forget_preview
}

/// The request Approve sends: what the page showed, the file's revision
/// and the diff's hash, with the step-up `confirmation`. `None` when the
/// page doesn't offer Approve.
pub(crate) fn approval(
    review: &ProposalReviewDto,
    confirmation: String,
) -> Option<ApproveProposalDto> {
    can_approve(review).then(|| ApproveProposalDto {
        revision: review.revision.clone(),
        diff_hash: review.diff_hash.clone(),
        confirmation,
    })
}

/// The request Reject sends: the reason typed for each change, in order,
/// the spaces around it dropped, so a field left blank or holding only
/// spaces gives that change no reason.
pub(crate) fn rejection(reasons: &[String]) -> RejectProposalDto {
    RejectProposalDto {
        reasons: reasons
            .iter()
            .map(|reason| reason.trim().to_owned())
            .collect(),
    }
}

/// What a refusal of an approval is said as in the step-up field, when it
/// is about what was typed there; any other refusal is the page's alert.
pub(crate) fn field_error(code: ErrorCode, asks: Asks, short_id: &str) -> Option<Said> {
    (code == ErrorCode::StepUpFailed).then(|| match asks {
        Asks::ShortId => (
            msg::UI_PROPOSAL_WRONG_ID,
            vec![(
                "length",
                MsgArg::Count(u64::try_from(short_id.chars().count()).unwrap_or(u64::MAX)),
            )],
        ),
        Asks::Password => (msg::UI_PROPOSAL_WRONG_PASSWORD, Vec::new()),
    })
}

/// A message about the proposal `id`.
fn about(message: MessageId, id: &str) -> Said {
    (message, vec![("proposal", MsgArg::Text(id.to_owned()))])
}

/// The heading of the proposal `id` as the page shows it.
pub(crate) fn heading(id: &str) -> Said {
    about(msg::UI_PROPOSAL_HEADING, id)
}

/// What is announced once the proposal `id` is applied.
pub(crate) fn applied(id: &str) -> Said {
    about(msg::UI_PROPOSAL_APPLIED, id)
}

/// What is announced once the proposal `id` is rejected.
pub(crate) fn rejected(id: &str) -> Said {
    about(msg::UI_PROPOSAL_REJECTED, id)
}

/// The field for the reason of change `change`, counted from 1.
pub(crate) fn reason_label(change: usize) -> Said {
    (
        msg::UI_PROPOSAL_REASON,
        vec![(
            "change",
            MsgArg::Count(u64::try_from(change).unwrap_or(u64::MAX)),
        )],
    )
}

#[cfg(test)]
mod tests;
