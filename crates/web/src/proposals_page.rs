//! The "Pending changes" page, the route `/proposals` (decision 89, point
//! 6): the proposals still pending, each opened in a section of the page
//! that shows its summary, the AI agent's words apart from it and the exact
//! diff of the configuration file, then approves it with the step-up field
//! or rejects it with a reason for each change. Which proposals it lists,
//! the words of each line, whether Approve is offered and the requests it
//! sends are `proposals.rs`'s, host-tested; this is the DOM glue, which the
//! `@ui` scenarios of `features/ui/proposals.feature`, and the page's in
//! `features/ui/buttons.feature`, run in Chromium.

use std::sync::Arc;
use std::time::Duration;

use api::{
    MsgArg, ProposalDto, ProposalForgetPreviewDto, ProposalReviewDto, ProposalsDto, SummaryLineDto,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use crate::announcer::Politeness;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::first_load;
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Banner, Button, StepUp, StepUpField, TextField, Toast, ToastTone, Toasts, focus_element,
    hand_focus_when_gone, refusal_alert, say_refusals, text_field,
};
use crate::live::reply;
use crate::plans::Said;
use crate::proposals::{
    ForgetListing, PROPOSALS_PATH, RepositoryShown, applied, approval, approve_path, can_approve,
    field_error, forget_preview_path, heading, pending, reason_label, reject_path, rejected,
    rejected_before, rejection, repository_shown, review_id, review_label, review_path,
    summary_line,
};
use crate::repositories::Sentence;
use crate::restore::Asks;
use crate::routes::{PAGE_HEADING_ID, PageHeading, refused_sentence, text_with};
use crate::session::Standing;
use crate::snapshots::snapshots_table;
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Phase, Posted};

/// The `id` of the open proposal's heading, which takes focus as it opens
/// and names its section.
const REVIEW_HEADING_ID: &str = "proposal-heading";

/// The `id` of the open proposal's step-up field.
const STEP_UP_ID: &str = "proposal-step-up";

/// What the open proposal is waiting for the daemon to do, said beside the
/// button that asked for it, which is busy meanwhile, the other unavailable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Deciding {
    Approving,
    Rejecting,
}

/// The text of `said` in the page's language.
fn say(localizer: &Localizer, (id, args): Said) -> String {
    text_with(localizer, id, &args)
}

/// The sentence said when the daemon did not answer a request.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_SESSION_UNREACHABLE), None)
}

/// The pending proposals as the page last read them: where the reading
/// stands, the proposals (`None` until the read answers, and while it is
/// refused), and a read the daemon refused, an Error alert in the list's
/// place.
#[derive(Clone, Copy)]
struct List {
    base: StoredValue<String>,
    phase: RwSignal<Phase>,
    proposals: RwSignal<Option<Vec<ProposalDto>>>,
    refused: RwSignal<Option<Sentence>>,
}

impl List {
    fn new() -> Self {
        Self {
            base: StoredValue::new(ws::origin()),
            phase: RwSignal::new(Phase::Loading),
            proposals: RwSignal::new(None),
            refused: RwSignal::new(None),
        }
    }

    fn url(self, path: &str) -> String {
        format!("{}{path}", self.base.get_value())
    }

