//! The routes of the app (technical design §4.5, "Route changes"): the pages a
//! path shows. Each page has its own `<h1>`, the heading focus moves to when a
//! reader navigates to it, so it carries `tabindex="-1"`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use api::{
    ErrorCode, LocationDto, LocationTriedDto, MsgArg, PlanSummaryDto, PlansDto, RepositoriesDto,
    RepositoryDto, RepositoryNotesDto,
};
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use std::str::FromStr;

use crate::announcer::Politeness;
use crate::backend::{OperationRow, OperationsState, ended};
use crate::components::alert::rules::AlertTone;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Content, Kind};
use crate::components::first_load;
use crate::components::link::rules::{Destination, Matching};
use crate::components::skeleton::rules::Awaited;
use crate::components::status_badge::rules::BadgeState;
use crate::components::{
    Alert, Banner, Button, Checkbox, Column, ErrorSummary, Link, Name as TableName, PauseUpdates,
    Progress, RadioGroup, Sentences, Sort, SortKey, State, StatusBadge, SummaryItem, Table,
    TextField, Toast, ToastTone, Toasts, address_view, cell_sentence_view, hand_focus_when_gone,
    refusal_alert, runs_view, say_refusals, text_field,
};
use crate::full_disk_access::describing_the_heading;
use crate::full_disk_access_warning::{FullDiskAccessFound, FullDiskAccessSteps};
use crate::live::{reply, say};
use crate::operations::{self, Name, Said};
use crate::pause::{self, Ends, Pausable};
use crate::repositories::{
    Action, Field, Form, Outcome, RowCreated, Sentence, advises_retry, create_label,
    create_saved_path, error_of, field_of, found_at, lists_locations, location_heading,
    location_option, location_rule, location_tested, locations_label, locations_tested,
    password_source, problems_of, repository_notes, rereads_list, row_refusal,
    test_locations_label, test_locations_path, tested_announcement,
};
use crate::session::SignInOffNotice;
use crate::sign_in::SignInOff;
use crate::variants::{Rem, Size, Tone};
use crate::visit::Visit;
use crate::ws::{self, Got, Phase, Posted};
use crate::{page, text};

/// The `id` of the notice, on every page, that sign-in is off on the network.
pub(crate) const SIGN_IN_OFF_NETWORK_ID: &str = "sign-in-off-network";

/// The `id` of every page's `<h1>`, which a table that lists what the page is
/// about takes its name from, rather than a caption that says the heading
/// again under it (#431).
pub(crate) const PAGE_HEADING_ID: &str = "page-heading";

/// The `<h1>` of a page, with `tabindex="-1"` so that focus can move to it.
/// When a reader navigates to the page it takes focus as it is shown
/// ([`page::focus_on_arrival`]), so the heading is there when focus moves.
#[component]
pub(crate) fn PageHeading(children: Children) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H1>::new();
    // With sign-in off on the network, and while the agent found Full Disk
    // Access missing, the notices above every page are read after the
    // heading, which takes focus when a reader navigates (§10.1, #948).
    let notice = use_context::<RwSignal<SignInOffNotice>>();
    let found = use_context::<FullDiskAccessFound>();
    let described_by = move || {
        let network = notice
            .is_some_and(|notice| notice.get().describes_the_heading())
            .then_some(SIGN_IN_OFF_NETWORK_ID);
        let found = found.and_then(|FullDiskAccessFound(found)| found.get());
        describing_the_heading(network, found.as_ref())
    };
    Effect::new(move |_| {
        if let Some(heading) = heading.get() {
            page::focus_on_arrival(&heading);
        }
    });
    view! {
        <h1 id=PAGE_HEADING_ID node_ref=heading tabindex="-1" aria-describedby=described_by>
            {children()}
        </h1>
    }
}

