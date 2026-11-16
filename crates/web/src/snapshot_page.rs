//! A snapshot's page's DOM glue (technical design §4.4, §10.4): the route
//! `/snapshots/{alias}/{snapshot}`, the snapshot's folders and files in a
//! tree, each folder read as it is opened, and beside it the pane that
//! restores the entry the tree selects. It decides nothing: the address of a
//! page is `tree::folder_url`, and the tree's keys, rows and answers are
//! `tree.rs`'s; what a restore sends, when it can be confirmed and what it
//! says are `restore.rs`'s, host-tested. What stays here is asking the
//! daemon and drawing the page; the journeys of
//! `features/ui/snapshot_browser.feature` and `features/restore/restore.feature`
//! run it in a browser, the `ws.rs` precedent.

use std::rc::Rc;
use std::sync::Arc;

use api::{
    FolderDto, MsgArg, RestoreDestinationDto, RestorePreviewDto, RestorePreviewRequestDto,
    RestoreStartedDto,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::announcer::Politeness;
use crate::components::{
    Confirmation, FolderLoader, FolderPage, RadioGroup, ReviewDialog, StepUp, StepUpField, Tree,
    Working, sentence_view,
};
use crate::live::{reply, say};
use crate::repositories::Sentence;
use crate::restore::{self, Asks, Dialog};
use crate::routes::{PageHeading, refused_sentence, short_id, text_with};
use crate::session::Standing;
use crate::text;
use crate::tree::{Selection, folder_url};
use crate::wording::Words;
use crate::ws::{self, Got, Posted};

/// The id of the button that opens the restore dialog, which focus returns
/// to when it closes.
const RESTORE_BUTTON: &str = "restore-open";

/// The values of the choice of where to restore.
const NEW_FOLDER: &str = "new-folder";
const IN_PLACE: &str = "in-place";

/// The page of one snapshot of one repository, both named by its address.
/// Download of what the tree selects is a later slice's (#564).
#[component]
pub(crate) fn Snapshot(localizer: Arc<Localizer>) -> impl IntoView {
    let params = use_params_map();
    // Another snapshot's address draws the page anew.
    move || {
        let (alias, snapshot) = params.with(|params| {
            (
                params.get("alias").unwrap_or_default(),
                params.get("snapshot").unwrap_or_default(),
            )
        });
        snapshot_view(Arc::clone(&localizer), alias, snapshot)
    }
}

fn snapshot_view(localizer: Arc<Localizer>, alias: String, snapshot: String) -> impl IntoView {
    let heading = text_with(
        &localizer,
        msg::UI_SNAPSHOT_HEADING,
        &[
            ("snapshot", MsgArg::Text(short_id(&snapshot))),
            ("alias", MsgArg::Text(alias.clone())),
        ],
    );
    let base = ws::origin();
    let refusals = Arc::clone(&localizer);
    let (tree_alias, tree_snapshot) = (alias.clone(), snapshot.clone());
    let loader: FolderLoader = Rc::new(move |path: String, from: u64| -> FolderPage {
        let url = folder_url(&base, &tree_alias, &tree_snapshot, &path, from);
        let localizer = Arc::clone(&refusals);
        Box::pin(async move {
            match ws::get::<FolderDto>(&url).await {
                Some(Got::Read(folder)) => Ok(folder),
                Some(Got::Refused(code, args, detail)) => {
                    Err(refused_sentence(&localizer, code, &args, detail.as_deref()))
                }
                None => Err(Sentence::new(
                    &text(&localizer, msg::UI_UNREACHABLE_BODY),
                    None,
                )),
            }
        })
    });
    let selected = RwSignal::new(None::<Selection>);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{text(&localizer, msg::UI_SNAPSHOT_BODY)}</p>
        <Tree
            localizer=Arc::clone(&localizer)
            label=text(&localizer, msg::UI_SNAPSHOT_TREE)
            id="snapshot-tree".to_owned()
            loader=loader
            empty=text(&localizer, msg::UI_SNAPSHOT_EMPTY)
            selected=selected
        />
        <RestorePane localizer=Arc::clone(&localizer) alias snapshot selected />
        <p>
            <A href="/snapshots">{text(&localizer, msg::UI_SNAPSHOT_BACK)}</A>
        </p>
    }
}

