//! A snapshot's page's DOM glue (technical design §4.4, §10.4): the route
//! `/snapshots/{alias}/{snapshot}`, the snapshot's folders and files in a
//! tree, each folder read as it is opened, and beside it the pane that
//! restores or downloads the entry the tree selects. It decides nothing: the
//! address of a page is the tree rules' `folder_url`, and the tree's keys,
//! rows and answers are `components/tree/rules.rs`'s; what a restore sends,
//! when it can be confirmed and what it says are `restore.rs`'s, and a
//! download's are `download.rs`'s, host-tested. What stays here is asking
//! the daemon and drawing the page; the journeys of
//! `features/ui/snapshot_browser.feature`,
//! `features/restore/restore.feature` and `features/restore/download.feature`
//! run it in a browser, the `ws.rs` precedent.

use std::rc::Rc;
use std::sync::Arc;

use api::{
    DownloadStartedDto, FolderDto, MsgArg, RestoreDestinationDto, RestorePreviewDto,
    RestorePreviewRequestDto, RestoreStartedDto,
};
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};
use leptos_router::hooks::use_params_map;

use crate::components::ACTIONS;
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, Matching};
use crate::components::tree::rules::{Selection, folder_url};
use crate::components::{
    Button, Confirmation, FolderLoader, FolderPage, Link, RadioGroup, ReviewDialog, StepUp,
    StepUpField, Toast, ToastTone, Toasts, Tree, Working, dialog_refusal_alert,
};
use crate::download;
use crate::live::say;
use crate::repositories::Sentence;
use crate::restore::{self, Asks, Dialog};
use crate::routes::{PageHeading, refused_sentence, short_id, text_with};
use crate::session::Standing;
use crate::text;
use crate::variants::Tone;
use crate::wording::Words;
use crate::ws::{self, Got, Posted};

/// The id of the button that opens the restore dialog, which focus returns
/// to when it closes.
const RESTORE_BUTTON: &str = "restore-open";

/// The id of the button that opens the download dialog, which focus returns
/// to when it closes.
const DOWNLOAD_BUTTON: &str = "download-open";

/// The values of the choice of where to restore.
const NEW_FOLDER: &str = "new-folder";
const IN_PLACE: &str = "in-place";

/// The pane's toast, which answers a restore or a download started, and the
/// button its dismissal hands focus back to: the one whose dialog started
/// what it says (2.4.3).
#[derive(Clone, Copy)]
struct Started {
    toasts: Toasts,
    returns_to: RwSignal<&'static str>,
}

impl Started {
    fn new() -> Self {
        Self {
            toasts: Toasts::new(),
            returns_to: RwSignal::new(RESTORE_BUTTON),
        }
    }

    /// Shows `words` as information, its dismissal handing focus to the
    /// button with the ID `button`.
    fn show(self, button: &'static str, words: String) {
        self.returns_to.set(button);
        self.toasts.show(ToastTone::Primary, words);
    }
}

/// The page of one snapshot of one repository, both named by its address.
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
    // Back to the Snapshots page, which is never the page the reader is on
    // here: the navigation's Snapshots link marks the section instead.
    let back = Destination::route("/snapshots", Matching::Exact);
    let back_words = text(&localizer, msg::UI_SNAPSHOT_BACK);
    let back_localizer = Arc::clone(&localizer);
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
        <EntryPane localizer=Arc::clone(&localizer) alias snapshot selected />
        <p>
            {back
                .map(|to| {
                    view! {
                        <Link to=to localizer=back_localizer>
                            {back_words}
                        </Link>
                    }
                })}
        </p>
    }
}

