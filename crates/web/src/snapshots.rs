//! The Snapshots page's DOM glue (technical design §4.4): the route
//! `/snapshots`, one repository's history grouped by owner, and the
//! maintenance that changes it, each run reviewed in a dialog before it
//! starts. It decides nothing. What a request carries, what a review says,
//! which group can be adopted, what the page holds while a request is out or a
//! run goes on and what it does next (`maintenance::Panel`), what shows of the
//! history and how a run's end is awaited are `maintenance.rs` and
//! `wording.rs`, host-tested. What stays here is sending the requests, doing
//! what the panel decides (`Page::act`) and drawing the elements; the
//! journeys of `features/ui` run it in a browser, the `ws.rs` precedent.

use std::sync::Arc;

use api::{
    MaintenancePreviewDto, MaintenanceRequestDto, MsgArg, OrphanedGroupDto, PlanGroupDto,
    RepositoriesDto, SnapshotDto, SnapshotGroupsDto, UnmanagedGroupDto, WarningDto,
};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use crate::components::{Confirmation, ReviewDialog, Working, recover_focus, sentence_view};
use crate::live::say;
use crate::maintenance::{
    self, Kind, Owner, Panel, Refusal, Review, Rows, Shown, Step, Told, shown, unreachable_after,
};
use crate::repositories::{Sentence, repository_notes};
use crate::routes::{Empty, PageHeading, instant, refusal, short_id, text_with};
use crate::text;
use crate::wording::{self, Adoption, Words};
use crate::ws::{self, Got, Posted};

/// The heading of a repository's maintenance: where focus goes when the
/// control that held it leaves the page with the history it showed.
const MAINTENANCE_HEADING: &str = "maintenance-heading";

/// The buttons that ask for a review, which focus returns to when the review
/// is closed.
const FORGET_BUTTON: &str = "maintenance-forget";
const PRUNE_BUTTON: &str = "maintenance-prune";
const CHECK_BUTTON: &str = "maintenance-check";

/// The element of a review that says what it is about.
const REVIEW_SUMMARY: &str = "review-summary";

/// A review the page shows, and the control that asked for it.
#[derive(Clone)]
struct Reviewing {
    review: Review,
    returns_to: String,
}

/// What the page holds of the repository it shows. Every field is a handle,
/// so the whole is copied into the closure of each button.
#[derive(Clone, Copy)]
struct Page {
    localizer: StoredValue<Arc<Localizer>>,
    /// The zone the browser reads its clock in, asked once: every time the
    /// page shows is written in it (§8.4).
    viewer: StoredValue<Option<api::IanaZone>>,
    base: StoredValue<String>,
    alias: StoredValue<String>,
    /// Whether this agent maintains the repository: only then does the page
    /// offer to forget, prune, check or adopt in it (I1).
    maintained: bool,
    unreachable: RwSignal<bool>,
    /// The history read last: `None` until a read is answered, and again when
    /// the history is asked for as if for the first time.
    groups: RwSignal<Option<SnapshotGroupsDto>>,
    /// Whether a read of the history is out.
    reading: RwSignal<bool>,
    panel: RwSignal<Panel>,
    review: RwSignal<Option<Reviewing>>,
    open: RwSignal<bool>,
}

impl Page {
    fn new(
        localizer: Arc<Localizer>,
        alias: String,
        maintained: bool,
        unreachable: RwSignal<bool>,
    ) -> Page {
        Page {
            localizer: StoredValue::new(localizer),
            viewer: StoredValue::new(l10n::viewer_zone()),
            base: StoredValue::new(ws::origin()),
            alias: StoredValue::new(alias),
            maintained,
            unreachable,
            groups: RwSignal::new(None),
            reading: RwSignal::new(false),
            panel: RwSignal::new(Panel::default()),
            review: RwSignal::new(None),
            open: RwSignal::new(false),
        }
    }

    /// Whether the page is still there. Its handles go with it, and an answer
    /// that arrives late must neither read nor write them.
    fn alive(self) -> bool {
        self.open.try_get_untracked().is_some()
    }