/// What the "Next run" cell of a plan says, and the instant it shows, as the
/// daemon sent it (UTC), when it shows one. A plan without a schedule runs
/// only when it is started; a scheduled one the scheduler has planned no run
/// for says so, never a blank cell.
pub(crate) fn next_run_cell(
    localizer: &Localizer,
    plan: &PlanSummaryDto,
    zone: Option<&api::IanaZone>,
) -> (String, Option<String>) {
    if !plan.scheduled {
        return (text(localizer, msg::UI_START_ON_DEMAND), None);
    }
    match &plan.next_run {
        Some(raw) => (instant(localizer, raw, zone), Some(raw.clone())),
        None => (text(localizer, msg::UI_START_NOT_PLANNED), None),
    }
}

/// A cell's instant, such as a plan's next run from [`next_run_cell`]: its
/// words in a `<time>` that gives the instant, `raw`, to a program, so the
/// cell's words are the instant and nothing hides it in a `title` (#851);
/// words alone where there is no instant.
pub(crate) fn time_view(shown: String, raw: Option<String>) -> AnyView {
    match raw {
        Some(raw) => view! { <time datetime=raw>{shown}</time> }.into_any(),
        None => shown.into_any(),
    }
}

/// The body a cancel request carries: the JSON-body rule applies to every
/// mutating method, so even an empty one is `{}`.
#[derive(Serialize)]
pub(crate) struct Empty {}

/// The text of message `id`, with `args` for its variables.
pub(crate) fn text_with(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    localizer.message(id, args).into_text()
}

/// An RFC 3339 instant as the page's locale writes it (§8.4). One that
/// doesn't parse is shown as it came: never a blank cell, never a guess.
pub(crate) fn instant(localizer: &Localizer, raw: &str, zone: Option<&api::IanaZone>) -> String {
    match jiff::Timestamp::from_str(raw) {
        Ok(timestamp) => text_with(
            localizer,
            msg::UI_INSTANT,
            &[(
                "when",
                MsgArg::Timestamp {
                    utc_ms: timestamp.as_millisecond(),
                    zone: zone.cloned(),
                },
            )],
        ),
        Err(_) => raw.to_owned(),
    }
}

/// When a listed snapshot was taken, as a page shows it (§16, decision 56):
/// what the clock that took it read, on the day a calendar retention rule
/// counts it on, rather than the viewer's reading of the instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// A time that isn't RFC 3339, shown as it came.
    Unread,
    /// A time without the offset of its clock, from a daemon older than the
    /// field: the instant, read in the viewer's zone as before.
    Instant(jiff::Timestamp),
    /// What the snapshot's own clock read, at that clock's `offset`. The
    /// offset is `named` when it differs from the viewer's at that instant,
    /// so that someone in the zone the snapshot was taken in reads the time
    /// as their own clock does.
    Clock {
        reading: jiff::civil::DateTime,
        offset: jiff::tz::Offset,
        named: bool,
    },
}

/// When the snapshot whose time is `raw` (RFC 3339) was taken, by a clock at
/// `offset`, for a viewer whose own clock is at `viewer(instant)` at that
/// instant (the browser's, [`crate::snapshots::viewer_offset`]).
pub(crate) fn taken(
    raw: &str,
    offset: Option<api::ClockOffset>,
    viewer: impl FnOnce(jiff::Timestamp) -> jiff::tz::Offset,
) -> Taken {
    let Ok(at) = jiff::Timestamp::from_str(raw) else {
        return Taken::Unread;
    };
    let Some(offset) = offset.map(api::ClockOffset::get) else {
        return Taken::Instant(at);
    };
    Taken::Clock {
        reading: offset.to_datetime(at),
        offset,
        named: viewer(at) != offset,
    }
}