    /// Reads the proposals again.
    async fn read(self, localizer: &Localizer) {
        let Some(base) = self.base.try_get_value() else {
            return;
        };
        let read = ws::get::<ProposalsDto>(&format!("{base}{PROPOSALS_PATH}")).await;
        // The reader may have left the page before the answer came.
        if self.phase.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                self.refused.set(None);
                self.proposals.set(Some(pending(read.proposals)));
                self.phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                self.refused.set(Some(refused_sentence(
                    localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                self.proposals.set(None);
                self.phase.set(Phase::Ready);
            }
            None => self.phase.set(Phase::Unreachable),
        }
    }
}

/// The "Pending changes" page: the heading, which says when the daemon
/// can't be reached, what the page is for, the toast that answers a
/// proposal decided, which nothing else on the page confirms once it has
/// left the list, the list of pending proposals with a Review button each,
/// and the proposal opened, in a section below.
#[component]
pub(crate) fn Proposals(localizer: Arc<Localizer>) -> impl IntoView {
    let list = List::new();
    say_refusals(list.refused);
    let read_localizer = Arc::clone(&localizer);
    spawn_local(async move { list.read(&read_localizer).await });
    let toasts = Toasts::new();

    // The proposal open, as the daemon showed it, and why it could not
    // open, an Error alert in its place.
    let reviewing = RwSignal::<Option<ProposalReviewDto>>::new(None);
    let not_opened = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(not_opened);
    let open_localizer = Arc::clone(&localizer);
    let open = Callback::new(move |id: String| {
        not_opened.set(None);
        let url = list.url(&review_path(&id));
        let localizer = Arc::clone(&open_localizer);
        spawn_local(async move {
            match ws::get::<ProposalReviewDto>(&url).await {
                Some(Got::Read(review)) => {
                    reviewing.try_set(Some(review));
                }
                Some(Got::Refused(code, args, detail)) => {
                    not_opened.try_set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                }
                None => {
                    not_opened.try_set(Some(unanswered(&localizer)));
                }
            }
        });
    });

    let heading = text(&localizer, msg::UI_PROPOSALS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_PROPOSALS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let none = text(&localizer, msg::UI_PROPOSALS_NONE);
    // With sign-in off, approving here is no safety boundary, and the page
    // says so in a warning banner (decision 89, point 7).
    let standing = use_context::<RwSignal<Standing>>();
    let sign_in_off = text(&localizer, msg::UI_PROPOSALS_SIGN_IN_OFF);
    let off_localizer = Arc::clone(&localizer);
    let off = move || {
        standing
            .is_some_and(|standing| standing.get() == Standing::Off)
            .then(|| {
                let words = sign_in_off.clone();
                view! {
                    <Banner localizer=Arc::clone(&off_localizer) tone=BannerTone::Warning>
                        {words}
                    </Banner>
                }
            })
    };
    // What the list's place shows; a memo, so a read again that changes
    // only which proposals are listed keeps the list, and its items follow
    // in place.
    let shape = Memo::new(move |_| {
        (
            list.phase.get(),
            list.refused.get(),
            list.proposals.with(|proposals| {
                proposals
                    .as_ref()
                    .is_some_and(|proposals| !proposals.is_empty())
            }),
        )
    });
    let list_localizer = Arc::clone(&localizer);
    let refusal_localizer = Arc::clone(&localizer);
    let not_opened_localizer = Arc::clone(&localizer);
    let review_localizer = Arc::clone(&localizer);
    view! {
        <PageHeading>
            {move || {
                if list.phase.get() == Phase::Unreachable {
                    unreachable_heading.clone()
                } else {
                    heading.clone()
                }
            }}
        </PageHeading>
        <p>{body}</p>
        // Dismissed, a toast hands focus to the heading just before it.
        <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=PAGE_HEADING_ID />
        {off}
        {move || match shape.get() {
            (Phase::Loading, _, _) => first_load(loading.clone(), Awaited::Text).into_any(),
            (Phase::Unreachable, _, _) => view! { <p>{unreachable.clone()}</p> }.into_any(),
            (Phase::Ready, Some(sentence), _) => {
                refusal_alert(&refusal_localizer, sentence).into_any()
            }
            (Phase::Ready, None, true) => {
                proposals_list(Arc::clone(&list_localizer), list, open).into_any()
            }
            (Phase::Ready, None, false) => view! { <p>{none.clone()}</p> }.into_any(),
        }}
        {move || {
            not_opened.get().map(|sentence| refusal_alert(&not_opened_localizer, sentence))
        }}
        {move || {
            reviewing
                .get()
                .map(|review| {
                    view! {
                        <Review
                            localizer=Arc::clone(&review_localizer)
                            list=list
                            review=review
                            reviewing=reviewing
                            toasts=toasts
                        />
                    }
                })
        }}
    }
}

/// The list of pending proposals, named by the page's heading: a Review
/// button each, named for the proposal and who proposed it. Its items are
/// keyed by the proposal's ID, so a read again never replaces a button a
/// reader has focused; one whose proposal goes while it holds focus hands
/// focus to the page's heading.
fn proposals_list(localizer: Arc<Localizer>, list: List, open: Callback<String>) -> impl IntoView {
    view! {
        <ul class="proposals" aria-labelledby=PAGE_HEADING_ID>
            <For
                each=move || list.proposals.get().unwrap_or_default()
                key=|proposal: &ProposalDto| proposal.id.clone()
                children=move |proposal: ProposalDto| {
                    let label = say(&localizer, review_label(&proposal));
                    let button = NodeRef::<leptos::html::Button>::new();
                    hand_focus_when_gone(button, PAGE_HEADING_ID.to_owned());
                    let id = proposal.id;
                    view! {
                        <li>
                            <Button
                                content=Content::Text(label.into())
                                id=review_id(&id)
                                node_ref=button
                                on_press=Callback::new(move |()| open.run(id.clone()))
                            />
                        </li>
                    }
                }
            />
        </ul>
    }
}

/// A summary line, and for a changed plan the settings it changes, listed
/// under it.
fn summary_item(localizer: &Localizer, line: &SummaryLineDto) -> impl IntoView + use<> {
    let (said, settings) = summary_line(line);
    let sentence = say(localizer, said);
    let settings = (!settings.is_empty()).then(|| {
        view! {
            <ul>
                {settings
                    .into_iter()
                    .map(|setting| view! { <li>{text(localizer, setting)}</li> })
                    .collect_view()}
            </ul>
        }
    });
    view! { <li>{sentence} {settings}</li> }
}

/// The proposal open: its heading, which takes focus as it opens, what its
/// changes do in Ladderback's words, the AI agent's words apart from them,
/// whether the file changed since it was drafted, how often its proposer
/// was rejected, the exact diff, then Approve, with the step-up field, and
/// Reject, with a reason for each change. Once the daemon applies or
/// rejects it, the page's `toasts` answer so, the list is read again, the
/// section closes and focus goes to the page's heading. A proposal that
/// could delete snapshots offers no Approve and says why.
#[component]
fn Review(
    localizer: Arc<Localizer>,
    list: List,
    review: ProposalReviewDto,
    reviewing: RwSignal<Option<ProposalReviewDto>>,
    toasts: Toasts,
) -> impl IntoView {
    let id = review.proposal.id.clone();
    let asks =
        Asks::of(use_context::<RwSignal<Standing>>().map(|standing| standing.get_untracked()));
    let working = RwSignal::new(None::<Deciding>);
    let approve_alert = RwSignal::new(None::<Sentence>);
    let reject_alert = RwSignal::new(None::<Sentence>);
    say_refusals(approve_alert);
    say_refusals(reject_alert);
    let heading_ref = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading_ref.get() {
            // A failed focus leaves focus where it was.
            let _ = heading.focus();
        }
    });