    fn text(self, id: MessageId, args: &[(&str, MsgArg)]) -> String {
        text_with(&self.localizer.get_value(), id, args)
    }

    fn plain(self, id: MessageId) -> String {
        self.text(id, &[])
    }

    fn words(self, words: &Words) -> String {
        self.text(words.id, &words.args)
    }

    /// The notes of `warnings`, one sentence each.
    fn notes(self, warnings: &[WarningDto]) -> Vec<String> {
        repository_notes(&self.localizer.get_value(), warnings)
    }

    fn url(self, route: String) -> String {
        format!("{}{route}", self.base.get_value())
    }

    /// What a refusal says, in two parts when the engine's own words are in
    /// it: those are English whatever the language of the page, and are
    /// marked so.
    fn refused_sentence(self, refused: &Refusal) -> Sentence {
        let text = refusal(
            &self.localizer.get_value(),
            refused.code,
            &refused.args,
            refused.detail.as_deref(),
        );
        Sentence::new(&text, refused.detail.as_deref())
    }

    /// Notes whether a request got an answer at all: any answer, a refusal
    /// too, says the daemon is there.
    fn answered(self, answered: bool) {
        self.unreachable.set(unreachable_after(answered));
    }

    /// Changes the panel, then does what the change decides.
    fn change(self, change: impl FnOnce(&mut Panel) -> Vec<maintenance::Effect>) {
        if let Some(effects) = self.panel.try_update(change) {
            self.act(effects);
        }
    }

    /// Does what the panel decided.
    fn act(self, effects: Vec<maintenance::Effect>) {
        use maintenance::Effect::{CloseReview, OpenReview, RecoverFocus, Reload, Say, Watch};
        for effect in effects {
            match effect {
                Say(told) => say(told.politeness(), &self.words(&wording::told(told))),
                OpenReview => self.open.set(true),
                CloseReview => self.open.set(false),
                Reload => self.reload(false),
                Watch => self.watch(),
                RecoverFocus => self.recover_focus(),
            }
        }
    }

    /// Puts focus on the maintenance heading if it has fallen to the page,
    /// once the elements the change removed are gone: the control that held it
    /// may have been one of them.
    fn recover_focus(self) {
        spawn_local(async {
            tick().await;
            recover_focus(MAINTENANCE_HEADING);
        });
    }

    /// Reads the repository's history afresh. `clear` first drops what was
    /// read, as the page does when the history is asked for as if for the
    /// first time; a read for a run that ended leaves it, so that the page
    /// does not blank while the new history is on its way.
    fn reload(self, clear: bool) {
        if !self.alive() {
            return;
        }
        if clear {
            self.groups.set(None);
        }
        self.reading.set(true);
        let url = self.url(maintenance::groups_path(&self.alias.get_value()));
        spawn_local(async move {
            let answer = ws::get::<SnapshotGroupsDto>(&url).await;
            if !self.alive() {
                return;
            }
            self.reading.set(false);
            self.answered(answer.is_some());
            match answer {
                Some(Got::Read(groups)) => {
                    self.groups.set(Some(groups));
                    // The control that held focus may have gone with the
                    // groups it was in.
                    self.recover_focus();
                }
                Some(Got::Refused(code, args, detail)) => self
                    .panel
                    .update(|panel| panel.reports(Refusal { code, args, detail })),
                None => {}
            }
        });
    }

