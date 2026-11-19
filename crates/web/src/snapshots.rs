//! The Snapshots page's DOM glue (technical design §4.4): the route
//! `/snapshots`, one repository's history grouped by owner, the maintenance
//! that changes it, each run reviewed in a dialog before it starts, and the
//! repository's stats. It decides nothing. What a request carries, what a
//! review says, which group can be adopted, what the page holds while a
//! request is out or a run goes on and what it does next
//! (`maintenance::Panel`, `stats::StatsPanel`), what shows of the history
//! and of the stats and how a run's end is awaited are `maintenance.rs`,
//! `stats.rs` and `wording.rs`, host-tested. What stays here is sending the
//! requests, doing what the panels decide (`Page::act`) and drawing the
//! elements; the journeys of `features/ui` run it in a browser, the `ws.rs`
//! precedent.

use std::sync::Arc;

use api::{
    MaintenancePreviewDto, MaintenanceRequestDto, MsgArg, OrphanedGroupDto, PlanGroupDto,
    RepositoriesDto, RepositoryStatsDto, SnapshotDto, SnapshotGroupsDto, StatsReadingDto,
    UnmanagedGroupDto, WarningDto,
};
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};
use leptos::wasm_bindgen::JsValue;

use crate::announcer::Politeness;
use crate::components::button::Content;
use crate::components::first_load;
use crate::components::skeleton::rules::Awaited;
use crate::components::tree::rules::snapshot_link;
use crate::components::{
    Button, Column, Confirmation, Link, Name as TableName, ReviewDialog, Sentences, State, Table,
    Working, dialog_refusal_alert, recover_focus, refusal_alert, say_refusals, sentence_view,
};
use crate::live::{reply, say};
use crate::maintenance::{
    self, Kind, Owner, Panel, Refusal, Review, Rows, Shown, Step, Told, shown, unreachable_after,
};
use crate::repositories::{Sentence, repository_notes};
use crate::routes::{
    Empty, PageHeading, refused_sentence, short_id, snapshot_time, text_with, time_view,
};
use crate::stats::{self, StatsPanel};
use crate::text;
use crate::variants::{Rem, Tone};
use crate::wording::{self, Adoption, Words};
use crate::ws::{self, Got, Posted};

/// The heading of a repository's maintenance: where focus goes when the
/// control that held it leaves the page with the history it showed.
const MAINTENANCE_HEADING: &str = "maintenance-heading";

/// The room a snapshot's short ID needs to read whole as a table's row
/// header: its eight hex digits in bold at their widest, DejaVu Sans Bold's
/// "b" and "d" (0.72em each), with the letter spacing a reader may add
/// (0.12em, 1.4.12), 6.7em in all, and the cell's padding at its widest
/// (0.5rem a side), rounded up. Two fifths of the forget review's frame at
/// 320 px leaves an ID 89.6 px, and the widest takes 91.6 (#1129).
const SHORT_ID_ROOM: Rem = Rem::tenths(80);

/// The heading of a repository's stats, which also names their table.
const STATS_HEADING: &str = "stats-heading";

/// The buttons that ask for a review, which focus returns to when the review
/// is closed.
const FORGET_BUTTON: &str = "maintenance-forget";
const PRUNE_BUTTON: &str = "maintenance-prune";
const CHECK_BUTTON: &str = "maintenance-check";

/// The button that turns maintenance from this agent on or off, which focus
/// returns to when its dialog is closed, and the element of that dialog that
/// says what the change means.
const MAINTAINER_BUTTON: &str = "maintainer-switch";
const MAINTAINER_BODY: &str = "maintainer-body";

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
    /// offer to forget, prune, check or adopt in it (I1). It changes when
    /// maintenance from this agent is turned on or off here.
    maintained: RwSignal<bool>,
    /// Whether this agent maintained the repository when the dialog that
    /// turns its maintenance on or off was last asked for, which decides
    /// what that dialog offers: `None` until it is asked for.
    switch: RwSignal<Option<bool>>,
    switch_open: RwSignal<bool>,
    /// Whether the change the dialog confirmed is out.
    switch_busy: RwSignal<bool>,
    /// The daemon's refusal of the change, said inside the dialog.
    switch_refused: RwSignal<Option<Sentence>>,
    /// Tells the list of repositories who maintains the one shown now, so
    /// that picking it again shows it so.
    remember: Callback<(String, bool)>,
    unreachable: RwSignal<bool>,
    /// The history read last: `None` until a read is answered, and again when
    /// the history is asked for as if for the first time.
    groups: RwSignal<Option<SnapshotGroupsDto>>,
    /// Whether a read of the history is out.
    reading: RwSignal<bool>,
    panel: RwSignal<Panel>,
    review: RwSignal<Option<Reviewing>>,
    open: RwSignal<bool>,
    /// The repository's stats: the reading shown, and the read that is out.
    stats: RwSignal<StatsPanel>,
}