/// The pane beside the tree: a Restore button named for the entry the tree
/// selects, and the dialog it opens, drawn anew at each opening for the
/// entry selected then, so the tree moving under it changes nothing.
#[component]
fn RestorePane(
    localizer: Arc<Localizer>,
    alias: String,
    snapshot: String,
    selected: RwSignal<Option<Selection>>,
) -> impl IntoView {
    let asks =
        Asks::of(use_context::<RwSignal<Standing>>().map(|standing| standing.get_untracked()));
    let open = RwSignal::new(false);
    let opening = RwSignal::new(None::<(u32, Selection)>);
    let button_localizer = Arc::clone(&localizer);
    let empty = text(&localizer, msg::UI_RESTORE_PANE_EMPTY);
    let button = move || match selected.get() {
        Some(selection) => {
            let label = text_with(
                &button_localizer,
                msg::UI_RESTORE_BUTTON,
                &[("name", MsgArg::Text(selection.name.clone()))],
            );
            view! {
                <p>
                    <button
                        type="button"
                        id=RESTORE_BUTTON
                        on:click=move |_| {
                            let count = opening
                                .with_untracked(|opened| opened.as_ref().map_or(0, |(n, _)| *n));
                            opening.set(Some((count.wrapping_add(1), selection.clone())));
                            open.set(true);
                        }
                    >
                        {label}
                    </button>
                </p>
            }
            .into_any()
        }
        None => view! { <p>{empty.clone()}</p> }.into_any(),
    };
    let dialog_localizer = Arc::clone(&localizer);
    let dialog = move || {
        opening.get().map(|(_, selection)| {
            restore_dialog(
                Arc::clone(&dialog_localizer),
                alias.clone(),
                snapshot.clone(),
                selection,
                asks,
                open,
            )
        })
    };
    view! {
        <section aria-labelledby="restore-pane-heading">
            <h2 id="restore-pane-heading">{text(&localizer, msg::UI_RESTORE_PANE)}</h2>
            {button}
            {dialog}
        </section>
    }
}