    /// Asks what `request` would do, and shows the answer as a review that
    /// the control `returns_to` asked for.
    fn preview(self, request: MaintenanceRequestDto, returns_to: String) {
        let Some(effects) = self
            .panel
            .try_update(|panel| panel.asks(Step::Preparing))
            .flatten()
        else {
            return;
        };
        self.act(effects);
        let url = self.url(maintenance::preview_path(&self.alias.get_value()));
        spawn_local(async move {
            let answer = ws::post(&url, &request).await;
            if !self.alive() {
                return;
            }
            match answer {
                Some(Posted::Done(body)) => {
                    match serde_json::from_str::<MaintenancePreviewDto>(&body) {
                        Ok(preview) => {
                            self.answered(true);
                            let review = Review::of(&request, &preview);
                            self.review.set(Some(Reviewing { review, returns_to }));
                            self.change(Panel::previewed);
                        }
                        Err(_) => {
                            self.answered(false);
                            self.change(Panel::silent);
                        }
                    }
                }
                Some(Posted::Refused(code, args, detail)) => {
                    self.answered(true);
                    self.change(|panel| panel.refused(Refusal { code, args, detail }));
                }
                None => {
                    self.answered(false);
                    self.change(Panel::silent);
                }
            }
        });
    }

    /// Confirms a review: starts the run it describes. The review stays open,
    /// saying so, until the daemon answers.
    fn start(self, review: Review) {
        let kind = review.kind;
        let Some(effects) = self
            .panel
            .try_update(|panel| panel.asks(Step::Starting(kind)))
            .flatten()
        else {
            return;
        };
        self.act(effects);
        let url = self.url(maintenance::start_path(&self.alias.get_value()));
        spawn_local(async move {
            let answer = ws::post(&url, &review.start_body()).await;
            if !self.alive() {
                return;
            }
            match answer {
                Some(Posted::Done(body)) => match ws::started_operation(&body) {
                    Some(operation) => {
                        self.answered(true);
                        self.change(|panel| panel.started(kind, operation));
                    }
                    None => {
                        self.answered(false);
                        self.change(Panel::silent);
                    }
                },
                Some(Posted::Refused(code, args, detail)) => {
                    self.answered(true);
                    self.change(|panel| panel.refused(Refusal { code, args, detail }));
                }
                None => {
                    self.answered(false);
                    self.change(Panel::silent);
                }
            }
        });
    }

    /// Waits for the run that started to end, and tells the panel how the
    /// wait came out.
    fn watch(self) {
        let Some(run) = self.panel.with_untracked(|panel| panel.run.clone()) else {
            return;
        };
        let base = self.base.get_value();
        spawn_local(async move {
            let how = ws::await_end(&base, &run.operation, run.kind, move || self.alive()).await;
            if self.alive() {
                self.change(|panel| panel.watched(how));
            }
        });
    }

    /// Asks the run that goes on to stop.
    fn cancel(self) {
        let Some(run) = self.panel.with_untracked(|panel| panel.run.clone()) else {
            return;
        };
        let url = self.url(maintenance::cancel_path(&run.operation));
        spawn_local(async move {
            let answer = ws::post(&url, &Empty {}).await;
            if !self.alive() {
                return;
            }
            self.answered(answer.is_some());
            if let Some(Posted::Refused(code, args, detail)) = answer {
                self.panel
                    .update(|panel| panel.reports(Refusal { code, args, detail }));
            }
        });
    }
}