impl Page {
    fn new(
        localizer: Arc<Localizer>,
        alias: String,
        maintained: bool,
        unreachable: RwSignal<bool>,
        remember: Callback<(String, bool)>,
    ) -> Page {
        Page {
            localizer: StoredValue::new(localizer),
            viewer: StoredValue::new(l10n::viewer_zone()),
            base: StoredValue::new(ws::origin()),
            alias: StoredValue::new(alias),
            maintained: RwSignal::new(maintained),
            switch: RwSignal::new(None),
            switch_open: RwSignal::new(false),
            switch_busy: RwSignal::new(false),
            switch_refused: RwSignal::new(None),
            remember,
            unreachable,
            groups: RwSignal::new(None),
            reading: RwSignal::new(false),
            panel: RwSignal::new(Panel::default()),
            review: RwSignal::new(None),
            open: RwSignal::new(false),
            stats: RwSignal::new(StatsPanel::default()),
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
        refused_sentence(
            &self.localizer.get_value(),
            refused.code,
            &refused.args,
            refused.detail.as_deref(),
        )
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
        // The stats too: a check that ended read them.
        self.look_at_stats();
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

    /// Reads the repository's stats as last read and kept.
    fn look_at_stats(self) {
        let Some(look) = self.stats.try_update(StatsPanel::look) else {
            return;
        };
        let url = self.url(stats::stats_path(&self.alias.get_value()));
        spawn_local(async move {
            let answer = ws::get::<RepositoryStatsDto>(&url).await;
            if !self.alive() {
                return;
            }
            self.answered(answer.is_some());
            match answer {
                Some(Got::Read(kept)) => self.stats.update(|panel| panel.looked(look, kept)),
                Some(Got::Refused(code, args, detail)) => self.stats.update(|panel| {
                    panel.look_refused(look, Refusal { code, args, detail });
                }),
                None => self.stats.update(StatsPanel::look_unanswered),
            }
        });
    }

    /// Reads the repository's stats now, saying so while the read is out and
    /// again once it is answered.
    fn read_stats(self) {
        let Some(said) = self.stats.try_update(StatsPanel::asks).flatten() else {
            return;
        };
        say(said.politeness, &self.words(&said.words));
        let url = self.url(stats::stats_path(&self.alias.get_value()));
        spawn_local(async move {
            let answer = ws::post(&url, &Empty {}).await;
            if !self.alive() {
                return;
            }
            match answer {
                Some(Posted::Done(body)) => match serde_json::from_str::<StatsReadingDto>(&body) {
                    Ok(reading) => {
                        self.answered(true);
                        if let Some(said) = self.stats.try_update(|panel| panel.read(reading)) {
                            say(said.politeness, &self.words(&said.words));
                        }
                    }
                    Err(_) => {
                        self.answered(false);
                        self.stats.update(StatsPanel::silent);
                    }
                },
                Some(Posted::Refused(code, args, detail)) => {
                    self.answered(true);
                    self.stats
                        .update(|panel| panel.refused(Refusal { code, args, detail }));
                }
                None => {
                    self.answered(false);
                    self.stats.update(StatsPanel::silent);
                }
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

    /// Opens the dialog that turns maintenance from this agent on or off,
    /// as this agent maintains the repository now. The dialog is drawn anew
    /// only when what it offers changed, as `close_modal_when_gone` asks of
    /// the view that holds a dialog; asked again unchanged, the same dialog
    /// opens again.
    fn ask_switch(self) {
        self.switch_refused.set(None);
        let asked = Some(self.maintained.get_untracked());
        if self.switch.get_untracked() != asked {
            self.switch.set(asked);
        }
        self.switch_open.set(true);
    }

    /// Turns maintenance from this agent off when it `maintained` the
    /// repository as the dialog was asked for, on when it didn't. The
    /// dialog holds still until the daemon answers, and a refusal is an
    /// Error alert inside it, which the dialog says itself.
    fn switch_maintainer(self, maintained: bool) {
        if self.switch_busy.get_untracked() {
            return;
        }
        self.switch_busy.set(true);
        self.switch_refused.set(None);
        let alias = self.alias.get_value();
        let url = self.url(maintenance::maintainer_path(&alias));
        let request = api::MaintainerDto {
            maintain_here: !maintained,
        };
        spawn_local(async move {
            let answer = ws::put(&url, &request).await;
            if !self.alive() {
                return;
            }
            self.switch_busy.set(false);
            self.answered(answer.is_some());
            match answer {
                Some(Posted::Done(_)) => {
                    self.switch_open.set(false);
                    self.maintained.set(!maintained);
                    self.remember.run((alias.clone(), !maintained));
                    let done = wording::switch(maintained, &alias).done;
                    reply(Politeness::Polite, &self.words(&done));
                    // What the history offers to adopt follows who maintains
                    // the repository.
                    self.reload(false);
                }
                Some(Posted::Refused(code, args, detail)) => {
                    let refused = self.refused_sentence(&Refusal { code, args, detail });
                    self.switch_refused.set(Some(refused));
                }
                None => {
                    let unanswered = Sentence::new(&self.plain(msg::UI_SESSION_UNREACHABLE), None);
                    self.switch_refused.set(Some(unanswered));
                }
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
    // A list the daemon refused, as a configuration with a mistake in it is:
    // an Error alert where the repositories to pick from would be (#347),
    // said assertively.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let refused_localizer = Arc::clone(&localizer);
    let alert_localizer = Arc::clone(&localizer);
    let repositories_url = format!("{}/api/v1/repositories", ws::origin());
    spawn_local(async move {
        match ws::get::<RepositoriesDto>(&repositories_url).await {
            Some(Got::Read(read)) => {
                unreachable.set(unreachable_after(true));
                repositories.set(Some(
                    read.repositories
                        .into_iter()
                        .map(|row| (row.alias, row.maintained_here))
                        .collect(),
                ));
            }
            Some(Got::Refused(code, args, detail)) => {
                unreachable.set(unreachable_after(true));
                refused.set(Some(refused_sentence(
                    &refused_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                repositories.set(Some(Vec::new()));
            }
            None => unreachable.set(unreachable_after(false)),
        }
    });
    // Who maintains a repository once its page turned that on or off, so
    // that picking it again shows it so.
    let remember = Callback::new(move |(alias, maintained): (String, bool)| {
        repositories.update(|rows| {
            for row in rows.iter_mut().flatten() {
                if row.0 == alias {
                    row.1 = maintained;
                }
            }
        });
    });
    let picker = Arc::clone(&localizer);
    view! {
        <PageHeading>
            {move || if unreachable.get() { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        {move || { unreachable.get().then(|| view! { <p>{unreachable_body.clone()}</p> }) }}
        {move || refused.get().map(|sentence| refusal_alert(&alert_localizer, sentence))}
        {move || match repositories.get() {
            None if unreachable.get() => ().into_any(),
            None => first_load(loading.clone(), Awaited::Text).into_any(),
            Some(aliases) => {
                aliases
                    .into_iter()
                    .map(|(alias, maintained)| {
                        let label = text_with(
                            &picker,
                            msg::UI_SNAPSHOTS_OF,
                            &[("alias", MsgArg::Text(alias.clone()))],
                        );
                        // A reading that reached no daemon brings no repositories: the
                        // page says it can't reach Ladderback, and waits for nothing.
                        view! {
                            <Button
                                content=Content::Text(label.into())
                                on_press=Callback::new(move |()| {
                                    chosen.set(Some((alias.clone(), maintained)));
                                    picks.update(|picks| *picks = picks.wrapping_add(1));
                                })
                            />
                        }
                    })
                    .collect_view()
                    .into_any()
            }
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
                            remember=remember
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
/// maintenance. Either way the page offers to turn maintenance from this
/// agent on or off, after a dialog that says what that means.
#[component]
fn Repository(
    localizer: Arc<Localizer>,
    alias: String,
    maintained: bool,
    unreachable: RwSignal<bool>,
    picks: RwSignal<u32>,
    remember: Callback<(String, bool)>,
) -> impl IntoView {
    let page = Page::new(localizer, alias.clone(), maintained, unreachable, remember);
    Effect::new(move |_| {
        picks.track();
        untrack(|| page.reload(true));
    });
    let heading = page.text(
        msg::UI_MAINTENANCE_HEADING,
        &[("alias", MsgArg::Text(alias.clone()))],
    );
    let stats_heading = page.text(msg::UI_STATS_HEADING, &[("alias", MsgArg::Text(alias))]);
    let intro = move || {
        page.plain(if page.maintained.get() {
            msg::UI_MAINTENANCE_BODY
        } else {
            msg::UI_MAINTENANCE_ELSEWHERE
        })
    };
    let buttons = move || {
        page.maintained.get().then(|| {
            view! {
                <p class="actions">
                    <Button
                        content=Content::IconAndText {
                            glyph: LucideGlyph::Trash.into(),
                            label: page.plain(msg::UI_MAINTENANCE_FORGET).into(),
                        }
                        tone=Tone::Error
                        id=FORGET_BUTTON
                        on_press=Callback::new(move |()| {
                            page.preview(
                                maintenance::request(Kind::Forget),
                                FORGET_BUTTON.to_owned(),
                            );
                        })
                    />
                    <Button
                        content=Content::IconAndText {
                            glyph: LucideGlyph::Trash.into(),
                            label: page.plain(msg::UI_MAINTENANCE_PRUNE).into(),
                        }
                        tone=Tone::Error
                        id=PRUNE_BUTTON
                        on_press=Callback::new(move |()| {
                            page.preview(
                                maintenance::request(Kind::Prune),
                                PRUNE_BUTTON.to_owned(),
                            );
                        })
                    />
                    <Button
                        content=Content::Text(page.plain(msg::UI_MAINTENANCE_CHECK).into())
                        id=CHECK_BUTTON
                        on_press=Callback::new(move |()| {
                            page.preview(
                                maintenance::request(Kind::Check),
                                CHECK_BUTTON.to_owned(),
                            );
                        })
                    />
                </p>
            }
        })
    };
    // One button whose words follow who maintains the repository, so focus
    // stays on it once its dialog has changed that.
    let switch_label = move || {
        let switch = wording::switch(page.maintained.get(), &page.alias.get_value());
        page.words(&switch.button)
    };
    let cancel = page.plain(msg::UI_MAINTENANCE_CANCEL);
    let loading = page.plain(msg::UI_LOADING);
    let empty = page.plain(msg::UI_NO_SNAPSHOTS);
    // Each part of the panel has a memo of its own, so that a change of one
    // does not draw the others again, and a refusal read again in the same
    // words is not said again (`say_refusals`).
    let step = Memo::new(move |_| page.panel.with(|panel| panel.step));
    let run = Memo::new(move |_| page.panel.with(|panel| panel.run.clone()));
    let ended = Memo::new(move |_| page.panel.with(|panel| panel.ended));
    let findings = Memo::new(move |_| page.panel.with(|panel| panel.findings.clone()));
    let refused = Memo::new(move |_| page.panel.with(|panel| panel.refusal.clone()));
    say_refusals(Signal::derive(move || {
        refused.get().map(|refused| page.refused_sentence(&refused))
    }));
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
            <p class="actions">
                <Button
                    content=Content::Text(Signal::derive(switch_label))
                    id=MAINTAINER_BUTTON
                    on_press=Callback::new(move |()| page.ask_switch())
                />
            </p>
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
                        refusal_alert(&page.localizer.get_value(), page.refused_sentence(&refused))
                    })
            }}
            {move || {
                run.get()
                    .map(|run| {
                        let shown = page.words(&wording::told(Told::Running(run.kind)));
                        view! {
                            <p>{shown}</p>
                            <p>
                                <Button
                                    content=Content::Text(cancel.clone().into())
                                    on_press=Callback::new(move |()| page.cancel())
                                />
                            </p>
                        }
                    })
            }}
            {move || {
                ended.get().map(|told| view! { <p>{page.words(&wording::told(told))}</p> })
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
            Shown::Loading => first_load(loading.clone(), Awaited::table(2)).into_any(),
            Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
            Shown::Nothing => ().into_any(),
            Shown::Lists => {
                (move || page.groups.get().map(|groups| groups_view(page, groups))).into_any()
            }
        }}
        {stats_view(page, stats_heading)}
        {move || page.review.get().map(|reviewing| review_view(page, reviewing))}
        {move || page.switch.get().map(|maintained| switch_view(page, maintained))}
    }
}

/// The repository's stats, after its history: the table of the reading
/// shown, when it was taken, and the button that reads them now, which keeps
/// focus while the read is out and after (§16, decision 104).
fn stats_view(page: Page, heading: String) -> impl IntoView {
    let loading = page.plain(msg::UI_LOADING);
    let never = page.plain(msg::UI_STATS_NONE);
    let read = page.plain(msg::UI_STATS_READ);
    let reading_now = page.plain(msg::UI_STATS_READING);
    // A memo for each part, so a change of one does not draw the others
    // again, and a refusal read again in the same words is not said again.
    let shown = Memo::new(move |_| page.stats.with(StatsPanel::shown));
    let latest = Memo::new(move |_| page.stats.with(|panel| panel.latest.clone()));
    let reading = Memo::new(move |_| page.stats.with(|panel| panel.reading));
    let refused = Memo::new(move |_| page.stats.with(|panel| panel.refusal.clone()));
    say_refusals(Signal::derive(move || {
        refused.get().map(|refused| page.refused_sentence(&refused))
    }));
    view! {
        <section aria-labelledby=STATS_HEADING>
            <h2 id=STATS_HEADING>{heading}</h2>
            {move || match shown.get() {
                stats::Shown::Loading => first_load(loading.clone(), Awaited::table(2)).into_any(),
                stats::Shown::Never => view! { <p>{never.clone()}</p> }.into_any(),
                stats::Shown::Table => {
                    (move || latest.get().map(|reading| stats_table(page, reading))).into_any()
                }
                stats::Shown::Nothing => ().into_any(),
            }}
            <p class="actions">
                <Button
                    content=Content::Text(read.into())
                    busy=reading
                    on_press=Callback::new(move |()| page.read_stats())
                />
            </p>
            {move || reading.get().then(|| view! { <p>{reading_now.clone()}</p> })}
            {move || {
                refused
                    .get()
                    .map(|refused| {
                        refusal_alert(&page.localizer.get_value(), page.refused_sentence(&refused))
                    })
            }}
        </section>
    }
}

/// The table of a reading, named by the section's heading: a row for each
/// figure, headed by what it is, and, after it, when the reading was taken.
fn stats_table(page: Page, reading: StatsReadingDto) -> impl IntoView {
    let localizer = page.localizer.get_value();
    let figure = Column::new(
        page.plain(msg::UI_STATS_COLUMN_FIGURE),
        Rem::tenths(120),
        move |row: Signal<stats::Row>| row.with_untracked(|row| page.plain(row.header)).into_any(),
    );
    let value = Column::new(
        page.plain(msg::UI_STATS_COLUMN_VALUE),
        Rem::tenths(200),
        move |row: Signal<stats::Row>| row.with_untracked(|row| page.words(&row.figure)).into_any(),
    );
    let sentences = Sentences {
        loading: text(&localizer, msg::UI_LOADING),
        empty: text(&localizer, msg::UI_STATS_NONE),
        unreachable: text(&localizer, msg::UI_UNREACHABLE_BODY),
    };
    let taken = stats::taken(&reading, page.viewer.get_value().as_ref())
        .map(|words| view! { <p>{page.words(&words)}</p> });
    view! {
        <Table
            name=TableName::Heading(STATS_HEADING.to_owned())
            row_header=figure
            columns=vec![value]
            rows=Signal::stored(stats::rows(&reading))
            key=|row: &stats::Row| row.header
            state=State::Ready
            sentences=sentences
        />
        {taken}
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

/// The offset of the viewer's own clock at `at`, as the browser reads it:
/// `Date.prototype.getTimezoneOffset`, the minutes it runs behind UTC then.
/// A listed snapshot's time names its clock's offset only when it differs
/// from this one (`routes::taken`, §16, decision 56). The browser knows the
/// zone it reads its own clock in even where `jiff`'s copy of the zones has
/// no such name. An answer no clock could give (none, or one past
/// ±25:59:59) is UTC's, the zone the page's formatter reads an instant in
/// without one. It runs only in a browser: `formatting.rs` tests it in
/// `just test-wasm`.
pub(crate) fn viewer_offset(at: jiff::Timestamp) -> jiff::tz::Offset {
    let date = web_sys::js_sys::Date::new(&JsValue::from_f64(at.as_millisecond() as f64));
    // Minutes behind UTC, which can have a fraction for an old local mean
    // time; `as` makes no answer (NaN) zero and saturates a huge one.
    let seconds = (-date.get_timezone_offset() * 60.0).round() as i32;
    jiff::tz::Offset::from_seconds(seconds).unwrap_or(jiff::tz::Offset::UTC)
}

/// A list of snapshots: the table of the ones listed, newest first as the
/// daemon names them, and what is left out; or the empty state.
fn snapshots_view(page: Page, caption: String, snapshots: Vec<SnapshotDto>, rows: Rows) -> AnyView {
    if rows.is_empty() {
        return view! { <p>{page.plain(msg::UI_NO_SNAPSHOTS)}</p> }.into_any();
    }
    let table = snapshots_table(
        &page.localizer.get_value(),
        caption,
        snapshots,
        page.viewer.get_value(),
        Some(page.alias.get_value()),
    );
    let more = wording::more(rows.more).map(|words| view! { <p>{page.words(&words)}</p> });
    view! {
        {table}
        {more}
    }
    .into_any()
}

/// A table of snapshots (§4.5), named by its `caption`: each snapshot's short
/// ID as its row's header, a link to the snapshot where its repository is
/// `alias`, and the time it was taken, written as [`snapshot_time`] writes it
/// in the viewer's `zone`, in a `<time>` that gives the instant to a program.
pub(crate) fn snapshots_table(
    localizer: &Arc<Localizer>,
    caption: String,
    snapshots: Vec<SnapshotDto>,
    zone: Option<api::IanaZone>,
    alias: Option<String>,
) -> AnyView {
    let id_localizer = Arc::clone(localizer);
    let snapshot = Column::new(
        text(localizer, msg::UI_COLUMN_SNAPSHOT),
        Rem::tenths(100),
        move |row: Signal<SnapshotDto>| {
            let id = row.with_untracked(|snapshot| snapshot.id.clone());
            let short = short_id(&id);
            // An ID the link's rules refuse as a path of this site shows as
            // text, never a link that could leave it.
            match alias.as_deref().and_then(|alias| snapshot_link(alias, &id)) {
                Some(to) => view! {
                    <Link to=to localizer=Arc::clone(&id_localizer)>
                        {short}
                    </Link>
                }
                .into_any(),
                None => short.into_any(),
            }
        },
    )
    .kept_whole(SHORT_ID_ROOM);
    // Whether the host writes this locale's instants, asked once per table.
    let writes_instants = localizer.writes_instants();
    let time_localizer = Arc::clone(localizer);
    let time = Column::new(
        text(localizer, msg::UI_COLUMN_TIME),
        Rem::tenths(200),
        move |row: Signal<SnapshotDto>| {
            row.with_untracked(|snapshot| {
                let shown = snapshot_time(
                    &time_localizer,
                    &snapshot.time,
                    snapshot.offset,
                    zone.as_ref(),
                    viewer_offset,
                    writes_instants,
                );
                let raw = snapshot
                    .time
                    .parse::<jiff::Timestamp>()
                    .is_ok()
                    .then(|| snapshot.time.clone());
                time_view(shown, raw)
            })
        },
    );
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_NO_SNAPSHOTS),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=TableName::Caption(caption)
            row_header=snapshot
            columns=vec![time]
            rows=Signal::stored(snapshots)
            key=|snapshot: &SnapshotDto| snapshot.id.clone()
            state=State::Ready
            sentences=sentences
        />
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
        .filter(|_| page.maintained.get())
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
            .map(|path| {
                view! {
                    <li>
                        <bdi>{path.clone()}</bdi>
                    </li>
                }
            })
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
        .filter(|_| page.maintained.get())
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
            <Button
                content=Content::Text(label.into())
                id=id
                on_press=Callback::new(move |()| page.preview(request.clone(), opener.clone()))
            />
        </p>
    }
}

/// The dialog that turns maintenance of the repository from this agent on or
/// off, as this agent `maintained` it when the dialog was asked for: what
/// that means, then Cancel, where focus starts, and the button that does it.
fn switch_view(page: Page, maintained: bool) -> impl IntoView {
    let switch = wording::switch(maintained, &page.alias.get_value());
    let confirm = Confirmation {
        label: page.words(&switch.confirm),
        tone: Tone::Primary,
        action: Callback::new(move |()| page.switch_maintainer(maintained)),
    };
    let working = Working {
        active: page.switch_busy.into(),
        status: page.plain(msg::UI_MAINTAINER_SAVING),
    };
    view! {
        <ReviewDialog
            open=page.switch_open
            title=page.words(&switch.title)
            dismiss_label=page.plain(msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=MAINTAINER_BUTTON.to_owned()
            described_by=MAINTAINER_BODY.to_owned()
            working=working
            fallback=MAINTENANCE_HEADING.to_owned()
        >
            <p id=MAINTAINER_BODY>{page.words(&switch.body)}</p>
            {move || {
                page.switch_refused
                    .get()
                    .map(|sentence| dialog_refusal_alert(&page.localizer.get_value(), sentence))
            }}
        </ReviewDialog>
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
            tone: wording::confirmation_tone(confirm),
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
