//! What the "Pending changes" page shows, sends and says (decision 89,
//! point 6), decided on the host: which proposals it lists, the words of
//! each summary line, whether a proposal can be approved here yet, the
//! requests Approve and Reject send, and what a refusal of the step-up
//! field is said as. `proposals_page.rs` is its glue.

use api::{
    ApproveProposalDto, ErrorCode, PlanSettingDto, ProposalDto, ProposalReviewDto,
    RejectProposalDto, SummaryLineDto,
};
use l10n::MessageId;

use crate::plans::Said;
use crate::restore::Asks;

/// The route that lists the proposals.
pub(crate) const PROPOSALS_PATH: &str = "/api/v1/proposals";

/// The route that answers the proposal `id` as the page shows it.
pub(crate) fn review_path(id: &str) -> String {
    let _ = id;
    todo!()
}

/// The route that approves the proposal `id`.
pub(crate) fn approve_path(id: &str) -> String {
    let _ = id;
    todo!()
}

/// The route that rejects the proposal `id`.
pub(crate) fn reject_path(id: &str) -> String {
    let _ = id;
    todo!()
}

/// The proposals a person can still decide, in the order the daemon lists
/// them (newest first): the pending ones.
pub(crate) fn pending(proposals: Vec<ProposalDto>) -> Vec<ProposalDto> {
    let _ = proposals;
    todo!()
}

/// The button that opens the proposal `proposal`, named for it and for
/// who proposed it: the API token's label, or no token.
pub(crate) fn review_label(proposal: &ProposalDto) -> Said {
    let _ = proposal;
    todo!()
}

/// The `id` of the button that opens the proposal `id`, where focus returns
/// once it is decided.
pub(crate) fn review_id(id: &str) -> String {
    let _ = id;
    todo!()
}

/// A summary line's sentence, and for a changed plan the names of the
/// settings it changes, listed under it.
pub(crate) fn summary_line(line: &SummaryLineDto) -> (Said, Vec<MessageId>) {
    let _ = line;
    todo!()
}

/// The name of a plan's setting, as a summary line lists it.
fn setting_name(setting: PlanSettingDto) -> MessageId {
    let _ = setting;
    todo!()
}

/// How often a person rejected the proposer's proposals, said beside the
/// proposal (decision 89, point 6).
pub(crate) fn rejected_before(review: &ProposalReviewDto) -> Said {
    let _ = review;
    todo!()
}

/// Whether the page offers Approve for `review`.
pub(crate) fn can_approve(review: &ProposalReviewDto) -> bool {
    let _ = review;
    todo!()
}

/// The request Approve sends, with the step-up `confirmation`. `None` when
/// the page doesn't offer Approve.
pub(crate) fn approval(
    review: &ProposalReviewDto,
    confirmation: String,
) -> Option<ApproveProposalDto> {
    let _ = (review, confirmation);
    todo!()
}

/// The request Reject sends: the reason typed for each change, in order.
pub(crate) fn rejection(reasons: &[String]) -> RejectProposalDto {
    let _ = reasons;
    todo!()
}

/// What a refusal of an approval is said as in the step-up field, when it
/// is about what was typed there; any other refusal is the page's alert.
pub(crate) fn field_error(code: ErrorCode, asks: Asks, short_id: &str) -> Option<Said> {
    let _ = (code, asks, short_id);
    todo!()
}

/// The heading of the proposal `id` as the page shows it.
pub(crate) fn heading(id: &str) -> Said {
    let _ = id;
    todo!()
}

/// What is announced once the proposal `id` is applied.
pub(crate) fn applied(id: &str) -> Said {
    let _ = id;
    todo!()
}

/// What is announced once the proposal `id` is rejected.
pub(crate) fn rejected(id: &str) -> Said {
    let _ = id;
    todo!()
}

/// The field for the reason of change `change`, counted from 1.
pub(crate) fn reason_label(change: usize) -> Said {
    let _ = change;
    todo!()
}

#[cfg(test)]
mod tests;