/// The pane beside the tree: a Restore and a Download button named for the
/// entry the tree selects, the toast that answers a restore or a download
/// started, which nothing else on the page shows, and the dialogs they
/// open, each drawn anew at each opening for the entry selected then, so
/// the tree moving under it changes nothing.
#[component]
fn EntryPane(
    localizer: Arc<Localizer>,
    alias: String,
    snapshot: String,
    selected: RwSignal<Option<Selection>>,
) -> impl IntoView {
    let asks =
        Asks::of(use_context::<RwSignal<Standing>>().map(|standing| standing.get_untracked()));
    let restore_open = RwSignal::new(false);
    let restoring = RwSignal::new(None::<(u32, Selection)>);
    let download_open = RwSignal::new(false);
    let downloading = RwSignal::new(None::<(u32, Selection)>);
    let started = Started::new();
    let button_localizer = Arc::clone(&localizer);
    let empty = text(&localizer, msg::UI_RESTORE_PANE_EMPTY);
    let button = move || match selected.get() {
        Some(selection) => {
            let named = [("name", MsgArg::Text(selection.name.clone()))];
            let restore_label = text_with(&button_localizer, msg::UI_RESTORE_BUTTON, &named);
            let download_label = text_with(&button_localizer, msg::UI_DOWNLOAD_BUTTON, &named);
            let restored = selection.clone();
            view! {
                <p class=ACTIONS>
                    <Button
                        content=Content::IconAndText {
                            glyph: LucideGlyph::RotateCcw.into(),
                            label: restore_label.into(),
                        }
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        id=RESTORE_BUTTON
                        on_press=Callback::new(move |()| {
                            opened(restoring, restore_open, &restored);
                        })
                    />
                    <Button
                        content=Content::Text(download_label.into())
                        appearance=Appearance::Outlined
                        tone=Tone::Secondary
                        id=DOWNLOAD_BUTTON
                        on_press=Callback::new(move |()| {
                            opened(downloading, download_open, &selection);
                        })
                    />
                </p>
            }
            .into_any()
        }
        None => view! { <p>{empty.clone()}</p> }.into_any(),
    };
    let restore_localizer = Arc::clone(&localizer);
    let (restore_alias, restore_snapshot) = (alias.clone(), snapshot.clone());
    let restore = move || {
        restoring.get().map(|(_, selection)| {
            restore_dialog(
                Arc::clone(&restore_localizer),
                restore_alias.clone(),
                restore_snapshot.clone(),
                selection,
                asks,
                restore_open,
                started,
            )
        })
    };
    let download_localizer = Arc::clone(&localizer);
    let download = move || {
        downloading.get().map(|(_, selection)| {
            download_dialog(
                Arc::clone(&download_localizer),
                alias.clone(),
                snapshot.clone(),
                selection,
                asks,
                download_open,
                started,
            )
        })
    };
    view! {
        <section aria-labelledby="entry-pane-heading">
            <h2 id="entry-pane-heading">{text(&localizer, msg::UI_RESTORE_PANE)}</h2>
            {button}
            // Dismissed, a toast hands focus to the button whose dialog
            // started what it says, there while an entry is selected, as it
            // is once either has started.
            <Toast
                localizer=Arc::clone(&localizer)
                toasts=started.toasts
                then_focus=Signal::derive(move || started.returns_to.get().to_owned())
            />
            {restore}
            {download}
        </section>
    }
}

/// A dialog's button was pressed with `selection` selected: its dialog is
/// drawn anew for it, under a count that changes at each opening, and opens.
fn opened(
    opening: RwSignal<Option<(u32, Selection)>>,
    open: RwSignal<bool>,
    selection: &Selection,
) {
    let count = opening.with_untracked(|opened| opened.as_ref().map_or(0, |(n, _)| *n));
    opening.set(Some((count.wrapping_add(1), selection.clone())));
    open.set(true);
}

/// The restore dialog of `selection`: what it restores, where to, what a
/// restore in place would change, and the step-up field it is confirmed
/// with. On the daemon's start it closes and the pane's toast answers that
/// the restore started; the page then says how it ended.
fn restore_dialog(
    localizer: Arc<Localizer>,
    alias: String,
    snapshot: String,
    selection: Selection,
    asks: Asks,
    open: RwSignal<bool>,
    started: Started,
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
                            started.show(RESTORE_BUTTON, words(&restore::running(&name)));
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
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label: text(&localizer, msg::UI_RESTORE_CONFIRM),
                tone: Tone::Primary,
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
                                <Button
                                    content=Content::Text(label.into())
                                    disabled=Signal::derive(move || {
                                        !state.with(Dialog::can_read_again)
                                    })
                                    on_press=Callback::new(read_again)
                                />
                            </p>
                        }
                    })
            }}
            {move || alert.get().map(|sentence| dialog_refusal_alert(&alert_localizer, sentence))}
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

