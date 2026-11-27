//! The Repositories page, the route `/repositories` (§4.4): the repositories
//! Ladderback backs up to, each row's Test and Create, and the form that
//! tests one, connects one that exists and creates one that doesn't. Its
//! decisions are `repositories.rs`'s; this is the DOM glue, which the `@ui`
//! scenarios of `features/ui/repositories.feature` run, with the form's own
//! state tested on the host below.

use std::collections::HashMap;
use std::sync::Arc;

use api::{
    LocationDto, LocationTriedDto, MsgArg, RepositoriesDto, RepositoryDto, RepositoryNotesDto,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::ACTIONS;
use crate::components::alert::rules::AlertTone;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Content, Kind};
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Alert, Banner, Button, Checkbox, Column, ColumnsChoice, ErrorSummary, Name as TableName,
    RadioGroup, Sentences, State, SummaryItem, Table, TextField, address_view, refusal_alert,
    runs_view, say_refusals, text_field,
};
use crate::components::{first_load, paced};
use crate::live::reply;
use crate::repositories::{
    Action, Field, Form, Outcome, RowCreated, Sentence, advises_retry, create_label,
    create_saved_path, error_of, field_of, found_at, lists_locations, location_heading,
    location_option, location_rule, location_tested, locations_label, locations_tested,
    maintainer_note, password_source, problems_of, repository_notes, rereads_list, row_refusal,
    test_locations_label, test_locations_path, tested_announcement,
};
use crate::routes::{PAGE_HEADING_ID, PageHeading, refusal, refused_sentence, text_with};
use crate::text;
use crate::variants::{Rem, Size};
use crate::ws::{self, Got, Phase, Posted};

/// What a person has typed into the Repositories form, one signal per field,
/// and whether they asked this agent to maintain the repository.
#[derive(Clone, Copy)]
struct Inputs {
    name: RwSignal<String>,
    location: RwSignal<String>,
    hot_storage: RwSignal<String>,
    password: RwSignal<String>,
    maintain_here: RwSignal<bool>,
    /// The choice of the radio group "Where to keep the password": the
    /// configuration file or the system keyring.
    keep_password: RwSignal<&'static str>,
}

impl Inputs {
    fn new() -> Self {
        Self {
            name: RwSignal::new(String::new()),
            location: RwSignal::new(String::new()),
            hot_storage: RwSignal::new(String::new()),
            password: RwSignal::new(String::new()),
            maintain_here: RwSignal::new(false),
            keep_password: RwSignal::new(KEEP_IN_CONFIGURATION),
        }
    }

    fn of(self, field: Field) -> RwSignal<String> {
        match field {
            Field::Name => self.name,
            Field::Location => self.location,
            Field::HotStorage => self.hot_storage,
            Field::Password => self.password,
        }
    }

    fn read(self) -> Form {
        Form {
            name: self.name.get_untracked(),
            location: self.location.get_untracked(),
            hot_storage: self.hot_storage.get_untracked(),
            password: self.password.get_untracked(),
            maintain_here: self.maintain_here.get_untracked(),
            keep_in_keyring: self.keep_password.get_untracked() == KEEP_IN_KEYRING,
        }
    }

    fn clear(self) {
        for field in FIELDS {
            self.of(field).set(String::new());
        }
        self.maintain_here.set(false);
        self.keep_password.set(KEEP_IN_CONFIGURATION);
    }
}

/// The radio group "Where to keep the password", and the values of its choices.
const KEEP_PASSWORD_GROUP: &str = "keep-password";
const KEEP_IN_CONFIGURATION: &str = "configuration";
const KEEP_IN_KEYRING: &str = "keyring";

/// The form's fields in the order the page shows them.
const FIELDS: [Field; 4] = [
    Field::Name,
    Field::Location,
    Field::HotStorage,
    Field::Password,
];

/// What is wrong with the form: the error each field shows, and the summary
/// that lists them all. Both are set together, so a field and the summary
/// never disagree.
#[derive(Clone, Copy)]
struct Problems {
    summary: RwSignal<Vec<SummaryItem>>,
    advice: RwSignal<Option<String>>,
    name: RwSignal<Option<String>>,
    location: RwSignal<Option<String>>,
    hot_storage: RwSignal<Option<String>>,
    password: RwSignal<Option<String>>,
}