    // Once decided: answered in a toast, the list read again, the section
    // closed and focus handed to the page's heading, since the proposal's
    // button has gone from the list.
    let decided = move |localizer: Arc<Localizer>, said: Said| async move {
        toasts.show(ToastTone::Success, say(&localizer, said));
        list.read(&localizer).await;
        if reviewing.try_set(None).is_some() {
            return;
        }
        gloo_timers::future::sleep(Duration::ZERO).await;
        focus_element(PAGE_HEADING_ID);
    };

    // Approve, with the step-up field's value and its error.
    let value = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let short_id = review.short_id.clone();
    let step_up = match asks {
        Asks::Password => StepUp::Password,
        Asks::ShortId => StepUp::ShortId {
            label: text(&localizer, msg::UI_PROPOSAL_ID),
            hint: text(&localizer, msg::UI_PROPOSAL_ID_HINT),
            short: short_id.clone(),
        },
    };
    let shown = StoredValue::new(review.clone());
    // The forget preview shown: its digest, which Approve sends, once a
    // preview read every repository; and whether one was shown at all.
    let forget_digest = RwSignal::new(None::<String>);
    let forget_shown = RwSignal::new(false);
    // A new preview read makes an earlier approval's refusal no longer
    // about what the page shows.
    Effect::new(move |_| {
        if forget_digest.with(Option::is_none) {
            approve_alert.set(None);
        }
    });
    let approve_url = list.url(&approve_path(&id));
    let approve_localizer = Arc::clone(&localizer);
    let approve_id = id.clone();
    let approve = Callback::new(move |()| {
        if working.get_untracked().is_some() {
            return;
        }
        let Some(sent) = shown.with_value(|review| {
            forget_digest
                .with_untracked(|digest| approval(review, digest.as_deref(), value.get_untracked()))
        }) else {
            return;
        };
        working.set(Some(Deciding::Approving));
        // One failure, one alert: a rejection's refusal is no longer the
        // answer once the reader approves.
        approve_alert.set(None);
        reject_alert.set(None);
        reply(
            Politeness::Polite,
            &text(&approve_localizer, msg::UI_PROPOSAL_APPROVING),
        );
        let url = approve_url.clone();
        let localizer = Arc::clone(&approve_localizer);
        let id = approve_id.clone();
        let short_id = short_id.clone();
        spawn_local(async move {
            let answer = ws::post(&url, &sent).await;
            if working.try_set(None).is_some() {
                return;
            }
            match answer {
                Some(Posted::Done(_)) => {
                    value.set(String::new());
                    error.set(None);
                    decided(localizer, applied(&id)).await;
                }
                Some(Posted::Refused(code, args, detail)) => {
                    match field_error(code, asks, &short_id) {
                        // Cleared first, so a second wrong try is said again.
                        Some(said) => {
                            error.set(None);
                            tick().await;
                            error.try_set(Some(say(&localizer, said)));
                        }
                        None => approve_alert.set(Some(refused_sentence(
                            &localizer,
                            code,
                            &args,
                            detail.as_deref(),
                        ))),
                    }
                }
                None => approve_alert.set(Some(unanswered(&localizer))),
            }
        });
    });