/// The download dialog of `selection`: what it downloads, as which archive
/// when it is a folder, and the step-up field it is confirmed with. On the
/// daemon's ticket it closes, the pane's toast answers that the download
/// started, and the browser is sent to the ticket's address, which it saves
/// as a download.
fn download_dialog(
    localizer: Arc<Localizer>,
    alias: String,
    snapshot: String,
    selection: Selection,
    asks: Asks,
    open: RwSignal<bool>,
    started: Started,
) -> impl IntoView {
    let base = ws::origin();
    let state = RwSignal::new(download::Dialog::new(
        selection.path.clone(),
        selection.folder,
    ));
    let asks_format = state.with_untracked(download::Dialog::asks_format);
    let chosen = RwSignal::new(download::TAR);
    let value = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let alert = RwSignal::new(None::<Sentence>);
    let name = selection.name;
    let title = text_with(
        &localizer,
        msg::UI_DOWNLOAD_BUTTON,
        &[("name", MsgArg::Text(name.clone()))],
    );
    let summary = text_with(
        &localizer,
        msg::UI_DOWNLOAD_SUMMARY,
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
    let start_url = format!("{base}{}", download::start_path(&alias, &snapshot));
    let start_localizer = Arc::clone(&localizer);
    let start = Callback::new(move |()| {
        let Some(request) = state
            .try_update(|dialog| dialog.confirm(chosen.get_untracked(), value.get_untracked()))
            .flatten()
        else {
            return;
        };
        alert.set(None);
        let url = start_url.clone();
        let base = base.clone();
        let localizer = Arc::clone(&start_localizer);
        let unreachable = unreachable.clone();
        let name = name.clone();
        spawn_local(async move {
            let words = |words: &Words| text_with(&localizer, words.id, &words.args);
            let answer = ws::post(&url, &request).await;
            if !alive() {
                return;
            }
            state.update(download::Dialog::answered);
            match answer {
                Some(Posted::Done(body)) => match serde_json::from_str::<DownloadStartedDto>(&body)
                {
                    Ok(DownloadStartedDto { ticket }) => {
                        let saved = format!("{base}{}", download::fetch_path(&ticket));
                        let handed = web_sys::window()
                            .is_some_and(|window| window.location().assign(&saved).is_ok());
                        if handed {
                            error.set(None);
                            value.set(String::new());
                            open.set(false);
                            started.show(DOWNLOAD_BUTTON, words(&download::handed(&name)));
                        } else {
                            alert.set(Some(unreachable));
                        }
                    }
                    Err(_) => alert.set(Some(unreachable)),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    match download::field_error(code, asks) {
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
    let format = asks_format.then(|| {
        view! {
            <RadioGroup
                name="download-format"
                legend=text(&localizer, msg::UI_DOWNLOAD_FORMAT)
                description=text(&localizer, msg::UI_DOWNLOAD_FORMAT_HINT)
                options=vec![
                    (download::TAR, text(&localizer, msg::UI_DOWNLOAD_TAR)),
                    (download::ZIP, text(&localizer, msg::UI_DOWNLOAD_ZIP)),
                ]
                chosen=chosen
            />
        }
    });
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label: text(&localizer, msg::UI_DOWNLOAD_CONFIRM),
                tone: Tone::Primary,
                action: start,
            })
            returns_to=DOWNLOAD_BUTTON.to_owned()
            described_by="download-summary".to_owned()
            working=Working {
                active: Signal::derive(move || state.with(|dialog| dialog.starting)),
                status: text(&localizer, msg::UI_DOWNLOAD_STARTING),
            }
        >
            <p id="download-summary">{summary}</p>
            {format}
            {move || alert.get().map(|sentence| dialog_refusal_alert(&alert_localizer, sentence))}
            <StepUpField
                id="download-step-up"
                localizer=Arc::clone(&localizer)
                step_up=step_up
                value=value
                error=error
            />
        </ReviewDialog>
    }
}
