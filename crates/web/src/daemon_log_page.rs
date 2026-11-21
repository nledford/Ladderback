//! The Log page's DOM glue (technical design §4.4, #618): it reads the recent
//! log once as the page opens, holds the choice of levels, lists the lines
//! the choice keeps and copies them for a bug report, answering in a toast
//! whether they are on the clipboard. `daemon_log.rs` decides for it.

use std::sync::Arc;

use api::{LogLineDto, RecentLogDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::ACTIONS;
use crate::components::button::Content;
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Button, Column, Name, RadioGroup, Sentences, State, Table, Toast, ToastTone, Toasts,
    refusal_alert, say_refusals, write_clipboard,
};
use crate::components::{first_load, paced};
use crate::daemon_log::{LOG_PATH, Levels, bug_report, level_name, shown};
use crate::repositories::Sentence;
use crate::routes::{PAGE_HEADING_ID, PageHeading, refused_sentence};
use crate::session::Told;
use crate::text;
use crate::variants::Rem;
use crate::ws::{self, Got, Phase};

/// The id of the button that copies, which a dismissed toast hands focus
/// to.
const COPY_ID: &str = "log-copy";

/// The Log page at `/log`: the recent log, newest line first, with the
/// choice of levels and the button that copies the lines listed. A read the
/// daemon refuses is an Error alert in the lines' place, as the daemon said
/// it, and said assertively; one that reaches no daemon is said as on every
/// page.
#[component]
pub(crate) fn DaemonLog(localizer: Arc<Localizer>) -> impl IntoView {
    let phase = RwSignal::new(Phase::Loading);
    let lines = RwSignal::new(Vec::<LogLineDto>::new());
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let url = format!("{}{LOG_PATH}", ws::origin());
    let load_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let read = ws::get::<RecentLogDto>(&url).await;
        // The reader may have left the page before the answer came.
        if phase.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                lines.set(read.lines);
                phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                refused.set(Some(refused_sentence(
                    &load_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                phase.set(Phase::Ready);
            }
            None => phase.set(Phase::Unreachable),
        }
    });

    let heading = text(&localizer, msg::UI_LOG_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_LOG_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let none = text(&localizer, msg::UI_LOG_NONE);
    let lines_localizer = Arc::clone(&localizer);
    let (phase, pace) = paced(move || phase.get(), |phase| *phase == Phase::Loading);
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
        {move || match phase.get() {
            Phase::Loading => first_load(loading.clone(), Awaited::table(4), pace).into_any(),
            Phase::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
            Phase::Ready => {
                match refused.get() {
                    Some(sentence) => refusal_alert(&lines_localizer, sentence).into_any(),
                    None if lines.with(Vec::is_empty) => view! { <p>{none.clone()}</p> }.into_any(),
                    None => {
                        view! { <Lines localizer=Arc::clone(&lines_localizer) lines=lines /> }
                            .into_any()
                    }
                }
            }
        }}
    }
}

/// The choice of levels, the button that copies, the toast that answers it,
/// and the table of the lines the choice keeps. Only the table follows the
/// choice, so focus stays on the radio button the reader moved to.
#[component]
fn Lines(localizer: Arc<Localizer>, lines: RwSignal<Vec<LogLineDto>>) -> impl IntoView {
    let chosen = RwSignal::new(Levels::All.value());
    let listed = Memo::new(move |_| lines.with(|lines| shown(lines, Levels::of(chosen.get()))));
    // The build the daemon told the page it runs (decision 90), for the
    // report's first line.
    let told = use_context::<RwSignal<Option<Told>>>();
    let toasts = Toasts::new();
    let copied = text(&localizer, msg::UI_LOG_COPIED);
    let refused = text(&localizer, msg::UI_LOG_COPY_FAILED);
    let copy = move |_| {
        let build = told
            .and_then(|told| told.get_untracked())
            .map(|told| told.build);
        let report = listed.with_untracked(|listed| bug_report(build.as_ref(), listed));
        let (copied, refused) = (copied.clone(), refused.clone());
        // Nothing else on the page shows whether the lines were copied, so a
        // toast does, said once. A refusal is an Error toast, which stays
        // beside the button until the reader dismisses it (§4.5).
        write_clipboard(&report, move |done| {
            if done {
                toasts.show(ToastTone::Success, copied.clone());
            } else {
                toasts.show(ToastTone::Error, refused.clone());
            }
        });
    };
    let options = Levels::ALL
        .into_iter()
        .map(|levels| (levels.value(), text(&localizer, levels.label())))
        .collect();
    let sentences = Sentences {
        loading: text(&localizer, msg::UI_LOADING),
        empty: text(&localizer, msg::UI_LOG_NONE_SHOWN),
        unreachable: text(&localizer, msg::UI_UNREACHABLE_BODY),
    };
    let [at, level, source, message] = [
        msg::UI_LOG_AT,
        msg::UI_LOG_LEVEL,
        msg::UI_LOG_SOURCE,
        msg::UI_LOG_MESSAGE,
    ]
    .map(|id| text(&localizer, id));
    // A line is its place in the list: two lines of the log can say the
    // same thing at the same instant.
    let row_header = Column::new(at, Rem::tenths(140), |row: Signal<(usize, LogLineDto)>| {
        let at = move || row.with(|(_, line)| line.at.clone());
        view! { <time datetime=at>{at}</time> }.into_any()
    });
    let level_localizer = Arc::clone(&localizer);
    let columns = vec![
        Column::new(
            level,
            Rem::tenths(70),
            move |row: Signal<(usize, LogLineDto)>| {
                let localizer = Arc::clone(&level_localizer);
                (move || text(&localizer, level_name(row.with(|(_, line)| line.level)))).into_any()
            },
        ),
        Column::new(
            source,
            Rem::tenths(120),
            |row: Signal<(usize, LogLineDto)>| {
                view! { <span lang="en">{move || row.with(|(_, line)| line.source.clone())}</span> }
                    .into_any()
            },
        ),
        // A message keeps the line breaks the log wrote.
        Column::new(
            message,
            Rem::tenths(240),
            |row: Signal<(usize, LogLineDto)>| {
                view! {
                    <span class="whitespace-pre-wrap" lang="en">
                        {move || row.with(|(_, line)| line.message.clone())}
                    </span>
                }
                .into_any()
            },
        ),
    ];
    let rows = Signal::derive(move || listed.get().into_iter().enumerate().collect::<Vec<_>>());
    view! {
        <RadioGroup
            name="log-levels"
            legend=text(&localizer, msg::UI_LOG_LEVELS)
            options=options
            chosen=chosen
        />
        <p class=ACTIONS>
            <Button
                content=Content::Text(text(&localizer, msg::UI_LOG_COPY).into())
                id=COPY_ID
                on_press=Callback::new(copy)
            />
        </p>
        <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=COPY_ID />
        <Table
            name=Name::Heading(PAGE_HEADING_ID.to_owned())
            row_header=row_header
            columns=columns
            rows=rows
            key=|row: &(usize, LogLineDto)| row.0
            state=State::Ready
            sentences=sentences
        />
    }
}