impl Problems {
    fn new() -> Self {
        Self {
            summary: RwSignal::new(Vec::new()),
            advice: RwSignal::new(None),
            name: RwSignal::new(None),
            location: RwSignal::new(None),
            hot_storage: RwSignal::new(None),
            password: RwSignal::new(None),
        }
    }

    fn of(self, field: Field) -> RwSignal<Option<String>> {
        match field {
            Field::Name => self.name,
            Field::Location => self.location,
            Field::HotStorage => self.hot_storage,
            Field::Password => self.password,
        }
    }

    /// Shows `items`, and `advice` after them: each field about which an item
    /// is shows it, and every other field is clear. No items is a clear form.
    fn show(self, items: Vec<SummaryItem>, advice: Option<String>) {
        for field in FIELDS {
            let error = error_of(&items, field);
            self.of(field).set(error);
        }
        self.advice.set(advice);
        self.summary.set(items);
    }
}

/// The repositories page, the route `/repositories`: the repositories
/// Ladderback backs up to, and the form that tests one, connects one that
/// exists and creates one that doesn't (§4.4). Its decisions are
/// `repositories.rs`; this is the glue. A failed submit moves focus to the
/// error summary, and a typed password stays in its field until the
/// repository is saved, never shown again (I4, A5).
#[component]
pub(crate) fn Repositories(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_REPOSITORIES_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_REPOSITORIES_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let empty = text(&localizer, msg::UI_NO_REPOSITORIES);
    let form_heading = text(&localizer, msg::UI_REPOSITORIES_FORM_HEADING);
    let working = text(&localizer, msg::UI_REPOSITORIES_WORKING);
    let (test_label, connect_label, create_label) = (
        text(&localizer, msg::UI_REPOSITORIES_TEST),
        text(&localizer, msg::UI_REPOSITORIES_CONNECT),
        text(&localizer, msg::UI_REPOSITORIES_CREATE),
    );
    let base = StoredValue::new(ws::origin());
    let phase = RwSignal::new(Phase::Loading);
    // The configured repositories; `None` until the first read answers.
    let rows = RwSignal::<Option<Vec<RepositoryDto>>>::new(None);
    // What each row's last Test found at each of its locations, by the row's
    // name; a read of the list forgets it, as the rows may have changed.
    let tested = RwSignal::<HashMap<String, Vec<LocationTriedDto>>>::new(HashMap::new());
    // A list the daemon refused, as a configuration with a mistake in it is:
    // said in the list's place, naming where the mistake is (#347).
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    // What the page notes of the machine above its list, as the catalog
    // writes each code (#923). Read once; an answer that never came notes
    // nothing.
    let notes = RwSignal::<Vec<String>>::new(Vec::new());
    let notes_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let url = format!("{}/api/v1/repositories/_notes", ws::origin());
        if let Some(Got::Read(read)) = ws::get::<RepositoryNotesDto>(&url).await {
            notes.try_set(repository_notes(&notes_localizer, &read.notes));
        }
    });
    let load_localizer = Arc::clone(&localizer);
    let load = Callback::new(move |()| {
        let url = format!("{}/api/v1/repositories", base.get_value());
        let localizer = Arc::clone(&load_localizer);
        spawn_local(async move {
            match ws::get::<RepositoriesDto>(&url).await {
                Some(Got::Read(read)) => {
                    refused.set(None);
                    tested.set(HashMap::new());
                    rows.set(Some(read.repositories));
                    phase.set(Phase::Ready);
                }
                Some(Got::Refused(code, args, detail)) => {
                    refused.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    rows.set(None);
                    phase.set(Phase::Ready);
                }
                None => phase.set(Phase::Unreachable),
            }
        });
    });
    load.run(());
    // What the last of the rows' Creates came to, said below the list.
    let creates = RowCreates {
        phase,
        created: RwSignal::new(None),
        load,
    };

    let inputs = Inputs::new();
    let problems = Problems::new();
    // What the last answer said, and which request is still on its way: its
    // button shows it is busy, the form's others take no press meanwhile.
    let result = RwSignal::<Option<String>>::new(None);
    let result_localizer = Arc::clone(&localizer);
    let running = RwSignal::<Option<Action>>::new(None);
    let submit_localizer = Arc::clone(&localizer);
    let submit = Callback::new(move |action: Action| {
        if running.get_untracked().is_some() {
            return;
        }
        let form = inputs.read();
        result.set(None);
        // What a row's Create came to is the page's last answer no more.
        creates.created.set(None);
        if let Some(items) = problems_of(&form, action, |id| text(&submit_localizer, id)) {
            problems.show(items, None);
            return;
        }
        running.set(Some(action));
        reply(
            Politeness::Polite,
            &text(&submit_localizer, msg::UI_REPOSITORIES_WORKING),
        );
        let localizer = Arc::clone(&submit_localizer);
        let url = format!("{}{}", base.get_value(), action.path());
        spawn_local(async move {
            let posted = match action {
                Action::Test => ws::post(&url, &form.test_request()).await,
                Action::Connect => ws::post(&url, &form.connect_request()).await,
                Action::Create => ws::post(&url, &form.create_request()).await,
            };
            // The reader may have followed a link while the engine worked: the
            // page's signals went with it, reading one would panic, and there
            // is nobody left to tell.
            let Some(phase_now) = phase.try_get_untracked() else {
                return;
            };
            running.set(None);
            // An answer, of any kind, is a daemon that is there.
            if rereads_list(posted.is_some(), phase_now) {
                load.run(());
            }
            match posted {
                Some(Posted::Done(answer)) => match Outcome::of(action, &answer) {
                    Some(outcome) => {
                        let alias = form.alias();
                        let (id, args) = outcome.shown(alias);
                        result.set(Some(text_with(&localizer, id, &args)));
                        let (id, args) = outcome.announced(alias);
                        reply(Politeness::Polite, &text_with(&localizer, id, &args));
                        problems.show(Vec::new(), None);
                        if outcome.saved() {
                            inputs.clear();
                            load.run(());
                        }
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    let shown = refusal(&localizer, code, &args, detail.as_deref());
                    problems.show(
                        vec![SummaryItem {
                            field: field_of(code, &args).map(Field::id),
                            sentence: Sentence::new(&shown, detail.as_deref()),
                        }],
                        advises_retry(code)
                            .then(|| text(&localizer, msg::UI_REPOSITORIES_TRY_AGAIN)),
                    );
                }
                // No answer at all: the page's phase says that its own way.
                None => phase.set(Phase::Unreachable),
            }
        });
    });

    let table_localizer = Arc::clone(&localizer);
    // What the list's place shows; a memo, so a read again that changes only
    // the rows keeps the table, and the rows follow in place.
    let shape = Memo::new(move |_| {
        (
            phase.get(),
            refused.get(),
            rows.with(|rows| rows.as_ref().is_some_and(|rows| !rows.is_empty())),
        )
    });
    let (shape, pace) = paced(move || shape.get(), |shape| shape.0 == Phase::Loading);
    let summary_localizer = Arc::clone(&localizer);
    let note_localizer = Arc::clone(&localizer);
    let field_text = |id| text(&localizer, id);
    view! {
        <PageHeading>
            {move || {
                if phase.get() == Phase::Unreachable {
                    unreachable_heading.clone()
                } else {
                    heading.clone()
                }
            }}
        </PageHeading>
        <p>{body}</p>
        {move || {
            notes
                .get()
                .into_iter()
                .map(|note| {
                    view! {
                        <Banner localizer=Arc::clone(&note_localizer) tone=BannerTone::Warning>
                            {note}
                        </Banner>
                    }
                })
                .collect_view()
        }}
        {move || match shape.get() {
            (Phase::Loading, _, _) => {
                first_load(loading.clone(), Awaited::table(4), pace).into_any()
            }
            (Phase::Unreachable, _, _) => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
            (Phase::Ready, Some(sentence), _) => {
                refusal_alert(&table_localizer, sentence).into_any()
            }
            (Phase::Ready, None, true) => {
                repositories_table(&table_localizer, base.get_value(), creates, rows, tested)
            }
            (Phase::Ready, None, false) => view! { <p>{empty.clone()}</p> }.into_any(),
        }}
        {move || {
            creates
                .created
                .get()
                .map(|(confirmation, id_sentence)| {
                    view! { <CreatedNote confirmation=confirmation id_sentence=id_sentence /> }
                })
        }}
        <section aria-labelledby="repositories-form-heading">
            <h2 id="repositories-form-heading">{form_heading}</h2>
            {move || {
                let items = problems.summary.get();
                (!items.is_empty())
                    .then(|| {
                        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                        let heading = text_with(
                            &summary_localizer,
                            msg::UI_REPOSITORIES_PROBLEMS,
                            &[("count", MsgArg::Count(count))],
                        );
                        view! {
                            <ErrorSummary
                                heading=heading
                                items=items
                                advice=problems.advice.get_untracked()
                            />
                        }
                    })
            }}
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    submit.run(Action::Test);
                }
            >
                <TextField
                    id=Field::Name.id()
                    localizer=Arc::clone(&localizer)
                    label=field_text(msg::UI_REPOSITORIES_NAME_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_NAME_HINT)
                    error=problems.name
                    value=inputs.name
                />
                <TextField
                    id=Field::Location.id()
                    localizer=Arc::clone(&localizer)
                    label=field_text(msg::UI_REPOSITORIES_LOCATION_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_LOCATION_HINT)
                    error=problems.location
                    value=inputs.location
                    required=true
                />
                <TextField
                    id=Field::HotStorage.id()
                    localizer=Arc::clone(&localizer)
                    label=field_text(msg::UI_REPOSITORIES_HOT_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_HOT_HINT)
                    error=problems.hot_storage
                    value=inputs.hot_storage
                />
                <TextField
                    id=Field::Password.id()
                    localizer=Arc::clone(&localizer)
                    label=field_text(msg::UI_REPOSITORIES_PASSWORD_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_PASSWORD_HINT)
                    error=problems.password
                    value=inputs.password
                    kind=text_field::Kind::Password
                    required=true
                />
                <RadioGroup
                    name=KEEP_PASSWORD_GROUP
                    legend=field_text(msg::UI_REPOSITORIES_KEEP_PASSWORD_LEGEND)
                    description=field_text(msg::UI_REPOSITORIES_KEEP_PASSWORD_HINT)
                    options=vec![
                        (
                            KEEP_IN_CONFIGURATION,
                            field_text(msg::UI_REPOSITORIES_KEEP_PASSWORD_CONFIGURATION),
                        ),
                        (KEEP_IN_KEYRING, field_text(msg::UI_REPOSITORIES_KEEP_PASSWORD_KEYRING)),
                    ]
                    chosen=inputs.keep_password
                />
                <Checkbox
                    id="repository-maintain-here"
                    label=field_text(msg::UI_REPOSITORIES_MAINTAIN_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_MAINTAIN_HINT)
                    checked=inputs.maintain_here
                />
                <p class=ACTIONS>
                    <Button
                        content=Content::Text(test_label.into())
                        kind=Kind::Submit
                        busy=Signal::derive(move || running.get() == Some(Action::Test))
                        disabled=Signal::derive(move || running.get().is_some())
                    />
                    <Button
                        content=Content::Text(connect_label.into())
                        busy=Signal::derive(move || running.get() == Some(Action::Connect))
                        disabled=Signal::derive(move || running.get().is_some())
                        on_press=Callback::new(move |()| submit.run(Action::Connect))
                    />
                    <Button
                        content=Content::Text(create_label.into())
                        busy=Signal::derive(move || running.get() == Some(Action::Create))
                        disabled=Signal::derive(move || running.get().is_some())
                        on_press=Callback::new(move |()| submit.run(Action::Create))
                    />
                </p>
            </form>
            {move || running.get().is_some().then(|| view! { <p>{working.clone()}</p> })}
            {move || {
                result
                    .get()
                    .map(|text| {
                        view! {
                            <Alert localizer=Arc::clone(&result_localizer) tone=AlertTone::Success>
                                {text}
                            </Alert>
                        }
                    })
            }}
        </section>
    }
}

