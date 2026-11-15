//! The Plans page, the route `/plans` (§4.4, decision 96): the plans of the
//! configuration file, each changed in a form and deleted after a dialog
//! that says what becomes of its snapshots, and new ones added. What each
//! field shows, the plan a form sends, which field a refusal is about, the
//! words of each button and announcement and when a saved plan waits for its
//! next run are `plans.rs`'s, host-tested; this is the DOM glue, which the
//! `@ui` scenarios of `features/ui/plans.feature` run in Chromium.

use std::sync::Arc;
use std::time::Duration;

use api::{IanaZone, MsgArg, PlanDto, PlanSummaryDto, PlansDto, RepositoriesDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::{
    Checkbox, Confirmation, ErrorSummary, FormField, ReviewDialog, SummaryItem, Working,
    focus_element, hand_focus_when_gone, sentence_view,
};
use crate::live::reply;
use crate::plans::{
    ADD_ID, DELETE_BODY_ID, FORM_HEADING_ID, Field, Kind, NEXT_RUN_EVERY, NEXT_RUN_TRIES,
    PLANS_PATH, PlanForm, READ_PATIENCE, REPOSITORIES_PATH, Said, awaits_next_run, change_id,
    change_label, delete_id, delete_label, delete_successor, delete_title, deleted, error_of,
    form_change_heading, new_plan, plan_path, refusal_of, saved, saved_id,
};
use crate::repositories::Sentence;
use crate::routes::{PAGE_HEADING_ID, PageHeading, next_run_cell, refused_sentence, text_with};
use crate::text;
use crate::ws::{self, Got, Phase, Posted};

/// The text of `said` in the page's language.
fn say(localizer: &Localizer, (id, args): Said) -> String {
    text_with(localizer, id, &args)
}

/// The sentence said when the daemon did not answer a request.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_SESSION_UNREACHABLE), None)
}

/// The plans list as the page last read it: where the reading stands, the
/// plans (`None` until the read answers, and while it is refused), and a
/// read the daemon refused, said in the table's place.
#[derive(Clone, Copy)]
struct List {
    base: StoredValue<String>,
    phase: RwSignal<Phase>,
    plans: RwSignal<Option<Vec<PlanSummaryDto>>>,
    refused: RwSignal<Option<Sentence>>,
}

impl List {
    fn new() -> Self {
        Self {
            base: StoredValue::new(ws::origin()),
            phase: RwSignal::new(Phase::Loading),
            plans: RwSignal::new(None),
            refused: RwSignal::new(None),
        }
    }

    fn url(self, path: &str) -> String {
        format!("{}{path}", self.base.get_value())
    }