/// A listed snapshot's time as the page writes it ([`taken`]). The host
/// writes the clock's reading as the locale does, with the offset after it
/// when it is named. A build whose host doesn't write instants
/// (`writes_instants` false) would write that reading as an instant in UTC
/// and call it so; the reading is written plainly instead, and its offset
/// always follows it, so nothing labels the reading of another clock UTC.
/// A time without its clock's offset is the instant in the viewer's `zone`,
/// as [`instant`] writes it.
pub(crate) fn snapshot_time(
    localizer: &Localizer,
    raw: &str,
    offset: Option<api::ClockOffset>,
    zone: Option<&api::IanaZone>,
    viewer: impl FnOnce(jiff::Timestamp) -> jiff::tz::Offset,
    writes_instants: bool,
) -> String {
    let (reading, offset, named) = match taken(raw, offset, viewer) {
        Taken::Unread => return raw.to_owned(),
        Taken::Instant(_) => return instant(localizer, raw, zone),
        Taken::Clock {
            reading,
            offset,
            named,
        } => (reading, offset, named),
    };
    // The host writes the reading as it writes an instant read in UTC, which
    // shows no zone. A reading past the last instant is no instant to hand it.
    let hosted = jiff::tz::TimeZone::UTC
        .to_timestamp(reading)
        .ok()
        .filter(|_| writes_instants);
    let (when, named) = match hosted {
        Some(as_if_utc) => (
            text_with(
                localizer,
                msg::UI_INSTANT,
                &[(
                    "when",
                    MsgArg::Timestamp {
                        utc_ms: as_if_utc.as_millisecond(),
                        zone: None,
                    },
                )],
            ),
            named,
        ),
        None => (reading.strftime("%Y-%m-%d %H:%M:%S").to_string(), true),
    };
    if !named {
        return when;
    }
    text_with(
        localizer,
        msg::UI_SNAPSHOT_TIME_OFFSET,
        &[
            ("when", MsgArg::Text(when)),
            (
                "offset",
                MsgArg::Text(api::ClockOffset::new(offset).to_string()),
            ),
        ],
    )
}

/// A refusal the daemon answered with, as the catalog writes it (§8.4): a
/// typed code with its arguments, never a bare status number.
pub(crate) fn refusal(
    localizer: &Localizer,
    code: ErrorCode,
    args: &[(String, MsgArg)],
    detail: Option<&str>,
) -> String {
    let args = args
        .iter()
        .map(|(name, arg)| (name.as_str(), arg.clone()))
        .collect::<Vec<_>>();
    localizer.error(code, &args, detail).into_text()
}

/// A refusal as [`refusal`] writes it, with the sender's own words marked:
/// they are English whatever the page's language (§8.4).
pub(crate) fn refused_sentence(
    localizer: &Localizer,
    code: ErrorCode,
    args: &[(String, MsgArg)],
    detail: Option<&str>,
) -> Sentence {
    Sentence::new(&refusal(localizer, code, args, detail), detail)
}

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
            (Phase::Loading, _, _) => first_load(loading.clone(), Awaited::table(4)).into_any(),
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
                <p class="actions">
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
/// repository's name as its row's header, where it is (the hot half on a line
/// of its own), or, for a repository reached at several locations, the list
/// of them in order with its Test (decision 80), where its password comes
/// from, and its Create, when the row offers it (decision 53). Never the
/// password itself (I4). A row follows a read of the list in place; its
/// buttons are built again only when what the row offers changes, as a
/// created repository's Create goes.
fn repositories_table(
    localizer: &Arc<Localizer>,
    base: String,
    creates: RowCreates,
    rows: RwSignal<Option<Vec<RepositoryDto>>>,
    tested: RwSignal<HashMap<String, Vec<LocationTriedDto>>>,
) -> AnyView {
    let name = Column::new(
        text(localizer, msg::UI_COLUMN_NAME),
        Rem::tenths(70),
        |row: Signal<RepositoryDto>| row.with_untracked(|row| row.alias.clone()).into_any(),
    );
    let place_localizer = Arc::clone(localizer);
    let location = Column::new(
        text(localizer, msg::UI_COLUMN_LOCATION),
        // An address such as `sftp:me@nas.home.lan:/srv/data/repo/pc/main`
        // on one line in DejaVu Sans, Linux's default sans-serif, the widest
        // common one. The four columns add up to the page's 58 rem.
        Rem::tenths(265),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&place_localizer);
            (move || row.with(|row| place_view(&localizer, row, tested))).into_any()
        },
    );
    let password_localizer = Arc::clone(localizer);
    let password = Column::new(
        text(localizer, msg::UI_COLUMN_PASSWORD),
        Rem::tenths(150),
        move |row: Signal<RepositoryDto>| {
            let localizer = Arc::clone(&password_localizer);
            (move || row.with(|row| password_view(&localizer, row))).into_any()
        },
    );
    let actions_localizer = Arc::clone(localizer);
    let actions = Column::new(
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
            row_header=name
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

/// Where a repository's password comes from, and what the page notes of it.
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

/// A snapshot's short ID: its first eight hex digits, as rustic's own lists
/// show one. A shorter ID is shown whole.
pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// What any path nothing serves shows.
#[component]
pub(crate) fn NotFound(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_NOT_FOUND_HEADING);
    let to = Destination::route("/", Matching::Exact);
    let body = text(&localizer, msg::UI_NOT_FOUND_BODY);
    let words = text(&localizer, msg::UI_NOT_FOUND_LINK);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{body}</p>
        <p>
            {to
                .map(|to| {
                    view! {
                        <Link to=to localizer=localizer>
                            {words}
                        </Link>
                    }
                })}
        </p>
    }
}