/// The restore dialog of `selection`: what it restores, where to, what a
/// restore in place would change, and the step-up field it is confirmed
/// with. On the daemon's start it closes and the page says the restore runs,
/// then how it ended.
fn restore_dialog(
    localizer: Arc<Localizer>,
    alias: String,
    snapshot: String,
    selection: Selection,
    asks: Asks,
    open: RwSignal<bool>,
) -> impl IntoView {
    let base = ws::origin();
    let words = {
        let localizer = Arc::clone(&localizer);
        move |words: &Words| text_with(&localizer, words.id, &words.args)
    };
    let state = RwSignal::new(Dialog::new(selection.path.clone()));
    let chosen = RwSignal::new(NEW_FOLDER);
    let value = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let alert = RwSignal::new(None::<Sentence>);
    let name = selection.name;
    let title = text_with(
        &localizer,
        msg::UI_RESTORE_BUTTON,
        &[("name", MsgArg::Text(name.clone()))],
    );
    let summary = text_with(
        &localizer,
        msg::UI_RESTORE_SUMMARY,
        &[
            ("name", MsgArg::Text(name.clone())),
            ("snapshot", MsgArg::Text(short_id(&snapshot))),
            ("alias", MsgArg::Text(alias.clone())),
        ],
    );
    let step_up = match asks {
        Asks::Password => StepUp::Password,
        Asks::ShortId => StepUp::ShortId {
            label: text(&localizer, msg::UI_RESTORE_SNAPSHOT_ID),
            hint: text(&localizer, msg::UI_RESTORE_SNAPSHOT_ID_HINT),
            short: short_id(&snapshot),
        },
    };
    let alive = move || state.try_get_untracked().is_some();
    let unreachable = Sentence::new(&text(&localizer, msg::UI_UNREACHABLE_BODY), None);
    // A dry run asked for, by choosing in place or reading it again, clears
    // what the last one alerted.
    let preview_url = format!("{base}{}", restore::preview_path(&alias, &snapshot));
    let preview_localizer = Arc::clone(&localizer);
    let preview_unreachable = unreachable.clone();
    let ask_preview = Callback::new(move |request: RestorePreviewRequestDto| {
        alert.set(None);
        let url = preview_url.clone();
        let localizer = Arc::clone(&preview_localizer);
        let unreachable = preview_unreachable.clone();
        spawn_local(async move {
            let answer = ws::post(&url, &request).await;
            if !alive() {
                return;
            }
            let found = match answer {
                Some(Posted::Done(body)) => {
                    let found = serde_json::from_str::<RestorePreviewDto>(&body).ok();
                    if found.is_none() {
                        alert.set(Some(unreachable));
                    }
                    found
                }
                Some(Posted::Refused(code, args, detail)) => {
                    alert.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    None
                }
                None => {
                    alert.set(Some(unreachable));
                    None
                }
            };
            state.update(|dialog| dialog.previewed(found));
        });
    });
    Effect::new(move |_| {
        let destination = if chosen.get() == IN_PLACE {
            RestoreDestinationDto::InPlace
        } else {
            RestoreDestinationDto::NewFolder
        };
        if let Some(request) = state
            .try_update(|dialog| dialog.choose(destination))
            .flatten()
        {
            ask_preview.run(request);
        }
    });
    let read_again = move |_| {
        if let Some(request) = state.try_update(Dialog::read_again).flatten() {
            ask_preview.run(request);
        }
    };
    let read_again_label = text(&localizer, msg::UI_RESTORE_READ_AGAIN);
    // A memo, so the button is drawn again only when in place is chosen or
    // left, never while the dry run it asks for changes, which would take
    // focus off it.
    let in_place = Memo::new(move |_| {
        state.with(|dialog| dialog.destination == RestoreDestinationDto::InPlace)
    });
    let held_words = words.clone();
    let held = Signal::derive(move || state.with(Dialog::held).map(|reason| held_words(&reason)));
    let preview_words = words.clone();
    let preview = move || {
        state
            .with(|dialog| dialog.preview.words())
            .map(|said| preview_words(&said))
    };
    let start_url = format!("{base}{}", restore::start_path(&alias, &snapshot));
    let start_localizer = Arc::clone(&localizer);
    let start_words = words.clone();
    let start = Callback::new(move |()| {
        let Some(request) = state
            .try_update(|dialog| dialog.confirm(value.get_untracked()))
            .flatten()
        else {
            return;
        };
        alert.set(None);
        let url = start_url.clone();
        let base = base.clone();
        let localizer = Arc::clone(&start_localizer);
        let words = start_words.clone();
        let unreachable = unreachable.clone();
        let name = name.clone();
        spawn_local(async move {
            let answer = ws::post(&url, &request).await;
            if !alive() {
                return;
            }
            state.update(Dialog::answered);
            match answer {
                Some(Posted::Done(body)) => {
                    match serde_json::from_str::<RestoreStartedDto>(&body) {
                        Ok(RestoreStartedDto { operation, folder }) => {
                            error.set(None);
                            value.set(String::new());
                            open.set(false);
                            reply(Politeness::Polite, &words(&restore::running(&name)));
                            let awaited = ws::await_operation(&base, &operation, || true).await;
                            if let Some((said, politeness)) =
                                restore::ended(&name, folder.as_deref(), &awaited)
                            {
                                say(politeness, &words(&said));
                            }
                        }
                        Err(_) => alert.set(Some(unreachable)),
                    }
                }
                Some(Posted::Refused(code, args, detail)) => {
                    match restore::field_error(code, asks) {
                        // Cleared first, so a second wrong try is said again.
                        Some(said) => {
                            error.set(None);
                            tick().await;
                            error.try_set(Some(words(&said)));
                        }
                        None => alert.set(Some(refused_sentence(
                            &localizer,
                            code,
                            &args,
                            detail.as_deref(),
                        ))),
                    }
                }
                None => alert.set(Some(unreachable)),
            }
        });
    });
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label: text(&localizer, msg::UI_RESTORE_CONFIRM),
                action: start,
            })
            returns_to=RESTORE_BUTTON.to_owned()
            described_by="restore-summary".to_owned()
            working=Working {
                active: Signal::derive(move || state.with(|dialog| dialog.starting)),
                status: text(&localizer, msg::UI_RESTORE_STARTING),
            }
            held=held
        >
            <p id="restore-summary">{summary}</p>
            <RadioGroup
                name="restore-where"
                legend=text(&localizer, msg::UI_RESTORE_WHERE)
                description=text(&localizer, msg::UI_RESTORE_WHERE_HINT)
                options=vec![
                    (NEW_FOLDER, text(&localizer, msg::UI_RESTORE_NEW_FOLDER)),
                    (IN_PLACE, text(&localizer, msg::UI_RESTORE_IN_PLACE)),
                ]
                chosen=chosen
            />
            <p aria-live="polite" aria-atomic="true">
                {preview}
            </p>
            {move || {
                let label = read_again_label.clone();
                in_place
                    .get()
                    .then(|| {
                        view! {
                            <p>
                                <button
                                    type="button"
                                    aria-disabled=move || {
                                        (!state.with(Dialog::can_read_again)).then_some("true")
                                    }
                                    on:click=read_again
                                >
                                    {label}
                                </button>
                            </p>
                        }
                    })
            }}
            {move || {
                alert
                    .get()
                    .map(|sentence| {
                        view! {
                            <p class="form-error" role="alert">
                                {sentence_view(sentence)}
                            </p>
                        }
                    })
            }}
            <StepUpField
                id="restore-step-up"
                localizer=Arc::clone(&localizer)
                step_up=step_up
                value=value
                error=error
            />
        </ReviewDialog>
    }
}