/// What a row's Create needs from its page: the page's phase, which an
/// answer that never came sets; where the page says what a Create came to;
/// and the reading of the list, after which a created repository's row
/// offers Create no more.
#[derive(Clone, Copy)]
struct RowCreates {
    phase: RwSignal<Phase>,
    /// What a row's Create came to: the confirmation, then the ID's sentence.
    created: RwSignal<Option<(String, String)>>,
    load: Callback<()>,
}

/// The repositories table (§4.5), named by the page's heading: each
/// repository's name as its row's header, then who maintains it, in a
/// column the reader can't hide (I1, decision 140, #1625), where it is (the
/// hot half on a line of its own), or, for a repository reached at several
/// locations, the list of them in order with its Test (decision 80), where
/// its password comes from, and its Create, when the row offers it
/// (decision 53). Never the password itself (I4). A row follows a read of
/// the list in place; its buttons are built again only when what the row
/// offers changes, as a created repository's Create goes.
fn repositories_table(
    localizer: &Arc<Localizer>,
    base: String,
    creates: RowCreates,
    rows: RwSignal<Option<Vec<RepositoryDto>>>,
    tested: RwSignal<HashMap<String, Vec<LocationTriedDto>>>,
) -> AnyView {
    // The five columns add up to the page's 58 rem at 1280 px, so a wide
    // screen never scrolls the frame (§4.5, the table).
    // Its header reads whole at 320 px with 200 % text too: "Name" in
    // DejaVu Sans Bold, Linux's default sans-serif, is 3.23em, wider than
    // two fifths of the 8rem frame less the cells' padding (94.4 px of 32 px
    // text), so it broke inside on CI. 4rem leaves it 120 px, room for the
    // word 16 % wider, and the cells beside it half the frame (§4.5).
    let name = Column::new(
        "name",
        text(localizer, msg::UI_COLUMN_NAME),
        Rem::tenths(70),
        |row: Signal<RepositoryDto>| row.with_untracked(|row| row.alias.clone()).into_any(),
    )
    .kept_whole(Rem::tenths(40));
    let maintenance_localizer = Arc::clone(localizer);
    let maintenance = Column::new(
        "maintenance",
        text(localizer, msg::UI_COLUMN_MAINTENANCE),
        // Its header on one line in DejaVu Sans Bold, Linux's default
        // sans-serif, the widest common one, with room to spare.
        Rem::tenths(90),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&maintenance_localizer);
            (move || row.with(|row| maintainer_view(&localizer, row))).into_any()
        },
    );
    let place_localizer = Arc::clone(localizer);
    let location = Column::new(
        "location",
        text(localizer, msg::UI_COLUMN_LOCATION),
        // The room the others leave: an address such as
        // `sftp:me@nas.home.lan:/srv/data/repo/pc/main` takes two lines,
        // broken only between its parts (`address_view`).
        Rem::tenths(175),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&place_localizer);
            (move || row.with(|row| place_view(&localizer, row, tested))).into_any()
        },
    );
    let password_localizer = Arc::clone(localizer);
    let password = Column::new(
        "password",
        text(localizer, msg::UI_COLUMN_PASSWORD),
        // A quarter of the table, so where a password comes from, and a
        // warning about it, read as sentences (#482).
        Rem::tenths(150),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&password_localizer);
            (move || row.with(|row| password_view(&localizer, row))).into_any()
        },
    );
    let actions_localizer = Arc::clone(localizer);
    let actions = Column::new(
        "actions",
        text(localizer, msg::UI_COLUMN_ACTIONS),
        // A row's Create button ("Create demo") on one line in DejaVu Sans.
        Rem::tenths(95),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&actions_localizer);
            let base = base.clone();
            let offers = Memo::new(move |_| {
                row.with(|row| {
                    (
                        row.alias.clone(),
                        lists_locations(&row.locations),
                        row.create_offered,
                    )
                })
            });
            (move || {
                let (alias, several, create_offered) = offers.get();
                let test = several.then(|| test_in_row(&localizer, &base, creates, &alias, tested));
                let create =
                    create_offered.then(|| create_in_row(&localizer, &base, creates, &alias));
                view! {
                    {test}
                    {create}
                }
            })
            .into_any()
        },
    );
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_NO_REPOSITORIES),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=TableName::Heading(PAGE_HEADING_ID.to_owned())
            columns_choice=ColumnsChoice::new("repositories", Arc::clone(localizer))
            row_header=name
            always_shown=vec![maintenance]
            columns=vec![location, password, actions]
            rows=Memo::new(move |_| rows.get().unwrap_or_default())
            key=|row: &RepositoryDto| row.alias.clone()
            state=State::Ready
            sentences=sentences
        />
    }
    .into_any()
}