/// The title of the page at `path`, for the browser's tab and history: the
/// page's own, in the page's language (technical design §4.5, "Route
/// changes"). A path with no page is the not-found page, and so is
/// `/_gallery` in a build without the gallery.
pub(crate) fn title(localizer: &Localizer, path: &str) -> String {
    title_in(localizer, path, cfg!(feature = "gallery"))
}

/// Whether `path` is a snapshot's page: `/snapshots/{alias}/{snapshot}`,
/// neither name empty.
fn is_snapshot_page(path: &str) -> bool {
    path.strip_prefix("/snapshots/").is_some_and(|rest| {
        let mut names = rest.split('/');
        matches!(
            (names.next(), names.next(), names.next()),
            (Some(alias), Some(snapshot), None) if !alias.is_empty() && !snapshot.is_empty()
        )
    })
}

/// [`title`] for a build that has the gallery or not, so that both are tested
/// whichever one the tests are built as.
fn title_in(localizer: &Localizer, path: &str, gallery: bool) -> String {
    let page = if path == "/" {
        msg::UI_START_TITLE
    } else if gallery && path.trim_suffix('/') == "/_gallery" {
        msg::UI_GALLERY_TITLE
    } else if path == "/operations" {
        msg::UI_OPERATIONS_TITLE
    } else if path == "/plans" {
        msg::UI_PLANS_TITLE
    } else if path == "/proposals" {
        msg::UI_PROPOSALS_TITLE
    } else if path == "/repositories" {
        msg::UI_REPOSITORIES_TITLE
    } else if path == "/snapshots" {
        msg::UI_SNAPSHOTS_TITLE
    } else if path == "/log" {
        msg::UI_LOG_TITLE
    } else if path == "/settings" {
        msg::UI_SETTINGS_TITLE
    } else if is_snapshot_page(path) {
        msg::UI_SNAPSHOT_TITLE
    } else {
        msg::UI_NOT_FOUND_TITLE
    };
    text(localizer, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_of(path: &str) -> String {
        title(&Localizer::new(&[]), path)
    }

    #[test]
    fn the_start_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/"), "Ladderback");
    }

    #[test]
    fn the_operations_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/operations"), "Operations – Ladderback");
    }

    #[test]
    fn the_repositories_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/repositories"), "Repositories – Ladderback");
    }

    #[test]
    fn the_snapshots_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/snapshots"), "Snapshots – Ladderback");
    }

    #[test]
    fn the_settings_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/settings"), "Settings – Ladderback");
    }

    #[test]
    fn the_log_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/log"), "Log – Ladderback");
    }

    #[test]
    fn the_plans_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/plans"), "Plans – Ladderback");
    }

    #[test]
    fn the_pending_changes_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/proposals"), "Pending changes – Ladderback");
    }

    #[test]
    fn a_path_with_no_page_is_titled_page_not_found() {
        assert_eq!(title_of("/no/such/page"), "Page not found – Ladderback");
    }

    #[test]
    fn the_gallery_has_its_title_only_in_a_build_that_has_it() {
        let with = title_in(&Localizer::new(&[]), "/_gallery", true);
        let without = title_in(&Localizer::new(&[]), "/_gallery", false);
        assert_eq!(with, "Component gallery – Ladderback");
        assert_eq!(without, "Page not found – Ladderback");
    }

    #[test]
    fn the_build_decides_whether_the_gallery_is_a_page() {
        let expected = if cfg!(feature = "gallery") {
            "Component gallery – Ladderback"
        } else {
            "Page not found – Ladderback"
        };
        assert_eq!(title_of("/_gallery"), expected);
    }

    #[test]
    fn a_trailing_slash_names_the_same_page() {
        let localizer = Localizer::new(&[]);
        assert_eq!(
            title_in(&localizer, "/_gallery/", true),
            "Component gallery – Ladderback"
        );
        assert_eq!(title_of("/no/such/page/"), "Page not found – Ladderback");
    }

    #[test]
    fn a_path_that_only_starts_like_a_page_is_not_that_page() {
        assert_eq!(title_of("/_gallery-2"), "Page not found – Ladderback");
        assert_eq!(title_of("//"), "Page not found – Ladderback");
    }

    #[test]
    fn a_snapshot_s_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/snapshots/demo/ab12"), "Snapshot – Ladderback");
        assert_eq!(
            title_of("/snapshots/my%20repo/ab12"),
            "Snapshot – Ladderback"
        );
    }

    #[test]
    fn a_path_under_snapshots_that_names_no_snapshot_is_not_its_page() {
        for path in [
            "/snapshots/demo",
            "/snapshots/demo/",
            "/snapshots//ab12",
            "/snapshots/demo/ab12/more",
            "/snapshotsx/demo/ab12",
        ] {
            assert_eq!(title_of(path), "Page not found – Ladderback", "{path}");
        }
    }

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

    #[test]
    fn an_instant_is_written_in_the_pages_locale_and_one_that_does_not_parse_stands_as_it_came() {
        let localizer = Localizer::new(&[]);
        assert_ne!(
            instant(&localizer, "2026-11-09T10:00:02Z", None),
            "2026-11-09T10:00:02Z",
            "a parsed instant is the locale's own writing, not the wire's"
        );
        assert_eq!(
            instant(&localizer, "not-an-instant", None),
            "not-an-instant",
            "never a blank cell, never a guess"
        );
    }

    fn plan(scheduled: bool, next_run: Option<&str>) -> PlanSummaryDto {
        PlanSummaryDto {
            id: "demo-home".to_owned(),
            repository: "demo".to_owned(),
            scheduled,
            next_run: next_run.map(str::to_owned),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn a_planned_run_is_written_in_the_pages_locale_and_carries_its_instant() {
        let localizer = Localizer::new(&[]);
        let raw = "2026-11-13T03:00:00Z";
        assert_eq!(
            next_run_cell(&localizer, &plan(true, Some(raw)), None),
            (instant(&localizer, raw, None), Some(raw.to_owned()))
        );
    }

    #[test]
    fn a_scheduled_plan_without_a_planned_run_says_so() {
        assert_eq!(
            next_run_cell(&Localizer::new(&[]), &plan(true, None), None),
            ("Not planned yet".to_owned(), None)
        );
    }

    #[test]
    fn a_plan_without_a_schedule_runs_when_started() {
        let localizer = Localizer::new(&[]);
        for next_run in [None, Some("2026-11-13T03:00:00Z")] {
            assert_eq!(
                next_run_cell(&localizer, &plan(false, next_run), None),
                ("When started from Operations".to_owned(), None)
            );
        }
    }

    #[test]
    fn a_refusal_is_written_through_the_catalog_with_its_argument() {
        let shown = refusal(
            &Localizer::new(&[]),
            ErrorCode::PlanUnknown,
            &[("plan".to_owned(), MsgArg::Text("demo-home".to_owned()))],
            None,
        );
        assert!(!shown.is_empty(), "a refusal is never a bare absence");
        assert!(
            shown.contains("demo-home"),
            "the argument is part of the message: {shown}"
        );
    }

    #[test]
    fn a_refusal_carries_the_engines_words_when_the_daemon_sent_them() {
        let localizer = Localizer::new(&[]);
        let with = refusal(
            &localizer,
            ErrorCode::OperationFailed,
            &[],
            Some("wrong password"),
        );
        assert!(
            with.starts_with("The operation failed: ") && with.contains("wrong password"),
            "{with}"
        );
        let without = refusal(&localizer, ErrorCode::OperationFailed, &[], None);
        assert_eq!(without, "The operation failed.");
    }

    #[test]
    fn a_mistake_in_the_configuration_is_said_with_its_line_and_its_setting_marked_as_sent() {
        let sentence = refused_sentence(
            &Localizer::new(&[]),
            ErrorCode::ConfigInvalid,
            &[
                ("file".to_owned(), MsgArg::Text("/c/config.toml".to_owned())),
                ("line".to_owned(), MsgArg::Text("6".to_owned())),
            ],
            Some("repositories.demo.colour"),
        );
        let said = sentence.plain().replace(['\u{2068}', '\u{2069}'], "");
        assert!(said.contains("/c/config.toml"), "{said}");
        assert!(said.contains("line 6: repositories.demo.colour"), "{said}");
        let (detail, _) = sentence.detail.expect("the setting is marked as sent");
        assert_eq!(detail, "repositories.demo.colour");
    }

    #[test]
    fn a_snapshot_id_shows_its_first_eight_digits_and_a_short_one_whole() {
        assert_eq!(short_id("0123456789abcdef"), "01234567");
        assert_eq!(short_id("abc123"), "abc123");
    }
    #[test]
    fn a_message_with_its_arguments_is_the_catalogs_own_text() {
        assert_eq!(
            text_with(
                &Localizer::new(&[]),
                msg::UI_SNAPSHOTS_OF,
                &[("alias", MsgArg::Text("demo".to_owned()))],
            )
            .replace(['\u{2066}', '\u{2069}'], ""),
            "Snapshots of demo",
            "isolation marks aside, the catalog's text and nothing else"
        );
    }

    fn clock(seconds: i32) -> Option<api::ClockOffset> {
        Some(api::ClockOffset::new(
            jiff::tz::Offset::from_seconds(seconds).unwrap(),
        ))
    }

    fn zone(name: &str) -> Option<api::IanaZone> {
        Some(api::IanaZone::new(name).unwrap())
    }

    /// The offset of a viewer's clock in the zone `name` at each instant, as
    /// the browser gives it.
    fn in_zone(name: &str) -> impl Fn(jiff::Timestamp) -> jiff::tz::Offset {
        let zone = jiff::tz::TimeZone::get(name).unwrap();
        move |at| zone.to_offset(at)
    }

    /// What the clock at `offset` read when it took the snapshot at `raw`,
    /// and whether the viewer in `viewer` is told the offset.
    fn read(raw: &str, offset: i32, viewer: &str) -> (String, bool) {
        match taken(raw, clock(offset), in_zone(viewer)) {
            Taken::Clock {
                reading,
                offset: read_at,
                named,
            } => {
                assert_eq!(read_at.seconds(), offset, "{raw}");
                (reading.to_string(), named)
            }
            other => panic!("{raw} at {offset} is a clock's reading: {other:?}"),
        }
    }

    /// Decision 56's examples: a container whose clock runs in UTC, a late
    /// evening in Chicago, and a laptop flown from Auckland to Hawaii.
    #[test]
    fn a_snapshot_is_shown_as_its_own_clock_read_it_with_an_offset_the_viewer_lacks() {
        let chicago = -6 * 3600;
        assert_eq!(
            read("2026-11-11T05:30:00Z", 0, "America/Chicago"),
            ("2026-11-11T05:30:00".to_owned(), true),
            "a container's clock in UTC, seen from Chicago"
        );
        assert_eq!(
            read("2026-11-11T05:30:00Z", chicago, "America/Chicago"),
            ("2026-11-10T23:30:00".to_owned(), false),
            "late evening in Chicago, seen from Chicago"
        );
        assert_eq!(
            read("2026-11-11T05:30:00Z", chicago, "America/New_York"),
            ("2026-11-10T23:30:00".to_owned(), true),
            "late evening in Chicago, seen from New York the next day"
        );
        assert_eq!(
            read("2026-11-09T11:30:00Z", 13 * 3600, "Pacific/Honolulu"),
            ("2026-11-10T00:30:00".to_owned(), true),
            "the laptop in Auckland, seen from Hawaii"
        );
        assert_eq!(
            read("2026-11-11T09:30:00Z", -10 * 3600, "Pacific/Honolulu"),
            ("2026-11-10T23:30:00".to_owned(), false),
            "the laptop in Hawaii, seen from Hawaii"
        );
    }

    /// The offset compared is the viewer's at the moment the snapshot was
    /// taken: the night daylight saving time ends in Chicago, both sides of
    /// it are the viewer's own, and a clock that kept the summer's offset
    /// after it is not.
    #[test]
    fn the_viewers_offset_is_the_one_in_force_when_the_snapshot_was_taken() {
        assert_eq!(
            read("2026-11-01T05:30:00Z", -5 * 3600, "America/Chicago"),
            ("2026-11-01T00:30:00".to_owned(), false)
        );
        assert_eq!(
            read("2026-11-02T05:30:00Z", -6 * 3600, "America/Chicago"),
            ("2026-11-01T23:30:00".to_owned(), false)
        );
        assert_eq!(
            read("2026-11-02T04:30:00Z", -5 * 3600, "America/Chicago"),
            ("2026-11-01T23:30:00".to_owned(), true)
        );
        assert_eq!(
            read("2026-07-01T05:30:00Z", -6 * 3600, "America/Chicago"),
            ("2026-06-30T23:30:00".to_owned(), true)
        );
    }

    /// The viewer's offset is asked for at the instant the snapshot was
    /// taken, and only it decides: a viewer at a fixed offset (a browser in
    /// UTC, or one without zone names) is told every other offset.
    #[test]
    fn the_viewers_offset_is_read_at_the_snapshots_instant() {
        let raw = "2026-11-11T05:30:00Z";
        let named = |offset: i32, viewer: i32| {
            let viewer = jiff::tz::Offset::from_seconds(viewer).unwrap();
            match taken(raw, clock(offset), |at| {
                assert_eq!(at, raw.parse().unwrap());
                viewer
            }) {
                Taken::Clock { named, .. } => named,
                other => panic!("a clock's reading: {other:?}"),
            }
        };
        assert!(!named(0, 0));
        assert!(named(-6 * 3600, 0));
        assert!(named(0, 5 * 3600 + 45 * 60));
        assert!(!named(5 * 3600 + 45 * 60, 5 * 3600 + 45 * 60));
    }

    #[test]
    fn a_time_without_its_clocks_offset_is_the_instant_in_the_viewers_zone_as_before() {
        let localizer = Localizer::new(&[]);
        let raw = "2026-11-11T05:30:00Z";
        let viewer = zone("America/Chicago");
        assert_eq!(
            taken(raw, None, in_zone("America/Chicago")),
            Taken::Instant(raw.parse().unwrap())
        );
        for host in [true, false] {
            assert_eq!(
                snapshot_time(
                    &localizer,
                    raw,
                    None,
                    viewer.as_ref(),
                    in_zone("America/Chicago"),
                    host
                ),
                instant(&localizer, raw, viewer.as_ref())
            );
        }
    }

    #[test]
    fn a_time_that_is_not_rfc_3339_is_shown_as_it_came() {
        let localizer = Localizer::new(&[]);
        let utc = |_| jiff::tz::Offset::UTC;
        assert_eq!(taken("yesterday", clock(0), utc), Taken::Unread);
        assert_eq!(taken("yesterday", None, utc), Taken::Unread);
        for host in [true, false] {
            assert_eq!(
                snapshot_time(&localizer, "yesterday", clock(0), None, utc, host),
                "yesterday"
            );
        }
    }

    /// The host writes the reading as it writes an instant in UTC, which
    /// shows no zone, and the offset follows only when it is named.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_host_writes_the_reading_and_the_offset_only_when_the_viewer_lacks_it() {
        let localizer = Localizer::new(&[]);
        if !localizer.writes_instants() {
            testkit::skip!("this build's l10n has no host that writes instants");
        }
        let raw = "2026-11-11T05:30:00Z";
        let reading = instant(&localizer, "2026-11-10T23:30:00Z", None);
        let shown = |offset: i32, viewer: &str| {
            snapshot_time(
                &localizer,
                raw,
                clock(offset),
                zone(viewer).as_ref(),
                in_zone(viewer),
                localizer.writes_instants(),
            )
        };
        assert_eq!(
            shown(-6 * 3600, "America/New_York"),
            format!("{reading} UTC-06:00")
        );
        assert_eq!(shown(-6 * 3600, "America/Chicago"), reading);
        assert_eq!(
            shown(0, "America/Chicago"),
            format!("{} UTC+00:00", instant(&localizer, raw, None))
        );
    }

    /// Without a host, `DATETIME` writes an instant in UTC and says "UTC":
    /// the reading of another clock is written plainly instead, and its
    /// offset always follows, even where the viewer's is the same.
    #[test]
    fn without_a_host_the_reading_is_written_plainly_and_never_called_utc() {
        let localizer = Localizer::new(&[]);
        let raw = "2026-11-11T05:30:00Z";
        let shown = |offset: i32, viewer: &str| {
            snapshot_time(
                &localizer,
                raw,
                clock(offset),
                zone(viewer).as_ref(),
                in_zone(viewer),
                false,
            )
        };
        for viewer in ["America/Chicago", "America/New_York"] {
            assert_eq!(
                shown(-6 * 3600, viewer),
                "2026-11-10 23:30:00 UTC-06:00",
                "{viewer}"
            );
        }
        assert_eq!(shown(0, "UTC"), "2026-11-11 05:30:00 UTC+00:00");
        assert_eq!(
            snapshot_time(
                &localizer,
                raw,
                clock(19 * 60 + 32),
                None,
                |_| jiff::tz::Offset::UTC,
                false
            ),
            "2026-11-11 05:49:32 UTC+00:19:32"
        );
    }

    /// A reading past the last instant (late on 30 December 9999, UTC) can't
    /// be handed to the host as one; it is written plainly, as it read.
    #[test]
    fn a_reading_past_the_last_instant_is_written_plainly() {
        let localizer = Localizer::new(&[]);
        let raw = "9999-12-30T22:00:00Z";
        let latest = 25 * 3600 + 59 * 60 + 59;
        assert_eq!(
            snapshot_time(
                &localizer,
                raw,
                clock(latest),
                None,
                |_| jiff::tz::Offset::UTC,
                true
            ),
            "9999-12-31 23:59:59 UTC+25:59:59"
        );
    }
}