    /// Reads the plans list again.
    async fn read(self, localizer: &Localizer) {
        let read = ws::get::<PlansDto>(&self.url(PLANS_PATH)).await;
        // The reader may have left the page before the answer came.
        if self.phase.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                self.refused.set(None);
                self.plans.set(Some(read.plans));
                self.phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                self.refused.set(Some(refused_sentence(
                    localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                self.plans.set(None);
                self.phase.set(Phase::Ready);
            }
            None => self.phase.set(Phase::Unreachable),
        }
    }

    /// Reads the list again while the saved plan `id` waits for the
    /// scheduler to plan its next run, as many times as `NEXT_RUN_TRIES`
    /// allows, so its row shows the run once there is one.
    async fn await_next_run(self, id: String) {
        // The reader may have left the page while the form closed.
        let Some(base) = self.base.try_get_value() else {
            return;
        };
        let url = format!("{base}{PLANS_PATH}");
        for _ in 0..NEXT_RUN_TRIES {
            let waiting = self.plans.try_with_untracked(|plans| {
                plans
                    .as_deref()
                    .is_some_and(|plans| awaits_next_run(plans, &id))
            });
            if waiting != Some(true) {
                return;
            }
            gloo_timers::future::sleep(NEXT_RUN_EVERY).await;
            if let Some(read) = ws::poll::<PlansDto>(&url, READ_PATIENCE).await
                && self.plans.try_set(Some(read.plans)).is_some()
            {
                return;
            }
        }
    }
}

/// The plan form that is open: the plan it changes (`None` for a new one),
/// the plan as it came, and the button that opened it, where focus returns
/// once it is saved or closed.
#[derive(Clone)]
struct Open {
    id: Option<String>,
    base: PlanDto,
    opener: String,
}

/// The Plans page: the heading, which says when the daemon can't be reached,
/// the table of plans, the button that adds one, the plan form while one is
/// open, and the dialog that asks before a plan is deleted.
#[component]
pub(crate) fn Plans(localizer: Arc<Localizer>) -> impl IntoView {
    let list = List::new();
    let read_localizer = Arc::clone(&localizer);
    spawn_local(async move { list.read(&read_localizer).await });

    // The form, and why it could not open, said in its place.
    let editing = RwSignal::<Option<Open>>::new(None);
    let not_opened = RwSignal::<Option<Sentence>>::new(None);
    let change_localizer = Arc::clone(&localizer);
    let change = Callback::new(move |id: String| {
        not_opened.set(None);
        let url = list.url(&plan_path(&id));
        let localizer = Arc::clone(&change_localizer);
        spawn_local(async move {
            match ws::get::<PlanDto>(&url).await {
                Some(Got::Read(base)) => {
                    let opener = change_id(&id);
                    editing.try_set(Some(Open {
                        id: Some(id),
                        base,
                        opener,
                    }));
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
    let add = move |_| {
        not_opened.set(None);
        let url = list.url(REPOSITORIES_PATH);
        spawn_local(async move {
            // A list that can't be read starts the plan with no repository,
            // which the daemon then refuses at its field.
            let repositories = match ws::get::<RepositoriesDto>(&url).await {
                Some(Got::Read(read)) => read.repositories,
                _ => Vec::new(),
            };
            editing.try_set(Some(Open {
                id: None,
                base: new_plan(&repositories),
                opener: ADD_ID.to_owned(),
            }));
        });
    };
    let close = Callback::new(move |()| editing.set(None));

    // The delete dialog: the plan it is about, and what deleting came to.
    let deleting = RwSignal::<Option<String>>::new(None);
    let delete_open = RwSignal::new(false);
    let delete_busy = RwSignal::new(false);
    let delete_error = RwSignal::<Option<Sentence>>::new(None);
    let ask_delete = Callback::new(move |id: String| {
        delete_error.set(None);
        deleting.set(Some(id));
        delete_open.set(true);
    });
    let delete_localizer = Arc::clone(&localizer);
    let delete = Callback::new(move |id: String| {
        if delete_busy.get_untracked() {
            return;
        }
        delete_busy.set(true);
        delete_error.set(None);
        let localizer = Arc::clone(&delete_localizer);
        let url = list.url(&plan_path(&id));
        spawn_local(async move {
            let answer = ws::delete(&url).await;
            if delete_busy.try_get_untracked().is_none() {
                return;
            }
            delete_busy.set(false);
            match answer {
                Some(Posted::Done(_)) => {
                    delete_open.set(false);
                    reply(Politeness::Polite, &say(&localizer, deleted(&id)));
                    list.read(&localizer).await;
                    let on_it = editing
                        .try_with_untracked(|open| {
                            open.as_ref()
                                .is_some_and(|open| open.id.as_deref() == Some(id.as_str()))
                        })
                        .unwrap_or(false);
                    if on_it {
                        editing.try_set(None);
                    }
                }
                Some(Posted::Refused(code, args, detail)) => delete_error.set(Some(
                    refused_sentence(&localizer, code, &args, detail.as_deref()),
                )),
                None => delete_error.set(Some(unanswered(&localizer))),
            }
        });
    });

    let heading = text(&localizer, msg::UI_PLANS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_PLANS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let none = text(&localizer, msg::UI_PLANS_NONE);
    let add_label = text(&localizer, msg::UI_PLAN_ADD);
    // What the list's place shows; a memo, so a read again that changes
    // only the plans' rows keeps the table, and the rows follow in place.
    let shape = Memo::new(move |_| {
        (
            list.phase.get(),
            list.refused.get(),
            list.plans
                .with(|plans| plans.as_ref().is_some_and(|plans| !plans.is_empty())),
        )
    });
    let table_localizer = Arc::clone(&localizer);
    let form_localizer = Arc::clone(&localizer);
    let dialog_localizer = Arc::clone(&localizer);
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
        {move || match shape.get() {
            (Phase::Loading, _, _) => view! { <p>{loading.clone()}</p> }.into_any(),
            (Phase::Unreachable, _, _) => view! { <p>{unreachable.clone()}</p> }.into_any(),
            (Phase::Ready, Some(sentence), _) => {
                view! { <p role="alert">{sentence_view(sentence)}</p> }.into_any()
            }
            (Phase::Ready, None, true) => {
                plans_table(Arc::clone(&table_localizer), list, change, ask_delete).into_any()
            }
            (Phase::Ready, None, false) => view! { <p>{none.clone()}</p> }.into_any(),
        }}
        <p class="actions">
            <button type="button" id=ADD_ID on:click=add>
                {add_label}
            </button>
        </p>
        {move || {
            not_opened.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
        }}
        {move || {
            editing
                .get()
                .map(|open| {
                    view! {
                        <PlanEditor
                            localizer=Arc::clone(&form_localizer)
                            list=list
                            open=open
                            close=close
                        />
                    }
                })
        }}
        {move || {
            deleting
                .get()
                .map(|id| {
                    view! {
                        <DeleteDialog
                            localizer=Arc::clone(&dialog_localizer)
                            id=id
                            open=delete_open
                            busy=delete_busy
                            error=delete_error
                            delete=delete
                        />
                    }
                })
        }}
    }
}

/// The table of plans, named by the page's heading. Its rows are keyed by
/// the plan's ID, so a read again changes a row's cells in place and never
/// replaces a button a reader has focused.
fn plans_table(
    localizer: Arc<Localizer>,
    list: List,
    change: Callback<String>,
    ask_delete: Callback<String>,
) -> impl IntoView {
    // Each next run is written in the viewer's own zone, as every instant
    // the page shows is (§8.4).
    let viewer = l10n::viewer_zone();
    let head = (
        text(&localizer, msg::UI_COLUMN_PLAN),
        text(&localizer, msg::UI_PLANS_COLUMN_REPOSITORY),
        text(&localizer, msg::UI_COLUMN_NEXT_RUN),
        text(&localizer, msg::UI_COLUMN_ACTIONS),
    );
    view! {
        <table class="plans" aria-labelledby=PAGE_HEADING_ID>
            <thead>
                <tr>
                    <th scope="col">{head.0}</th>
                    <th scope="col">{head.1}</th>
                    <th scope="col">{head.2}</th>
                    <th scope="col">{head.3}</th>
                </tr>
            </thead>
            <tbody>
                <For
                    each=move || list.plans.get().unwrap_or_default()
                    key=|plan: &PlanSummaryDto| plan.id.clone()
                    children=move |plan: PlanSummaryDto| {
                        plan_row(
                            Arc::clone(&localizer),
                            list,
                            plan.id,
                            viewer.clone(),
                            change,
                            ask_delete,
                        )
                    }
                />
            </tbody>
        </table>
    }
}

/// A plan's row: its ID as the row's header, its repository, its next run
/// (the instant in the cell's `title`), and its Change and Delete buttons,
/// named for it. Each button hands focus to the page's heading when its row
/// goes while it holds focus, as after a delete.
fn plan_row(
    localizer: Arc<Localizer>,
    list: List,
    id: String,
    viewer: Option<IanaZone>,
    change: Callback<String>,
    ask_delete: Callback<String>,
) -> impl IntoView {
    let (change_text, delete_text) = (
        say(&localizer, change_label(&id)),
        say(&localizer, delete_label(&id)),
    );
    let row_id = id.clone();
    let summary = Memo::new(move |_| {
        list.plans.with(|plans| {
            plans
                .iter()
                .flatten()
                .find(|plan| plan.id == row_id)
                .map(|plan| {
                    let (shown, title) = next_run_cell(&localizer, plan, viewer.as_ref());
                    (plan.repository.clone(), shown, title)
                })
                .unwrap_or_default()
        })
    });
    let change_button = NodeRef::<leptos::html::Button>::new();
    let delete_button = NodeRef::<leptos::html::Button>::new();
    hand_focus_when_gone(change_button, PAGE_HEADING_ID.to_owned());
    hand_focus_when_gone(delete_button, PAGE_HEADING_ID.to_owned());
    let (change_for, delete_for) = (id.clone(), id.clone());
    view! {
        <tr>
            <th scope="row">{id.clone()}</th>
            <td>{move || summary.with(|(repository, _, _)| repository.clone())}</td>
            <td title=move || {
                summary.with(|(_, _, title)| title.clone())
            }>{move || summary.with(|(_, shown, _)| shown.clone())}</td>
            <td>
                <div class="actions">
                    <button
                        type="button"
                        id=change_id(&id)
                        node_ref=change_button
                        on:click=move |_| change.run(change_for.clone())
                    >
                        {change_text}
                    </button>
                    <button
                        type="button"
                        id=delete_id(&id)
                        node_ref=delete_button
                        on:click=move |_| ask_delete.run(delete_for.clone())
                    >
                        {delete_text}
                    </button>
                </div>
            </td>
        </tr>
    }
}

/// What a person has typed and chosen in the form, one signal per field,
/// with the error each field shows, and the summary of what is wrong, set
/// together so they never disagree.
#[derive(Clone, Copy)]
struct Inputs {
    fields: [(Field, RwSignal<String>, RwSignal<Option<String>>); 18],
    git_ignore: RwSignal<bool>,
    summary: RwSignal<Vec<SummaryItem>>,
}

impl Inputs {
    /// The inputs of `form`, each holding what the form shows.
    fn of(form: &PlanForm) -> Self {
        Self {
            fields: Field::ALL.map(|field| {
                let text = RwSignal::new(form.text(field).to_owned());
                (field, text, RwSignal::new(None))
            }),
            git_ignore: RwSignal::new(form.git_ignore),
            summary: RwSignal::new(Vec::new()),
        }
    }

    /// The text and the error of `field`.
    fn input(self, field: Field) -> Option<(RwSignal<String>, RwSignal<Option<String>>)> {
        self.fields
            .into_iter()
            .find(|(each, _, _)| *each == field)
            .map(|(_, text, error)| (text, error))
    }

    fn read(self) -> PlanForm {
        let mut form = PlanForm::default();
        for (field, text, _) in self.fields {
            if field.kind() != Kind::Flag {
                form.set_text(field, text.get_untracked());
            }
        }
        form.git_ignore = self.git_ignore.get_untracked();
        form
    }

    fn show(self, items: Vec<SummaryItem>) {
        for (field, _, error) in self.fields {
            error.set(error_of(&items, field));
        }
        self.summary.set(items);
    }
}

/// The plan form: its heading, which takes focus as it opens, a field for
/// each setting of a plan in four groups (the Name only for a new plan,
/// whose ID it is), the error summary that takes focus when the daemon
/// refuses the plan, and the buttons that save it and close it. Once saved,
/// the form closes, the list is read again and focus returns to the button
/// that opened the form; the row of a plan on a schedule then shows its next
/// run as soon as the scheduler has planned one.
#[component]
fn PlanEditor(
    localizer: Arc<Localizer>,
    list: List,
    open: Open,
    close: Callback<()>,
) -> impl IntoView {
    let Open { id, base, opener } = open;
    let inputs = Inputs::of(&PlanForm::of(&base));
    let busy = RwSignal::new(false);
    let heading_ref = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading_ref.get() {
            // A failed focus leaves focus where it was.
            let _ = heading.focus();
        }
    });
    let heading = match &id {
        Some(id) => say(&localizer, form_change_heading(id)),
        None => text(&localizer, msg::UI_PLAN_FORM_NEW),
    };
    let new = id.is_none();
    let cancel_opener = opener.clone();
    let request = StoredValue::new((id, base, opener));
    let save_localizer = Arc::clone(&localizer);
    let save = move || {
        if busy.get_untracked() {
            return;
        }
        let (id, base, opener) = request.get_value();
        let sent = inputs.read().request(&base);
        busy.set(true);
        reply(
            Politeness::Polite,
            &text(&save_localizer, msg::UI_PLAN_SAVING),
        );
        let localizer = Arc::clone(&save_localizer);
        spawn_local(async move {
            let answer = match &id {
                Some(id) => ws::put(&list.url(&plan_path(id)), &sent).await,
                None => ws::post(&list.url(PLANS_PATH), &sent).await,
            };
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            match answer {
                Some(Posted::Done(answer)) => {
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    let Some(saved_as) = saved_id(&answer) else {
                        list.phase.set(Phase::Unreachable);
                        return;
                    };
                    reply(
                        Politeness::Polite,
                        &say(&localizer, saved(&saved_as, &base, &sent)),
                    );
                    list.read(&localizer).await;
                    // The reader may have left the page while it was read.
                    if close.try_run(()).is_none() {
                        return;
                    }
                    // Once the form has left the page and the list shows
                    // the plan, whose Change button may be new.
                    gloo_timers::future::sleep(Duration::ZERO).await;
                    focus_element(&opener);
                    list.await_next_run(saved_as).await;
                }
                Some(Posted::Refused(code, args, detail)) => {
                    let sentence = refused_sentence(&localizer, code, &args, detail.as_deref());
                    inputs.show(vec![refusal_of(
                        code,
                        &args,
                        |message| text(&localizer, message),
                        sentence,
                    )]);
                }
                None => inputs.show(vec![SummaryItem {
                    field: None,
                    sentence: unanswered(&localizer),
                }]),
            }
        });
    };
    let cancel = move |_| {
        close.run(());
        focus_element(&cancel_opener);
    };

    let field_view = |field: Field| {
        let label = say(&localizer, field.label());
        let hint = say(&localizer, field.hint());
        let Some((value, error)) = inputs.input(field) else {
            return ().into_any();
        };
        match field.kind() {
            Kind::Flag => view! { <Checkbox id=field.id() label=label description=hint checked=inputs.git_ignore /> }
            .into_any(),
            Kind::Lines => view! { <FormField id=field.id() label=label description=hint error=error value=value lines=3 /> }
            .into_any(),
            Kind::Text | Kind::Count => view! { <FormField id=field.id() label=label description=hint error=error value=value /> }
            .into_any(),
        }
    };
    let group = |legend, fields: &[Field]| {
        view! {
            <fieldset class="choices">
                <legend>{text(&localizer, legend)}</legend>
                {fields.iter().map(|field| field_view(*field)).collect_view()}
            </fieldset>
        }
    };
    let name = new.then(|| field_view(Field::Name));
    let repository = field_view(Field::Repository);
    let groups = (
        group(msg::UI_PLAN_GROUP_SOURCES, &Field::SOURCES),
        group(msg::UI_PLAN_GROUP_SCHEDULE, &[Field::Cron]),
        group(msg::UI_PLAN_GROUP_RETENTION, &Field::RETENTION),
        group(msg::UI_PLAN_GROUP_HOOKS, &Field::HOOKS),
    );
    let save_label = text(&localizer, msg::UI_PLAN_SAVE);
    let cancel_label = text(&localizer, msg::UI_PLAN_CANCEL);
    let saving = text(&localizer, msg::UI_PLAN_SAVING);
    let summary_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=FORM_HEADING_ID>
            <h2 id=FORM_HEADING_ID tabindex="-1" node_ref=heading_ref>
                {heading}
            </h2>
            {move || {
                let items = inputs.summary.get();
                (!items.is_empty())
                    .then(|| {
                        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                        let heading = text_with(
                            &summary_localizer,
                            msg::UI_PLAN_PROBLEMS,
                            &[("count", MsgArg::Count(count))],
                        );
                        view! { <ErrorSummary heading=heading items=items advice=None /> }
                    })
            }}
            <form
                aria-labelledby=FORM_HEADING_ID
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    save();
                }
            >
                {name}
                {repository}
                {groups}
                <p class="actions">
                    <button type="submit">{save_label}</button>
                    <button type="button" on:click=cancel>
                        {cancel_label}
                    </button>
                </p>
            </form>
            {move || busy.get().then(|| view! { <p>{saving.clone()}</p> })}
        </section>
    }
}

/// The dialog that asks before the plan `id` is deleted, named for it, and
/// says what becomes of its snapshots: they stay in the repository, nothing
/// forgets them on its own any more (I6), and a new plan with its ID would
/// take them over. Cancel comes first, so a stray Enter backs out. While the
/// daemon deletes the plan, the dialog holds still and says so; a refusal is
/// an alert inside it.
#[component]
fn DeleteDialog(
    localizer: Arc<Localizer>,
    id: String,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<Sentence>>,
    delete: Callback<String>,
) -> impl IntoView {
    let title = say(&localizer, delete_title(&id));
    let successor = say(&localizer, delete_successor(&id));
    let returns_to = delete_id(&id);
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_PLAN_DELETE_CONFIRM),
        action: Callback::new(move |()| delete.run(id.clone())),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_PLAN_DELETING),
    };
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=returns_to
            described_by=DELETE_BODY_ID.to_owned()
            working=working
            fallback=PAGE_HEADING_ID.to_owned()
        >
            <div id=DELETE_BODY_ID>
                <p>{text(&localizer, msg::UI_PLAN_DELETE_STAY)}</p>
                <p>{text(&localizer, msg::UI_PLAN_DELETE_FORGET)}</p>
                <p>{successor}</p>
            </div>
            {move || {
                error.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
            }}
        </ReviewDialog>
    }
}