    // Reject, with a reason field for each change, counted from 1.
    let reasons: Vec<RwSignal<String>> = review
        .proposal
        .changes
        .iter()
        .map(|_| RwSignal::new(String::new()))
        .collect();
    let reason_fields = reasons
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let change = index.saturating_add(1);
            view! {
                <TextField
                    id=format!("proposal-reason-{change}")
                    localizer=Arc::clone(&localizer)
                    label=say(&localizer, reason_label(change))
                    description=text(&localizer, msg::UI_PROPOSAL_REASON_HINT)
                    value=*value
                    kind=text_field::Kind::Lines { rows: 3 }
                />
            }
        })
        .collect_view();
    let reject_url = list.url(&reject_path(&id));
    let reject_localizer = Arc::clone(&localizer);
    let reject_id = id.clone();
    let reject = move || {
        if working.get_untracked().is_some() {
            return;
        }
        let typed: Vec<String> = reasons
            .iter()
            .map(|reason| reason.get_untracked())
            .collect();
        let sent = rejection(&typed);
        working.set(Some(Deciding::Rejecting));
        approve_alert.set(None);
        reject_alert.set(None);
        reply(
            Politeness::Polite,
            &text(&reject_localizer, msg::UI_PROPOSAL_REJECTING),
        );
        let url = reject_url.clone();
        let localizer = Arc::clone(&reject_localizer);
        let id = reject_id.clone();
        spawn_local(async move {
            let answer = ws::post(&url, &sent).await;
            if working.try_set(None).is_some() {
                return;
            }
            match answer {
                Some(Posted::Done(_)) => decided(localizer, rejected(&id)).await,
                Some(Posted::Refused(code, args, detail)) => reject_alert.set(Some(
                    refused_sentence(&localizer, code, &args, detail.as_deref()),
                )),
                None => reject_alert.set(Some(unanswered(&localizer))),
            }
        });
    };

    let summary = review
        .summary
        .iter()
        .map(|line| summary_item(&localizer, line))
        .collect_view();
    let rationale = if review.proposal.rationale.is_empty() {
        view! { <p>{text(&localizer, msg::UI_PROPOSAL_RATIONALE_NONE)}</p> }.into_any()
    } else {
        view! {
            <blockquote>
                <p>
                    <bdi>{review.proposal.rationale.clone()}</bdi>
                </p>
            </blockquote>
        }
        .into_any()
    };
    let changed_since = review
        .changed_since_drafted
        .then(|| view! { <p>{text(&localizer, msg::UI_PROPOSAL_CHANGED_SINCE)}</p> });
    let approving = text(&localizer, msg::UI_PROPOSAL_APPROVING);
    let form_localizer = Arc::clone(&localizer);
    let approve_form = move || {
        let localizer = Arc::clone(&form_localizer);
        let alert_localizer = Arc::clone(&localizer);
        let approving = approving.clone();
        view! {
            <form
                aria-labelledby="proposal-approve-heading"
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    approve.run(());
                }
            >
                <StepUpField
                    id=STEP_UP_ID
                    localizer=Arc::clone(&localizer)
                    step_up=step_up.clone()
                    value=value
                    error=error
                />
                <p class="actions">
                    <Button
                        content=Content::Text(text(&localizer, msg::UI_PROPOSAL_APPROVE).into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        kind=Kind::Submit
                        busy=Signal::derive(move || working.get() == Some(Deciding::Approving))
                        disabled=Signal::derive(move || working.get() == Some(Deciding::Rejecting))
                    />
                </p>
            </form>
            {move || {
                (working.get() == Some(Deciding::Approving))
                    .then(|| view! { <p>{approving.clone()}</p> })
            }}
            {move || {
                approve_alert.get().map(|sentence| refusal_alert(&alert_localizer, sentence))
            }}
        }
    };
    let approve_part = if can_approve(&review, None) {
        approve_form().into_any()
    } else {
        let url = list.url(&forget_preview_path(&id));
        let unread = text(&localizer, msg::UI_PROPOSAL_FORGET_PREVIEW_UNREAD);
        let needs = review.clone();
        // Approve, once a forget preview that read every repository shows;
        // why not, once one that couldn't does.
        let from_preview = move || {
            if forget_digest.with(|digest| can_approve(&needs, digest.as_deref())) {
                Some(approve_form().into_any())
            } else {
                forget_shown
                    .get()
                    .then(|| view! { <p>{unread.clone()}</p> }.into_any())
            }
        };
        view! {
            <p>{text(&localizer, msg::UI_PROPOSAL_NEEDS_FORGET_PREVIEW)}</p>
            <ForgetPreview
                localizer=Arc::clone(&localizer)
                url=url
                disabled=Signal::derive(move || working.get().is_some())
                digest=forget_digest
                shown_any=forget_shown
            />
            {from_preview}
        }
        .into_any()
    };
    let rejecting = text(&localizer, msg::UI_PROPOSAL_REJECTING);
    let reject_alert_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=REVIEW_HEADING_ID>
            <h2 id=REVIEW_HEADING_ID tabindex="-1" node_ref=heading_ref>
                {say(&localizer, heading(&id))}
            </h2>
            <h3>{text(&localizer, msg::UI_PROPOSAL_SUMMARY_HEADING)}</h3>
            <ul>{summary}</ul>
            <h3>{text(&localizer, msg::UI_PROPOSAL_RATIONALE_HEADING)}</h3>
            {rationale}
            {changed_since}
            <p>{say(&localizer, rejected_before(&review))}</p>
            <h3 id="proposal-diff-heading">{text(&localizer, msg::UI_PROPOSAL_DIFF_HEADING)}</h3>
            <pre class="proposal-diff">
                <code>{review.diff.clone()}</code>
            </pre>
            <h3 id="proposal-approve-heading">
                {text(&localizer, msg::UI_PROPOSAL_APPROVE_HEADING)}
            </h3>
            {approve_part}
            <h3 id="proposal-reject-heading">
                {text(&localizer, msg::UI_PROPOSAL_REJECT_HEADING)}
            </h3>
            <p>{text(&localizer, msg::UI_PROPOSAL_REJECT_BODY)}</p>
            <form
                aria-labelledby="proposal-reject-heading"
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    reject();
                }
            >
                {reason_fields}
                <p class="actions">
                    <Button
                        content=Content::Text(text(&localizer, msg::UI_PROPOSAL_REJECT).into())
                        kind=Kind::Submit
                        busy=Signal::derive(move || working.get() == Some(Deciding::Rejecting))
                        disabled=Signal::derive(move || working.get() == Some(Deciding::Approving))
                    />
                </p>
            </form>
            {move || {
                (working.get() == Some(Deciding::Rejecting))
                    .then(|| view! { <p>{rejecting.clone()}</p> })
            }}
            {move || {
                reject_alert.get().map(|sentence| refusal_alert(&reject_alert_localizer, sentence))
            }}
        </section>
    }
}

