//! The Snapshots page's DOM glue (technical design §4.4): the route
//! `/snapshots`, one repository's history grouped by owner, and the
//! maintenance that changes it, each run reviewed in a dialog before it
//! starts. It decides nothing: what a request carries, what a review says,
//! which group can be adopted and how a run's end is awaited are
//! `maintenance.rs` and `wording.rs`, host-tested; this module sends the
//! requests and shows the answers, and `features/ui/maintenance.feature` runs
//! it in a browser, the `ws.rs` precedent.

use std::sync::Arc;

use api::{
    ErrorCode, MaintenancePreviewDto, MaintenanceRequestDto, MsgArg, OrphanedGroupDto,
    PlanGroupDto, RepositoriesDto, SnapshotDto, SnapshotGroupsDto, UnmanagedGroupDto,
};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use crate::announcer::Politeness;
use crate::components::{Confirmation, ReviewDialog, recover_focus};
use crate::live::say;
use crate::maintenance::{self, Kind, Owner, Review, Rows};
use crate::routes::{Empty, PageHeading, instant, refusal, short_id, text_with};
use crate::text;
use crate::wording::{self, Adoption, Words};
use crate::ws::{self, Got, Posted, Waited};

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

/// The run that goes on: what it is, and the operation to stop it by.
#[derive(Clone)]
struct Running {
    kind: Kind,
    operation: String,
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
    unreachable: RwSignal<bool>,
    groups: RwSignal<Option<SnapshotGroupsDto>>,
    alert: RwSignal<Option<String>>,
    preparing: RwSignal<bool>,
    busy: RwSignal<bool>,
    running: RwSignal<Option<Running>>,
    ended: RwSignal<Option<String>>,
    review: RwSignal<Option<Reviewing>>,
    open: RwSignal<bool>,
}

impl Page {
    fn new(localizer: Arc<Localizer>, alias: String, unreachable: RwSignal<bool>) -> Page {
        Page {
            localizer: StoredValue::new(localizer),
            viewer: StoredValue::new(l10n::viewer_zone()),
            base: StoredValue::new(ws::origin()),
            alias: StoredValue::new(alias),
            unreachable,
            groups: RwSignal::new(None),
            alert: RwSignal::new(None),
            preparing: RwSignal::new(false),
            busy: RwSignal::new(false),
            running: RwSignal::new(None),
            ended: RwSignal::new(None),
            review: RwSignal::new(None),
            open: RwSignal::new(false),
        }
    }

    /// Whether the page is still there. Its handles go with it, and an answer
    /// that arrives late must neither read nor write them.
    fn alive(self) -> bool {
        self.busy.try_get_untracked().is_some()
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

    fn url(self, route: String) -> String {
        format!("{}{route}", self.base.get_value())
    }

    /// Shows a refusal where a reader is looking, as an alert.
    fn refuse(self, code: ErrorCode, args: &[(String, MsgArg)], detail: Option<&str>) {
        let shown = refusal(&self.localizer.get_value(), code, args, detail);
        self.alert.set(Some(shown));
    }

    /// Reads the repository's history afresh.
    fn reload(self) {
        spawn_local(async move {
            let route = maintenance::groups_path(&self.alias.get_value());
            let answer = ws::get::<SnapshotGroupsDto>(&self.url(route)).await;
            if !self.alive() {
                return;
            }
            match answer {
                Some(Got::Read(groups)) => {
                    self.groups.set(Some(groups));
                    // The control that held focus may have gone with the
                    // groups it was in.
                    tick().await;
                    recover_focus(MAINTENANCE_HEADING);
                }
                Some(Got::Refused(code, args, detail)) => {
                    self.refuse(code, &args, detail.as_deref());
                }
                None => self.unreachable.set(true),
            }
        });
    }

    /// Asks what `request` would do, and shows the answer as a review that
    /// the control `returns_to` asked for.
    fn preview(self, request: MaintenanceRequestDto, returns_to: String) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.preparing.set(true);
        self.alert.set(None);
        spawn_local(async move {
            let route = maintenance::preview_path(&self.alias.get_value());
            let answer = ws::post(&self.url(route), &request).await;
            if !self.alive() {
                return;
            }
            self.preparing.set(false);
            self.busy.set(false);
            match answer {
                Some(Posted::Done(body)) => {
                    match serde_json::from_str::<MaintenancePreviewDto>(&body) {
                        Ok(preview) => {
                            let review = Review::of(&request, &preview);
                            self.review.set(Some(Reviewing { review, returns_to }));
                            self.open.set(true);
                        }
                        Err(_) => self.unreachable.set(true),
                    }
                }
                Some(Posted::Refused(code, args, detail)) => {
                    self.refuse(code, &args, detail.as_deref());
                    if maintenance::rereads(code) {
                        self.reload();
                    }
                }
                None => self.unreachable.set(true),
            }
        });
    }