/// Where a repository is: its address, its hot half on a line of its own, or
/// the list of its locations.
fn place_view(
    localizer: &Arc<Localizer>,
    row: &RepositoryDto,
    tested: RwSignal<HashMap<String, Vec<LocationTriedDto>>>,
) -> AnyView {
    if lists_locations(&row.locations) {
        return locations_list(localizer, &row.alias, row.locations.clone(), tested).into_any();
    }
    let hot = row.repo_hot.clone().map(|location| {
        text_with(
            localizer,
            msg::UI_REPOSITORY_HOT,
            &[("location", MsgArg::Text(location))],
        )
    });
    view! {
        <div>{address_view(&row.repository)}</div>
        {hot.map(|hot| view! { <div>{runs_view(&hot)}</div> })}
    }
    .into_any()
}

/// Who maintains a repository (§7.3), in words, never a color alone
/// (#1548).
fn maintainer_view(localizer: &Localizer, row: &RepositoryDto) -> AnyView {
    text(
        localizer,
        maintainer_note(row.maintenance, row.maintenance_paused_since.is_some()),
    )
    .into_any()
}

/// Where a repository's password comes from, and what the page notes of
/// it.
fn password_view(localizer: &Localizer, row: &RepositoryDto) -> AnyView {
    let (id, args) = password_source(&row.password);
    let password = text_with(localizer, id, &args);
    let notes = repository_notes(localizer, &row.warnings);
    view! {
        <div>{runs_view(&password)}</div>
        {notes
            .into_iter()
            .map(|note| view! { <div class="text-muted">{runs_view(&note)}</div> })
            .collect_view()}
    }
    .into_any()
}