/// The snapshots page, the route `/snapshots`: the repositories to pick from,
/// and the history and maintenance of the one picked. The repositories come
/// from one read, the history of the picked one from another, and a read that
/// does not get an answer says so in the page's heading, like the operations
/// page.
#[component]
pub(crate) fn Snapshots(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_SNAPSHOTS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_SNAPSHOTS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let unreachable = RwSignal::new(false);
    // The repositories to pick from, each with whether this agent maintains
    // it; `None` while the read is on its way.
    let repositories = RwSignal::<Option<Vec<(String, bool)>>>::new(None);
    let chosen = RwSignal::<Option<(String, bool)>>::new(None);
    // The page of a repository is made anew when another is picked, not when
    // the one shown is picked again; that only asks for its history again.
    let shown_alias = Memo::new(move |_| chosen.get());
    let picks = RwSignal::new(0_u32);
    let repositories_url = format!("{}/api/v1/repositories", ws::origin());
    spawn_local(async move {
        match ws::read::<RepositoriesDto>(&repositories_url).await {
            Some(read) => {
                unreachable.set(unreachable_after(true));
                repositories.set(Some(
                    read.repositories
                        .into_iter()
                        .map(|row| (row.alias, row.maintained_here))
                        .collect(),
                ));
            }
            None => unreachable.set(unreachable_after(false)),
        }
    });
    let picker = Arc::clone(&localizer);
    view! {
        <PageHeading>
            {move || if unreachable.get() { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        {move || {
            unreachable
                .get()
                .then(|| view! { <p>{unreachable_body.clone()}</p> })
        }}
        {move || match repositories.get() {
            None => view! { <p>{loading.clone()}</p> }.into_any(),
            Some(aliases) => aliases
                .into_iter()
                .map(|(alias, maintained)| {
                    let label = text_with(
                        &picker,
                        msg::UI_SNAPSHOTS_OF,
                        &[("alias", MsgArg::Text(alias.clone()))],
                    );
                    view! {
                        <button
                            type="button"
                            on:click=move |_| {
                                chosen.set(Some((alias.clone(), maintained)));
                                picks.update(|picks| *picks = picks.wrapping_add(1));
                            }
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()
                .into_any(),
        }}
        {move || {
            shown_alias
                .get()
                .map(|(alias, maintained)| {
                    view! {
                        <Repository
                            localizer=Arc::clone(&localizer)
                            alias=alias
                            maintained=maintained
                            unreachable=unreachable
                            picks=picks
                        />
                    }
                })
        }}
    }
}

/// One repository: its maintenance, its history by owner and the review of
/// whatever is asked. It is made anew for each repository picked, and reads
/// its history again each time its repository is picked. A repository this
/// agent does not maintain shows its history and says why it offers no
/// maintenance.
#[component]
fn Repository(
    localizer: Arc<Localizer>,
    alias: String,
    maintained: bool,
    unreachable: RwSignal<bool>,
    picks: RwSignal<u32>,
) -> impl IntoView {
    let page = Page::new(localizer, alias.clone(), maintained, unreachable);
    Effect::new(move |_| {
        picks.track();
        untrack(|| page.reload(true));
    });
    let heading = page.text(
        msg::UI_MAINTENANCE_HEADING,
        &[("alias", MsgArg::Text(alias))],
    );
    let intro = page.plain(if maintained {
        msg::UI_MAINTENANCE_BODY
    } else {
        msg::UI_MAINTENANCE_ELSEWHERE
    });
    let (forget, prune, check) = (
        page.plain(msg::UI_MAINTENANCE_FORGET),
        page.plain(msg::UI_MAINTENANCE_PRUNE),
        page.plain(msg::UI_MAINTENANCE_CHECK),
    );
    let buttons = maintained.then(|| {
        view! {
            <p class="actions">
                <button
                    type="button"
                    id=FORGET_BUTTON
                    on:click=move |_| {
                        page.preview(maintenance::request(Kind::Forget), FORGET_BUTTON.to_owned());
                    }
                >
                    {forget}
                </button>
                <button
                    type="button"
                    id=PRUNE_BUTTON
                    on:click=move |_| {
                        page.preview(maintenance::request(Kind::Prune), PRUNE_BUTTON.to_owned());
                    }
                >
                    {prune}
                </button>
                <button
                    type="button"
                    id=CHECK_BUTTON
                    on:click=move |_| {
                        page.preview(maintenance::request(Kind::Check), CHECK_BUTTON.to_owned());
                    }
                >
                    {check}
                </button>
            </p>
        }
    });
    let cancel = page.plain(msg::UI_MAINTENANCE_CANCEL);
    let loading = page.plain(msg::UI_LOADING);
    let empty = page.plain(msg::UI_NO_SNAPSHOTS);
    // Each part of the panel has a memo of its own, so that a change of one
    // does not draw the others again: an alert drawn again is announced again.
    let step = Memo::new(move |_| page.panel.with(|panel| panel.step));
    let run = Memo::new(move |_| page.panel.with(|panel| panel.run.clone()));
    let ended = Memo::new(move |_| page.panel.with(|panel| panel.ended));
    let findings = Memo::new(move |_| page.panel.with(|panel| panel.findings.clone()));
    let refused = Memo::new(move |_| page.panel.with(|panel| panel.refusal.clone()));
    let area = Memo::new(move |_| {
        let reading = page.reading.get();
        page.groups.with(|groups| shown(groups.as_ref(), reading))
    });
    // What the history warns of, such as snapshots from other hosts in a
    // repository maintained here (§7.3): said before the buttons that act on it.
    let warned = Memo::new(move |_| {
        page.groups
            .with(|groups| groups.as_ref().map(|groups| groups.warnings.clone()))
            .unwrap_or_default()
    });
    view! {
        <section aria-labelledby=MAINTENANCE_HEADING>
            <h2 id=MAINTENANCE_HEADING tabindex="-1">
                {heading}
            </h2>
            <p>{intro}</p>
            {move || {
                page.notes(&warned.get())
                    .into_iter()
                    .map(|note| view! { <p>{note}</p> })
                    .collect_view()
            }}
            {buttons}
            {move || {
                step.get()
                    .map(|step| {
                        view! { <p>{page.words(&wording::told(Told::from(step)))}</p> }
                    })
            }}
            {move || {
                refused
                    .get()
                    .map(|refused| {
                        view! { <p role="alert">{sentence_view(page.refused_sentence(&refused))}</p> }
                    })
            }}
            {move || {
                run.get()
                    .map(|run| {
                        let shown = page.words(&wording::told(Told::Running(run.kind)));
                        view! {
                            <p>{shown}</p>
                            <p>
                                <button type="button" on:click=move |_| page.cancel()>
                                    {cancel.clone()}
                                </button>
                            </p>
                        }
                    })
            }}
            {move || {
                ended
                    .get()
                    .map(|told| view! { <p>{page.words(&wording::told(told))}</p> })
            }}
            {move || {
                let findings = findings.get();
                let localizer = page.localizer.get_value();
                let listed = (!findings.listed.is_empty())
                    .then(|| {
                        let items = findings
                            .listed
                            .into_iter()
                            .map(|found| {
                                let shown = localizer
                                    .warning(found.code, &[], Some(&found.detail))
                                    .into_text();
                                let sentence = Sentence::new(&shown, Some(&found.detail));
                                view! { <li>{sentence_view(sentence)}</li> }
                            })
                            .collect::<Vec<_>>();
                        view! { <ul>{items}</ul> }
                    });
                let more = wording::check_more(findings.more)
                    .map(|more| view! { <p>{page.words(&more)}</p> });
                (listed, more)
            }}
        </section>
        {move || match area.get() {
            Shown::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
            Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
            Shown::Nothing => ().into_any(),
            Shown::Lists => {
                (move || page.groups.get().map(|groups| groups_view(page, groups))).into_any()
            }
        }}
        {move || page.review.get().map(|reviewing| review_view(page, reviewing))}
    }
}

/// The history of a repository by owner: a section for each plan of this
/// agent, then the orphaned history, then the unmanaged.
fn groups_view(page: Page, groups: SnapshotGroupsDto) -> impl IntoView {
    let plans = groups
        .plans
        .into_iter()
        .enumerate()
        .map(|(index, plan)| plan_view(page, index, plan))
        .collect_view();
    view! {
        {plans}
        {orphaned_view(page, groups.orphaned)}
        {unmanaged_view(page, groups.unmanaged)}
    }
}

fn plan_view(page: Page, index: usize, plan: PlanGroupDto) -> impl IntoView {
    let id = format!("plan-{index}");
    let heading = page.text(
        msg::UI_SNAPSHOTS_PLAN,
        &[("plan", MsgArg::Text(plan.plan.clone()))],
    );
    let caption = page.text(
        msg::UI_SNAPSHOTS_OF_PLAN,
        &[("plan", MsgArg::Text(plan.plan))],
    );
    let rows = Rows::of(plan.count, plan.snapshots.len());
    view! {
        <section aria-labelledby=id.clone()>
            <h2 id=id.clone()>{heading}</h2>
            {snapshots_view(page, caption, plan.snapshots, rows)}
        </section>
    }
}

/// A list of snapshots: the table of the ones listed, newest first as the
/// daemon names them, and what is left out; or the empty state.
fn snapshots_view(page: Page, caption: String, snapshots: Vec<SnapshotDto>, rows: Rows) -> AnyView {
    if rows.is_empty() {
        return view! { <p>{page.plain(msg::UI_NO_SNAPSHOTS)}</p> }.into_any();
    }
    let localizer = page.localizer.get_value();
    let zone = page.viewer.get_value();
    let body = snapshots
        .into_iter()
        .map(|snapshot| {
            let shown = instant(&localizer, &snapshot.time, zone.as_ref());
            view! {
                <tr>
                    <td>{short_id(&snapshot.id)}</td>
                    <td title=snapshot.time>{shown}</td>
                </tr>
            }
        })
        .collect_view();
    let more = wording::more(rows.more).map(|words| view! { <p>{page.words(&words)}</p> });
    view! {
        <table class="snapshots">
            <caption class="text-muted">{caption}</caption>
            <thead>
                <tr>
                    <th scope="col">{page.plain(msg::UI_COLUMN_SNAPSHOT)}</th>
                    <th scope="col">{page.plain(msg::UI_COLUMN_TIME)}</th>
                </tr>
            </thead>
            <tbody>{body}</tbody>
        </table>
        {more}
    }
    .into_any()
}

/// The orphaned history, under one heading of its own; nothing when the
/// repository holds none.
fn orphaned_view(page: Page, groups: Vec<OrphanedGroupDto>) -> impl IntoView {
    (!groups.is_empty()).then(|| {
        let heading = page.plain(msg::UI_ORPHANED_HEADING);
        let intro = page.plain(msg::UI_ORPHANED_BODY);
        let sections = groups
            .into_iter()
            .enumerate()
            .map(|(index, group)| orphaned_group_view(page, index, group))
            .collect_view();
        view! {
            <section aria-labelledby="orphaned-heading">
                <h2 id="orphaned-heading">{heading}</h2>
                <p>{intro}</p>
                {sections}
            </section>
        }
    })
}

fn orphaned_group_view(page: Page, index: usize, group: OrphanedGroupDto) -> impl IntoView {
    let heading = page.words(&wording::owner(Owner::of(&group)));
    let caption = page.words(&wording::owner_caption(Owner::of(&group)));
    let rows = Rows::of(group.count, group.snapshots.len());
    let adoptions = group
        .adopt_into
        .iter()
        .enumerate()
        .filter(|_| page.maintained)
        .filter_map(|(target, into)| {
            wording::adopt_orphaned(&group, into).map(|adoption| {
                adoption_view(page, format!("adopt-orphaned-{index}-{target}"), adoption)
            })
        })
        .collect_view();
    let stuck = maintenance::orphaned_stuck(&group)
        .map(|why| view! { <p>{page.words(&wording::orphaned_stuck(why))}</p> });
    view! {
        <h3>{heading}</h3>
        {snapshots_view(page, caption, group.snapshots, rows)}
        {adoptions}
        {stuck}
    }
}

/// The unmanaged history, under one heading of its own; nothing when the
/// repository holds none.
fn unmanaged_view(page: Page, groups: Vec<UnmanagedGroupDto>) -> impl IntoView {
    (!groups.is_empty()).then(|| {
        let heading = page.plain(msg::UI_UNMANAGED_HEADING);
        let intro = page.plain(msg::UI_UNMANAGED_BODY);
        let sections = groups
            .into_iter()
            .enumerate()
            .map(|(index, group)| unmanaged_group_view(page, index, group))
            .collect_view();
        view! {
            <section aria-labelledby="unmanaged-heading">
                <h2 id="unmanaged-heading">{heading}</h2>
                <p>{intro}</p>
                {sections}
            </section>
        }
    })
}

fn unmanaged_group_view(page: Page, index: usize, group: UnmanagedGroupDto) -> impl IntoView {
    let heading = page.words(&wording::unmanaged(&group));
    let caption = page.words(&wording::unmanaged_caption(&group));
    // The paths, one to an item: each is data, laid out in a direction of its
    // own.
    let paths = (!group.paths.is_empty()).then(|| {
        let items = group
            .paths
            .iter()
            .map(|path| view! { <li><bdi>{path.clone()}</bdi></li> })
            .collect_view();
        view! {
            <p>{page.plain(msg::UI_UNMANAGED_PATHS)}</p>
            <ul>{items}</ul>
        }
    });
    let rows = Rows::of(group.count, group.snapshots.len());
    let adoptions = group
        .adopt_into
        .iter()
        .enumerate()
        .filter(|_| page.maintained)
        .map(|(target, into)| {
            let adoption = wording::adopt_unmanaged(&group, into);
            adoption_view(page, format!("adopt-unmanaged-{index}-{target}"), adoption)
        })
        .collect_view();
    let no_plan = maintenance::unmanaged_stuck(&group)
        .then(|| view! { <p>{page.words(&wording::unmanaged_stuck())}</p> });
    view! {
        <h3>{heading}</h3>
        {paths}
        {snapshots_view(page, caption, group.snapshots, rows)}
        {adoptions}
        {no_plan}
    }
}

/// A button that asks for the review of an adoption.
fn adoption_view(page: Page, id: String, adoption: Adoption) -> impl IntoView {
    let label = page.words(&adoption.label);
    let request = adoption.request;
    let opener = id.clone();
    view! {
        <p>
            <button
                type="button"
                id=id
                on:click=move |_| page.preview(request.clone(), opener.clone())
            >
                {label}
            </button>
        </p>
    }
}

/// The review of a run: what it would do, the snapshots it names, and the
/// buttons that close it or confirm it. While the run is being started it
/// says so and holds still.
fn review_view(page: Page, reviewing: Reviewing) -> impl IntoView {
    let Reviewing { review, returns_to } = reviewing;
    let kind = review.kind;
    let title = page.words(&wording::title(&review, &page.alias.get_value()));
    let dismiss_label = page.plain(wording::dismiss(&review));
    let confirm = review.confirm.map(|confirm| {
        let started = review.clone();
        Confirmation {
            label: page.words(&wording::confirmation(confirm)),
            action: Callback::new(move |()| page.start(started.clone())),
        }
    });
    let working = Working {
        active: Signal::derive(move || page.panel.with(Panel::starting)),
        status: page.words(&wording::told(Told::Starting(kind))),
    };
    let facts = review
        .facts
        .iter()
        .flat_map(wording::fact)
        .map(|words| view! { <p>{page.words(&words)}</p> })
        .collect_view();
    // A warning is part of what the review says, so a screen reader reads it
    // with the facts as the dialog opens.
    let warnings = page
        .notes(&review.warnings)
        .into_iter()
        .map(|note| view! { <p>{note}</p> })
        .collect_view();
    let listings = review
        .listings
        .into_iter()
        .map(|listing| {
            let caption = page.words(&wording::listing_caption(kind, &listing.plan));
            let rows = Rows {
                shown: listing.snapshots.len(),
                more: listing.more,
            };
            snapshots_view(page, caption, listing.snapshots, rows)
        })
        .collect_view();
    view! {
        <ReviewDialog
            open=page.open
            title=title
            dismiss_label=dismiss_label
            confirm=confirm
            returns_to=returns_to
            described_by=REVIEW_SUMMARY.to_owned()
            working=working
            fallback=MAINTENANCE_HEADING.to_owned()
        >
            <div id=REVIEW_SUMMARY>{facts}{warnings}</div>
            {listings}
        </ReviewDialog>
    }
}