    /// Confirms a review: starts the run it describes, closes the review
    /// whatever the answer, and waits for the run's end.
    fn start(self, review: Review) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.alert.set(None);
        spawn_local(async move {
            let route = maintenance::start_path(&self.alias.get_value());
            let answer = ws::post(&self.url(route), &review.start_body()).await;
            if !self.alive() {
                return;
            }
            self.busy.set(false);
            self.open.set(false);
            match answer {
                Some(Posted::Done(body)) => match ws::started_operation(&body) {
                    Some(operation) => self.wait(review.kind, operation).await,
                    None => self.unreachable.set(true),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    self.refuse(code, &args, detail.as_deref());
                    if maintenance::rereads(code) {
                        self.reload();
                    }
                }
                None => self.unreachable.set(true),
            }
        });
    }

    /// Shows the run that goes on and, when it has ended, tells how it ended
    /// and reads the history it changed.
    async fn wait(self, kind: Kind, operation: String) {
        self.ended.set(None);
        self.running.set(Some(Running {
            kind,
            operation: operation.clone(),
        }));
        let base = self.base.get_value();
        let waited = ws::await_end(&base, &operation, kind, move || self.alive()).await;
        if !self.alive() {
            return;
        }
        self.running.set(None);
        match waited {
            Waited::Ended(end) => {
                let shown = self.words(&wording::end(&end));
                say(end.politeness, &shown);
                self.ended.set(Some(shown));
            }
            Waited::GaveUp => say(Politeness::Polite, &self.plain(msg::UI_STILL_WAITING)),
            Waited::Gone => {}
        }
        self.reload();
    }

    /// Asks the run that goes on to stop.
    fn cancel(self) {
        let Some(running) = self.running.get_untracked() else {
            return;
        };
        spawn_local(async move {
            let route = maintenance::cancel_path(&running.operation);
            let answer = ws::post(&self.url(route), &Empty {}).await;
            if !self.alive() {
                return;
            }
            if let Some(Posted::Refused(code, args, detail)) = answer {
                self.refuse(code, &args, detail.as_deref());
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
    // The repositories to pick from; `None` while the read is on its way.
    let repositories = RwSignal::<Option<Vec<String>>>::new(None);
    let chosen = RwSignal::<Option<String>>::new(None);
    let repositories_url = format!("{}/api/v1/repositories", ws::origin());
    spawn_local(async move {
        match ws::read::<RepositoriesDto>(&repositories_url).await {
            Some(read) => repositories.set(Some(
                read.repositories.into_iter().map(|row| row.alias).collect(),
            )),
            None => unreachable.set(true),
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
                .map(|alias| {
                    let label = text_with(
                        &picker,
                        msg::UI_SNAPSHOTS_OF,
                        &[("alias", MsgArg::Text(alias.clone()))],
                    );
                    view! {
                        <button type="button" on:click=move |_| chosen.set(Some(alias.clone()))>
                            {label}
                        </button>
                    }
                })
                .collect_view()
                .into_any(),
        }}
        {move || {
            chosen
                .get()
                .map(|alias| {
                    view! {
                        <Repository
                            localizer=Arc::clone(&localizer)
                            alias=alias
                            unreachable=unreachable
                        />
                    }
                })
        }}
    }
}

/// One repository: its maintenance, its history by owner and the review of
/// whatever is asked. It is made anew for each repository picked.
#[component]
fn Repository(
    localizer: Arc<Localizer>,
    alias: String,
    unreachable: RwSignal<bool>,
) -> impl IntoView {
    let page = Page::new(localizer, alias.clone(), unreachable);
    page.reload();
    let heading = page.text(
        msg::UI_MAINTENANCE_HEADING,
        &[("alias", MsgArg::Text(alias))],
    );
    let intro = page.plain(msg::UI_MAINTENANCE_BODY);
    let (forget, prune, check) = (
        page.plain(msg::UI_MAINTENANCE_FORGET),
        page.plain(msg::UI_MAINTENANCE_PRUNE),
        page.plain(msg::UI_MAINTENANCE_CHECK),
    );
    let preparing = page.plain(msg::UI_MAINTENANCE_PREPARING);
    let cancel = page.plain(msg::UI_MAINTENANCE_CANCEL);
    view! {
        <section aria-labelledby=MAINTENANCE_HEADING>
            <h2 id=MAINTENANCE_HEADING tabindex="-1">
                {heading}
            </h2>
            <p>{intro}</p>
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
            {move || page.preparing.get().then(|| view! { <p>{preparing.clone()}</p> })}
            {move || page.alert.get().map(|shown| view! { <p role="alert">{shown}</p> })}
            {move || {
                page.running
                    .get()
                    .map(|running| {
                        let shown = page.words(&wording::running(running.kind));
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
            {move || page.ended.get().map(|shown| view! { <p>{shown}</p> })}
        </section>
        {move || page.groups.get().map(|groups| groups_view(page, groups))}
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
        <table>
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
    let caption = page.text(
        msg::UI_ORPHANED_CAPTION,
        &[("group", MsgArg::Text(heading.clone()))],
    );
    let rows = Rows::of(group.count, group.snapshots.len());
    let adoptions = group
        .adopt_into
        .iter()
        .enumerate()
        .filter_map(|(target, into)| {
            wording::adopt_orphaned(&group, into).map(|adoption| {
                adoption_view(page, format!("adopt-orphaned-{index}-{target}"), adoption)
            })
        })
        .collect_view();
    view! {
        <h3>{heading}</h3>
        {snapshots_view(page, caption, group.snapshots, rows)}
        {adoptions}
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
    let paths = page.words(&wording::paths(&group.paths));
    let caption = page.text(
        msg::UI_UNMANAGED_CAPTION,
        &[("group", MsgArg::Text(heading.clone()))],
    );
    let rows = Rows::of(group.count, group.snapshots.len());
    let adoptions = group
        .adopt_into
        .iter()
        .enumerate()
        .map(|(target, into)| {
            let adoption = wording::adopt_unmanaged(&heading, into);
            adoption_view(page, format!("adopt-unmanaged-{index}-{target}"), adoption)
        })
        .collect_view();
    let no_plan = group
        .adopt_into
        .is_empty()
        .then(|| view! { <p>{page.plain(msg::UI_UNMANAGED_NO_PLAN)}</p> });
    view! {
        <h3>{heading}</h3>
        <p>{paths}</p>
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
/// buttons that close it or confirm it.
fn review_view(page: Page, reviewing: Reviewing) -> impl IntoView {
    let Reviewing { review, returns_to } = reviewing;
    let title = page.words(&wording::title(&review, &page.alias.get_value()));
    let dismiss_label = page.plain(wording::dismiss(&review));
    let confirm = review.confirm.map(|confirm| {
        let started = review.clone();
        Confirmation {
            label: page.words(&wording::confirmation(confirm)),
            action: Callback::new(move |()| page.start(started.clone())),
        }
    });
    let facts = review
        .facts
        .iter()
        .flat_map(wording::fact)
        .map(|words| view! { <p>{page.words(&words)}</p> })
        .collect_view();
    let kind = review.kind;
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
        >
            <div id=REVIEW_SUMMARY>{facts}</div>
            {listings}
        </ReviewDialog>
    }
}