/// A row's locations, in the order a job tries them: each headed by its
/// place, the first saying it is tried first, then its address, its hot
/// half, its options by name (a value only when it is references alone, I4),
/// and what the row's last Test found there.
fn locations_list(
    localizer: &Arc<Localizer>,
    alias: &str,
    locations: Vec<LocationDto>,
    tested: RwSignal<HashMap<String, Vec<LocationTriedDto>>>,
) -> impl IntoView + use<> {
    let (id, args) = locations_label(alias);
    let label = text_with(localizer, id, &args);
    let items = locations
        .into_iter()
        .map(|location| {
            let (id, args) = location_heading(location.position);
            let heading = text_with(localizer, id, &args);
            let hot = location.repo_hot.map(|hot| {
                text_with(
                    localizer,
                    msg::UI_REPOSITORY_HOT,
                    &[("location", MsgArg::Text(hot))],
                )
            });
            let options: Vec<String> = location
                .options
                .iter()
                .map(|option| {
                    let (id, args) = location_option(option);
                    text_with(localizer, id, &args)
                })
                .collect();
            let rule: Vec<String> = location_rule(&location.when)
                .into_iter()
                .map(|(id, args)| text_with(localizer, id, &args))
                .collect();
            let position = location.position;
            let localizer = Arc::clone(localizer);
            let alias = alias.to_owned();
            let found = move || {
                tested.with(|tested| {
                    found_at(tested.get(&alias).map_or(&[], Vec::as_slice), position).map(|tried| {
                        location_tested(tried)
                            .into_iter()
                            .map(|(id, args)| text_with(&localizer, id, &args))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
            };
            view! {
                <li>
                    <div>{heading}</div>
                    <div>{address_view(&location.repository)}</div>
                    {hot.map(|hot| view! { <div>{runs_view(&hot)}</div> })}
                    {options
                        .into_iter()
                        .map(|option| view! { <div class="text-muted">{runs_view(&option)}</div> })
                        .collect_view()}
                    {rule
                        .into_iter()
                        .map(|said| view! { <div>{runs_view(&said)}</div> })
                        .collect_view()}
                    {move || found().map(|found| view! { <div>{runs_view(&found)}</div> })}
                </li>
            }
        })
        .collect_view();
    view! { <ol aria-label=label>{items}</ol> }
}

/// A row's Test: the button, named after its row, that probes each of the
/// repository's locations and records nothing. While it works, and once it
/// answered, the page says so politely and focus stays on the button; what
/// each location came to is in the row's list. A refusal is the row's alert.
fn test_in_row(
    localizer: &Arc<Localizer>,
    base: &str,
    creates: RowCreates,
    alias: &str,
    tested: RwSignal<HashMap<String, Vec<LocationTriedDto>>>,
) -> impl IntoView + use<> {
    // It shows "Test" and is named for its row: the column beside the
    // addresses has room for one word, and the name begins with what the
    // button shows (WCAG 2.5.3) (#482).
    let (id, args) = test_locations_label(alias);
    let label = text_with(localizer, id, &args);
    let shown = text(localizer, msg::UI_REPOSITORY_TEST);
    let url = format!("{base}{}", test_locations_path(alias));
    let alias = alias.to_owned();
    let localizer = Arc::clone(localizer);
    let refused = RwSignal::<Option<String>>::new(None);
    let busy = RwSignal::new(false);
    let alert_localizer = Arc::clone(&localizer);
    let test = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        refused.set(None);
        tested.update(|tested| {
            tested.remove(&alias);
        });
        reply(
            Politeness::Polite,
            &text(&localizer, msg::UI_REPOSITORIES_WORKING),
        );
        let (url, alias, localizer) = (url.clone(), alias.clone(), Arc::clone(&localizer));
        spawn_local(async move {
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            // The reader may have followed a link while the probes ran: the
            // page went with it, and there is nobody left to tell.
            if creates.phase.try_get_untracked().is_none() {
                return;
            }
            // The list may have been drawn again meanwhile, and the row's own
            // signals went with the row.
            busy.try_set(false);
            match posted {
                Some(Posted::Done(answer)) => match locations_tested(&answer) {
                    Some(found) => {
                        let (id, args) = tested_announcement(&alias, &found);
                        reply(Politeness::Polite, &text_with(&localizer, id, &args));
                        tested.try_update(|tested| tested.insert(alias, found));
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => creates.phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    // The row's alert is no live region: the refusal, the
                    // press's answer, is said assertively too (§4.5).
                    let shown = refusal(&localizer, code, &args, detail.as_deref());
                    reply(Politeness::Assertive, &shown);
                    refused.try_set(Some(shown));
                }
                // No answer at all: the page's phase says that its own way.
                None => creates.phase.set(Phase::Unreachable),
            }
        });
    };
    view! {
        <Button
            content=Content::Text(shown.into())
            size=Size::Small
            fuller_name=Signal::stored(label)
            busy=busy
            on_press=Callback::new(test)
        />
        {move || {
            refused
                .get()
                .map(|text| {
                    view! {
                        <Alert localizer=Arc::clone(&alert_localizer) tone=AlertTone::Error>
                            {text}
                        </Alert>
                    }
                })
        }}
    }
}

/// A row's Create: the button, named after its row, that creates the saved
/// repository where its entry says. A created repository is said below the
/// list, which is read again, so the row's button goes and focus moves to
/// what was said ([`CreatedNote`]); a refusal is the row's alert, and the
/// row and its button stay.
fn create_in_row(
    localizer: &Arc<Localizer>,
    base: &str,
    creates: RowCreates,
    alias: &str,
) -> impl IntoView + use<> {
    let (id, args) = create_label(alias);
    let label = text_with(localizer, id, &args);
    let url = format!("{base}{}", create_saved_path(alias));
    let alias = alias.to_owned();
    let localizer = Arc::clone(localizer);
    // The last answer's refusal, as the row shows it, and whether the button
    // takes no press: from a press until the answer is a refusal, or, once the
    // repository is created, until the list is read again and the row goes.
    let refused = RwSignal::<Option<String>>::new(None);
    let busy = RwSignal::new(false);
    let alert_localizer = Arc::clone(&localizer);
    let create = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        creates.created.set(None);
        refused.set(None);
        reply(
            Politeness::Polite,
            &text(&localizer, msg::UI_REPOSITORIES_WORKING),
        );
        let (url, alias, localizer) = (url.clone(), alias.clone(), Arc::clone(&localizer));
        spawn_local(async move {
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            // The reader may have followed a link while the engine worked:
            // the page went with it, and there is nobody left to tell.
            if creates.phase.try_get_untracked().is_none() {
                return;
            }
            match posted {
                Some(Posted::Done(answer)) => match RowCreated::of(&answer) {
                    // The button stays busy: the list is read again, and
                    // the row and its button go with it.
                    Some(created) => {
                        let (id, args) = RowCreated::confirmation(&alias);
                        let confirmation = text_with(&localizer, id, &args);
                        let (id, args) = created.id_sentence();
                        let id_sentence = text_with(&localizer, id, &args);
                        creates.created.set(Some((confirmation, id_sentence)));
                        creates.load.run(());
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => creates.phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    // The list may have been drawn again while the engine
                    // worked, and the row's own signals went with the row.
                    busy.try_set(false);
                    let shown = match row_refusal(code, &alias) {
                        Some((id, args)) => text_with(&localizer, id, &args),
                        None => refusal(&localizer, code, &args, detail.as_deref()),
                    };
                    // The row's alert is no live region: the refusal, the
                    // press's answer, is said assertively too (§4.5).
                    reply(Politeness::Assertive, &shown);
                    refused.try_set(Some(shown));
                }
                // No answer at all: the page's phase says that its own way.
                None => creates.phase.set(Phase::Unreachable),
            }
        });
    };
    view! {
        <Button
            content=Content::Text(label.into())
            size=Size::Small
            busy=busy
            on_press=Callback::new(create)
        />
        {move || {
            refused
                .get()
                .map(|text| {
                    view! {
                        <Alert localizer=Arc::clone(&alert_localizer) tone=AlertTone::Error>
                            {text}
                        </Alert>
                    }
                })
        }}
    }
}

/// What a row's Create came to, below the list: focus moves to the short
/// confirmation as it is shown, for the row's button goes when the list is
/// read again, and a screen reader reads it there, so nothing is announced
/// (§4.5). The ID is a sentence of its own after it, outside what has focus,
/// so a screen reader doesn't spell out 64 hex digits unasked.
#[component]
fn CreatedNote(confirmation: String, id_sentence: String) -> impl IntoView {
    let note = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        if let Some(note) = note.get() {
            // A failed focus leaves focus where it was.
            let _ = note.focus();
        }
    });
    view! {
        <p tabindex="-1" node_ref=note>
            {confirmation}
        </p>
        <p>{id_sentence}</p>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_field_has_its_own_input_and_the_form_reads_them_all() {
        let inputs = Inputs::new();
        for (field, value) in [
            (Field::Name, "copy"),
            (Field::Location, "/srv/main"),
            (Field::HotStorage, "/srv/hot"),
            (Field::Password, "typed"),
        ] {
            assert_eq!(
                inputs.of(field).get_untracked(),
                "",
                "{field:?} starts empty"
            );
            inputs.of(field).set(value.to_owned());
        }
        assert!(
            !inputs.maintain_here.get_untracked(),
            "the checkbox starts off"
        );
        inputs.maintain_here.set(true);
        assert!(
            inputs.keep_password.get_untracked() == KEEP_IN_CONFIGURATION,
            "the configuration file is chosen first"
        );
        inputs.keep_password.set(KEEP_IN_KEYRING);
        // `Form` has no `Debug`, so that nothing can print a password: compare.
        assert!(
            inputs.read()
                == Form {
                    name: "copy".to_owned(),
                    location: "/srv/main".to_owned(),
                    hot_storage: "/srv/hot".to_owned(),
                    password: "typed".to_owned(),
                    maintain_here: true,
                    keep_in_keyring: true,
                }
        );
        for field in FIELDS {
            assert!(!inputs.of(field).get_untracked().is_empty(), "{field:?}");
        }
    }

    #[test]
    fn clearing_the_form_empties_every_input_and_turns_the_checkbox_off() {
        let inputs = Inputs::new();
        for field in FIELDS {
            inputs.of(field).set("typed".to_owned());
        }
        inputs.maintain_here.set(true);
        inputs.clear();
        assert!(inputs.read() == Form::default());
        assert!(!inputs.maintain_here.get_untracked());
    }

    #[test]
    fn clearing_the_form_chooses_the_configuration_file_again() {
        let inputs = Inputs::new();
        inputs.keep_password.set(KEEP_IN_KEYRING);
        inputs.clear();
        assert!(inputs.keep_password.get_untracked() == KEEP_IN_CONFIGURATION);
        assert!(!inputs.read().keep_in_keyring);
    }

    #[test]
    fn the_problems_a_form_shows_are_the_summary_and_each_field_s_own_error() {
        let problems = Problems::new();
        let items = vec![SummaryItem {
            field: Some(Field::Location.id()),
            sentence: Sentence::new("Enter where the repository is.", None),
        }];
        problems.show(items.clone(), Some("Try again.".to_owned()));
        assert_eq!(problems.summary.get_untracked(), items);
        assert_eq!(
            problems.advice.get_untracked().as_deref(),
            Some("Try again.")
        );
        assert_eq!(
            problems.of(Field::Location).get_untracked().as_deref(),
            Some("Enter where the repository is.")
        );
        for field in [Field::Name, Field::HotStorage, Field::Password] {
            assert_eq!(problems.of(field).get_untracked(), None, "{field:?}");
        }
        // No items clears the form again, and the advice with it.
        problems.show(Vec::new(), None);
        assert!(problems.summary.get_untracked().is_empty());
        assert_eq!(problems.advice.get_untracked(), None);
        for field in FIELDS {
            assert_eq!(problems.of(field).get_untracked(), None, "{field:?}");
        }
    }

    #[test]
    fn each_field_has_its_own_error_signal() {
        let problems = Problems::new();
        for field in FIELDS {
            problems.of(field).set(Some(format!("{field:?}")));
        }
        for field in FIELDS {
            assert_eq!(
                problems.of(field).get_untracked(),
                Some(format!("{field:?}")),
                "{field:?} shares a signal"
            );
        }
    }
}