/// The open proposal's forget preview (decision 89, point 8), read when the
/// person asks, since it reads every repository the proposal touches: under
/// its heading, each repository's lines and differences, or why it couldn't
/// be read. While it reads, its button is busy and the page says so; while
/// `disabled`, as while an approval or a rejection waits, its button is
/// unavailable. A refused read, and each repository that couldn't be read,
/// is an Error alert said assertively once. Once read, `shown_any` is set and
/// `digest` holds what an approval of the preview shown brings, none while a
/// repository couldn't be read.
#[component]
fn ForgetPreview(
    localizer: Arc<Localizer>,
    url: String,
    disabled: Signal<bool>,
    digest: RwSignal<Option<String>>,
    shown_any: RwSignal<bool>,
) -> impl IntoView {
    let reading = RwSignal::new(false);
    let shown = RwSignal::new(None::<Vec<RepositoryShown>>);
    let alert = RwSignal::new(None::<Sentence>);
    say_refusals(alert);
    let read_localizer = Arc::clone(&localizer);
    let read = Callback::new(move |()| {
        if reading.get_untracked() {
            return;
        }
        reading.set(true);
        alert.set(None);
        // A preview read before is no longer shown as current.
        shown.set(None);
        digest.set(None);
        shown_any.set(false);
        reply(
            Politeness::Polite,
            &text(&read_localizer, msg::UI_PROPOSAL_FORGET_PREVIEWING),
        );
        let url = url.clone();
        let localizer = Arc::clone(&read_localizer);
        spawn_local(async move {
            let answer = ws::post(&url, &serde_json::Map::new()).await;
            if reading.try_set(false).is_some() {
                return;
            }
            let read = match answer {
                Some(Posted::Done(answer)) => {
                    serde_json::from_str::<ProposalForgetPreviewDto>(&answer).ok()
                }
                Some(Posted::Refused(code, args, detail)) => {
                    alert.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    return;
                }
                None => None,
            };
            match read {
                Some(preview) => {
                    shown.set(Some(
                        preview.repositories.iter().map(repository_shown).collect(),
                    ));
                    digest.set(preview.digest);
                    shown_any.set(true);
                    reply(
                        Politeness::Polite,
                        &text(&localizer, msg::UI_PROPOSAL_FORGET_HEADING),
                    );
                }
                // No answer, or one that isn't the route's.
                None => alert.set(Some(unanswered(&localizer))),
            }
        });
    });
    let previewing = text(&localizer, msg::UI_PROPOSAL_FORGET_PREVIEWING);
    let alert_localizer = Arc::clone(&localizer);
    let view_localizer = Arc::clone(&localizer);
    let preview = move || {
        shown.get().map(|repositories| {
            let localizer = Arc::clone(&view_localizer);
            let repositories = repositories
                .into_iter()
                .map(|repository| repository_view(&localizer, repository))
                .collect_view();
            view! {
                <h3>{text(&localizer, msg::UI_PROPOSAL_FORGET_HEADING)}</h3>
                {repositories}
            }
        })
    };
    view! {
        <p class="actions">
            <Button
                content=Content::Text(text(&localizer, msg::UI_PROPOSAL_FORGET_PREVIEW).into())
                busy=Signal::derive(move || reading.get())
                disabled=disabled
                on_press=read
            />
        </p>
        {move || reading.get().then(|| view! { <p>{previewing.clone()}</p> })}
        {move || alert.get().map(|sentence| refusal_alert(&alert_localizer, sentence))}
        {preview}
    }
}

/// One repository of a forget preview, under its heading: what retention
/// would forget there, or why it couldn't be read, as an Error alert. That
/// alert is no live region, so why is said assertively too, as the preview
/// is drawn: once for each answer, since the preview is drawn again only
/// when a read answers (a second, rarer way than `say_refusals`, for a
/// refusal that is part of a list rather than a signal of its own).
fn repository_view(localizer: &Arc<Localizer>, repository: RepositoryShown) -> AnyView {
    let heading = |repository: String| {
        say(
            localizer,
            (
                msg::UI_PROPOSAL_FORGET_REPOSITORY,
                vec![("repository", MsgArg::Text(repository))],
            ),
        )
    };
    match repository {
        RepositoryShown::Compared {
            repository,
            lines,
            listings,
        } => {
            let lines = lines
                .into_iter()
                .map(|said| view! { <p>{say(localizer, said)}</p> })
                .collect_view();
            let listings = listings
                .into_iter()
                .map(|listing| listing_view(localizer, listing))
                .collect_view();
            view! {
                <h4>{heading(repository)}</h4>
                {lines}
                {listings}
            }
            .into_any()
        }
        RepositoryShown::Refused { repository, error } => {
            let args: Vec<(String, MsgArg)> = error.args.into_iter().collect();
            let sentence = refused_sentence(localizer, error.code, &args, error.detail.as_deref());
            reply(Politeness::Assertive, &sentence.plain());
            view! {
                <h4>{heading(repository)}</h4>
                {refusal_alert(localizer, sentence)}
            }
            .into_any()
        }
    }
}

/// The snapshots one difference names, newest first, in a table named by its
/// caption, and what is said of those the daemon left out.
fn listing_view(localizer: &Arc<Localizer>, listing: ForgetListing) -> impl IntoView + use<> {
    let table = snapshots_table(
        localizer,
        say(localizer, listing.caption),
        listing.snapshots,
        l10n::viewer_zone(),
        None,
    );
    let more = listing
        .more
        .map(|said| view! { <p>{say(localizer, said)}</p> });
    view! {
        {table}
        {more}
    }
}
