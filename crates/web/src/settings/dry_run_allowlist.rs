//! The dry-run allowlist's section of Settings (§16, decision 89, point 13):
//! the folders a plan dry run an AI agent asks for may read, listed in the
//! shared table, each removed once its dialog is confirmed; a folder added
//! by its full path; and "Add home folder", which warns first of what the
//! home folder opens, as a folder typed that is the home folder or holds it
//! does. What each decides is `dry_run_allowlist.rs`'s, host-tested; this
//! is the DOM glue, which the `@ui` scenarios of
//! `features/ui/dry_run_allowlist.feature` run.

use std::sync::Arc;

use api::{DryRunAllowlistDto, ErrorCode, MsgArg};
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::say;
use crate::announcer::Politeness;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::{
    Button, Column, Confirmation, Name, ReviewDialog, Sentences, State, Table, TextField, Working,
    dialog_refusal_alert, hand_focus_when_gone, refusal_alert, say_refusals, sentence_view,
};
use crate::dry_run_allowlist::{
    ALLOWLIST_PATH, HEADING_ID, Listed, added, alert_words, field_refusal, home_body, home_offered,
    opens_home, remove_id, remove_label, remove_title, removed, request, rows, with_added, without,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::{refused_sentence, text_with};
use crate::text;
use crate::variants::{Rem, Size, Tone};
use crate::ws::{self, Got, Posted};

/// The `id` of the field that takes a folder's path.
const FIELD_ID: &str = "dry-run-folder";

/// The `id` of the button that adds the folder typed.
const ADD_ID: &str = "dry-run-folder-add";

/// The `id` of "Add home folder", where focus returns when its dialog is
/// left without adding it.
const ADD_HOME_ID: &str = "dry-run-home-add";

/// The `id` of what the remove dialog says, which describes it.
const REMOVE_BODY_ID: &str = "dry-run-folder-remove-body";

/// The `id` of what the home folder's dialog says, which describes it.
const HOME_BODY_ID: &str = "dry-run-home-body";

/// What a save of the list came to.
enum Saved {
    /// The daemon saved it, and answered the list as it now is.
    Done(DryRunAllowlistDto),
    /// The daemon refused it: its code, the code's values and its detail.
    Refused(ErrorCode, Vec<(String, MsgArg)>, Option<String>),
    /// The daemon didn't answer, or not as the route does.
    Unreachable,
}

/// Saves `folders` as the whole list.
async fn save(url: &str, folders: Vec<String>) -> Saved {
    match ws::put(url, &request(folders)).await {
        Some(Posted::Done(answer)) => serde_json::from_str::<DryRunAllowlistDto>(&answer)
            .map_or(Saved::Unreachable, Saved::Done),
        Some(Posted::Refused(code, args, detail)) => Saved::Refused(code, args, detail),
        None => Saved::Unreachable,
    }
}

/// The alert's sentence for the daemon's refusal of a save: the page's own
/// words where the daemon's would name a control the page hasn't, else the
/// daemon's.
fn refusal(
    localizer: &Localizer,
    code: ErrorCode,
    args: &[(String, MsgArg)],
    detail: Option<&str>,
) -> Sentence {
    match alert_words(code) {
        Some(words) => {
            let args: Vec<(&str, MsgArg)> = args
                .iter()
                .map(|(name, arg)| (name.as_str(), arg.clone()))
                .collect();
            Sentence::new(&text_with(localizer, words, &args), None)
        }
        None => refused_sentence(localizer, code, args, detail),
    }
}

/// The sentence for a daemon that didn't answer a save.
fn unreachable(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_SESSION_UNREACHABLE), None)
}

/// What the section holds: the list as the daemon last answered it, and
/// whether a save of it is going. Every change saves the whole list, so one
/// save at a time: while one goes, every control that changes the list is
/// busy, and takes no press.
#[derive(Clone, Copy)]
struct Allowlist {
    base: StoredValue<String>,
    /// The list; `None` until the read answers, and while it is refused.
    held: RwSignal<Option<Listed>>,
    saving: RwSignal<bool>,
}

impl Allowlist {
    fn url(self) -> String {
        format!("{}{ALLOWLIST_PATH}", self.base.get_value())
    }

    /// The folders as the list holds them, none while unread.
    fn current(self) -> Vec<String> {
        self.held
            .with_untracked(|held| held.as_ref().map(|held| held.folders.clone()))
            .unwrap_or_default()
    }

    /// The daemon's home folder, as the list last named it.
    fn home(self) -> Option<String> {
        self.held
            .with_untracked(|held| held.as_ref().and_then(|held| held.home.clone()))
    }

    /// Takes the daemon's answer to a save.
    fn take(self, answer: DryRunAllowlistDto) {
        self.held.update(|held| match held {
            Some(held) => held.take(answer),
            None => *held = Some(Listed::read(answer)),
        });
    }

    /// Starts a save: `false`, and nothing started, while another goes.
    fn start(self) -> bool {
        if self.saving.get_untracked() {
            return false;
        }
        self.saving.set(true);
        true
    }

    /// Ends the save, unless the reader has left the page.
    fn end(self) {
        self.saving.try_set(false);
    }
}

/// A folder the home folder's dialog asks about: the daemon's home folder
/// "Add home folder" offers, or a folder typed that is it or holds it.
#[derive(Clone)]
struct Asked {
    folder: String,
    home: String,
    /// Whether the Folder field asked, where focus then returns.
    typed: bool,
}

/// The dry-run allowlist's section: the text that says what it is, the
/// table of folders (or, empty, why a dry run then refuses every source),
/// "Add home folder", the form that adds a folder, and the dialogs that ask
/// before a folder is removed and before the home folder is added. A read
/// the daemon refuses is said calmly in the table's place, with no form.
#[component]
pub(super) fn DryRunAllowlistSection(localizer: Arc<Localizer>) -> impl IntoView {
    let list = Allowlist {
        base: StoredValue::new(ws::origin()),
        held: RwSignal::new(None),
        saving: RwSignal::new(false),
    };
    let state = RwSignal::new(State::Loading);
    let read_refused = RwSignal::<Option<Sentence>>::new(None);
    let read_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let read = ws::get::<DryRunAllowlistDto>(&list.url()).await;
        // The reader may have left the page before the answer came.
        if state.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                list.held.set(Some(Listed::read(read)));
                state.set(State::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                read_refused.set(Some(refused_sentence(
                    &read_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                state.set(State::Ready);
            }
            None => state.set(State::Unreachable),
        }
    });
    let readable = Memo::new(move |_| list.held.with(Option::is_some));
    let shown = Signal::derive(move || {
        list.held.with(|held| {
            held.as_ref()
                .map(|held| rows(&held.folders))
                .unwrap_or_default()
        })
    });
    let offered = Memo::new(move |_| {
        list.held.with(|held| {
            held.as_ref()
                .and_then(|held| home_offered(&held.folders, held.home.as_deref()))
        })
    });
    // What the Folder field holds: the form's, and the home folder's dialog
    // empties it once the folder typed is added.
    let typed = RwSignal::new(String::new());

    // The remove dialog: the folder it is about, and what removing came to.
    let removing = RwSignal::<Option<String>>::new(None);
    let remove_open = RwSignal::new(false);
    let remove_error = RwSignal::<Option<Sentence>>::new(None);
    let ask_remove = Callback::new(move |folder: String| {
        remove_error.set(None);
        removing.set(Some(folder));
        remove_open.set(true);
    });
    let remove_localizer = Arc::clone(&localizer);
    let remove = Callback::new(move |folder: String| {
        if !list.start() {
            return;
        }
        remove_error.set(None);
        let localizer = Arc::clone(&remove_localizer);
        let folders = without(&list.current(), &folder);
        spawn_local(async move {
            let saved = save(&list.url(), folders).await;
            if list.saving.try_get_untracked().is_none() {
                return;
            }
            match saved {
                Saved::Done(answer) => {
                    list.take(answer);
                    reply(Politeness::Polite, &say(&localizer, removed(&folder)));
                    // Once the row has gone, so the dialog hands focus to the
                    // heading rather than to the button the row took away;
                    // busy until then, so the confirm button takes no press.
                    request_animation_frame(move || {
                        list.end();
                        remove_open.try_set(false);
                    });
                }
                Saved::Refused(code, args, detail) => {
                    list.end();
                    remove_error.set(Some(refusal(&localizer, code, &args, detail.as_deref())));
                }
                Saved::Unreachable => {
                    list.end();
                    remove_error.set(Some(unreachable(&localizer)));
                }
            }
        });
    });

    // The home folder's dialog: what it asks about, and what adding came to.
    let asking = RwSignal::<Option<Asked>>::new(None);
    let home_open = RwSignal::new(false);
    let home_error = RwSignal::<Option<Sentence>>::new(None);
    let ask_home = Callback::new(move |asked: Asked| {
        home_error.set(None);
        asking.set(Some(asked));
        home_open.set(true);
    });
    let home_localizer = Arc::clone(&localizer);
    let add_home = Callback::new(move |asked: Asked| {
        let Ok(folders) = with_added(&list.current(), &asked.folder) else {
            return;
        };
        if !list.start() {
            return;
        }
        home_error.set(None);
        let localizer = Arc::clone(&home_localizer);
        spawn_local(async move {
            let saved = save(&list.url(), folders).await;
            if list.saving.try_get_untracked().is_none() {
                return;
            }
            match saved {
                Saved::Done(answer) => {
                    list.take(answer);
                    if asked.typed {
                        empty_if_still(typed, &asked.folder);
                    }
                    reply(Politeness::Polite, &say(&localizer, added(&asked.folder)));
                    // Once "Add home folder" has gone, so the dialog hands
                    // focus to the heading; busy until then.
                    request_animation_frame(move || {
                        list.end();
                        home_open.try_set(false);
                    });
                }
                Saved::Refused(code, args, detail) => {
                    list.end();
                    home_error.set(Some(refusal(&localizer, code, &args, detail.as_deref())));
                }
                Saved::Unreachable => {
                    list.end();
                    home_error.set(Some(unreachable(&localizer)));
                }
            }
        });
    });

    let heading = text(&localizer, msg::UI_DRY_RUN_ALLOWLIST_HEADING);
    let body = text(&localizer, msg::UI_DRY_RUN_ALLOWLIST_BODY);
    let table_localizer = Arc::clone(&localizer);
    let home_button = text(&localizer, msg::UI_DRY_RUN_HOME_ADD);
    let form_localizer = Arc::clone(&localizer);
    let remove_dialog_localizer = Arc::clone(&localizer);
    let home_dialog_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            <p>{body}</p>
            {move || match read_refused.get() {
                Some(sentence) => view! { <p>{sentence_view(sentence)}</p> }.into_any(),
                None => {
                    folders_table(&table_localizer, shown, state.into(), list.saving, ask_remove)
                        .into_any()
                }
            }}
            {move || {
                offered
                    .get()
                    .map(|home| {
                        view! {
                            <p class="actions">
                                <Button
                                    content=Content::Text(home_button.clone().into())
                                    id=ADD_HOME_ID
                                    busy=list.saving
                                    on_press=Callback::new(move |()| {
                                        ask_home
                                            .run(Asked {
                                                folder: home.clone(),
                                                home: home.clone(),
                                                typed: false,
                                            })
                                    })
                                />
                            </p>
                        }
                    })
            }}
            {move || {
                readable
                    .get()
                    .then(|| {
                        view! {
                            <AddForm
                                localizer=Arc::clone(&form_localizer)
                                list=list
                                typed=typed
                                ask_home=ask_home
                            />
                        }
                    })
            }}
            {move || {
                removing
                    .get()
                    .map(|folder| {
                        view! {
                            <RemoveDialog
                                localizer=Arc::clone(&remove_dialog_localizer)
                                folder=folder
                                open=remove_open
                                busy=list.saving
                                error=remove_error
                                remove=remove
                            />
                        }
                    })
            }}
            {move || {
                asking
                    .get()
                    .map(|asked| {
                        view! {
                            <HomeDialog
                                localizer=Arc::clone(&home_dialog_localizer)
                                asked=asked
                                open=home_open
                                busy=list.saving
                                error=home_error
                                add=add_home
                            />
                        }
                    })
            }}
        </section>
    }
}

/// Empties the Folder field once `folder` is added, unless what it holds
/// has changed since: what was typed meanwhile stays.
fn empty_if_still(typed: RwSignal<String>, folder: &str) {
    if typed
        .try_with_untracked(|now| now.trim() == folder)
        .unwrap_or(false)
    {
        typed.set(String::new());
    }
}

/// The table of folders, named by the section's heading: each folder the
/// row's header, isolated as data, and its Remove button named for it,
/// busy while the list saves, which hands focus to the heading when its row
/// goes while it holds focus.
fn folders_table(
    localizer: &Arc<Localizer>,
    rows: Signal<Vec<String>>,
    state: Signal<State>,
    saving: RwSignal<bool>,
    ask_remove: Callback<String>,
) -> impl IntoView + use<> {
    let folder = Column::new(
        text(localizer, msg::UI_DRY_RUN_ALLOWLIST_COLUMN_FOLDER),
        Rem::tenths(240),
        |row: Signal<String>| view! { <bdi>{move || row.get()}</bdi> }.into_any(),
    );
    let buttons = Arc::clone(localizer);
    let actions = Column::new(
        text(localizer, msg::UI_COLUMN_ACTIONS),
        Rem::tenths(160),
        move |row: Signal<String>| {
            let folder = row.get_untracked();
            let label = say(&buttons, remove_label(&folder));
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone(button, HEADING_ID.to_owned());
            view! {
                <Button
                    content=Content::IconAndText {
                        glyph: LucideGlyph::Trash.into(),
                        label: label.into(),
                    }
                    tone=Tone::Error
                    size=Size::Small
                    id=remove_id(&folder)
                    node_ref=button
                    busy=saving
                    on_press=Callback::new(move |()| ask_remove.run(folder.clone()))
                />
            }
            .into_any()
        },
    );
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_DRY_RUN_ALLOWLIST_NONE),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=Name::Heading(HEADING_ID.to_owned())
            row_header=folder
            columns=vec![actions]
            rows=rows
            key=|folder: &String| folder.clone()
            state=state
            sentences=sentences
        />
    }
}

/// The form that adds a folder: its path, checked for nothing typed and for
/// a folder listed already before it is sent; a folder that is the home
/// folder or holds it asks first in the home folder's dialog. A field's
/// error, and the daemon's refusal of a folder that isn't a full path, are
/// the field's, said assertively; any other refusal of the save is an
/// Error alert after the form, said once. Focus stays where it was, and the
/// field keeps what was typed until that folder is added.
#[component]
fn AddForm(
    localizer: Arc<Localizer>,
    list: Allowlist,
    typed: RwSignal<String>,
    ask_home: Callback<Asked>,
) -> impl IntoView {
    let error = RwSignal::<Option<String>>::new(None);
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let submit_localizer = Arc::clone(&localizer);
    let submit = move || {
        if list.saving.get_untracked() {
            return;
        }
        refused.set(None);
        let folders = match with_added(&list.current(), &typed.get_untracked()) {
            Ok(folders) => folders,
            Err(lack) => {
                let said = text(&submit_localizer, lack.said());
                reply(Politeness::Assertive, &said);
                error.set(Some(said));
                return;
            }
        };
        error.set(None);
        let folder = folders.last().cloned().unwrap_or_default();
        let home = list.home();
        if let Some(home) = home
            .clone()
            .filter(|home| opens_home(&folder, Some(home.as_str())))
        {
            ask_home.run(Asked {
                folder,
                home,
                typed: true,
            });
            return;
        }
        if !list.start() {
            return;
        }
        reply(
            Politeness::Polite,
            &text(&submit_localizer, msg::UI_DRY_RUN_FOLDER_ADDING),
        );
        let localizer = Arc::clone(&submit_localizer);
        spawn_local(async move {
            let saved = save(&list.url(), folders.clone()).await;
            if list.saving.try_get_untracked().is_none() {
                return;
            }
            list.end();
            match saved {
                Saved::Done(answer) => {
                    list.take(answer);
                    empty_if_still(typed, &folder);
                    reply(Politeness::Polite, &say(&localizer, added(&folder)));
                }
                Saved::Refused(code, args, detail) => {
                    match field_refusal(code, &args, &folders, home.as_deref()) {
                        Some(said) => {
                            let said = say(&localizer, said);
                            reply(Politeness::Assertive, &said);
                            error.set(Some(said));
                        }
                        None => {
                            refused.set(Some(refusal(&localizer, code, &args, detail.as_deref())))
                        }
                    }
                }
                Saved::Unreachable => refused.set(Some(unreachable(&localizer))),
            }
        });
    };
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby="dry-run-folder-form-heading">
            <h3 id="dry-run-folder-form-heading">
                {text(&localizer, msg::UI_DRY_RUN_FOLDER_FORM_HEADING)}
            </h3>
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    submit();
                }
            >
                <TextField
                    id=FIELD_ID
                    localizer=Arc::clone(&localizer)
                    label=text(&localizer, msg::UI_DRY_RUN_FOLDER_LABEL)
                    description=text(&localizer, msg::UI_DRY_RUN_FOLDER_HINT)
                    error=error
                    value=typed
                    required=true
                />
                <p class="actions">
                    <Button
                        content=Content::Text(text(&localizer, msg::UI_DRY_RUN_FOLDER_ADD).into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        kind=Kind::Submit
                        id=ADD_ID
                        busy=list.saving
                    />
                </p>
            </form>
            {move || refused.get().map(|sentence| refusal_alert(&alert_localizer, sentence))}
        </section>
    }
}

/// The dialog that asks before `folder` is removed, named for it: Cancel
/// first, so a stray Enter backs out. While the daemon saves, the dialog
/// holds still and says so; a refusal is an alert inside it.
#[component]
fn RemoveDialog(
    localizer: Arc<Localizer>,
    folder: String,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<Sentence>>,
    remove: Callback<String>,
) -> impl IntoView {
    let title = say(&localizer, remove_title(&folder));
    let returns_to = remove_id(&folder);
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_DRY_RUN_FOLDER_REMOVE_CONFIRM),
        tone: Tone::Error,
        action: Callback::new(move |()| remove.run(folder.clone())),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_DRY_RUN_FOLDER_REMOVING),
    };
    let error_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=returns_to
            described_by=REMOVE_BODY_ID.to_owned()
            working=working
            fallback=HEADING_ID.to_owned()
        >
            <p id=REMOVE_BODY_ID>{text(&localizer, msg::UI_DRY_RUN_FOLDER_REMOVE_BODY)}</p>
            {move || error.get().map(|sentence| dialog_refusal_alert(&error_localizer, sentence))}
        </ReviewDialog>
    }
}

/// The dialog that asks before the home folder, or a folder typed that is
/// it or holds it, is added: it warns that an AI agent could then count the
/// files under the whole home folder, `~/.ssh` and browser profiles
/// included, though never learn a name, and suggests a narrower folder.
/// Cancel first, the safe choice, so a stray Enter backs out. Left, it
/// returns focus to what asked: "Add home folder", or the Folder field.
#[component]
fn HomeDialog(
    localizer: Arc<Localizer>,
    asked: Asked,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<Sentence>>,
    add: Callback<Asked>,
) -> impl IntoView {
    let returns_to = if asked.typed { FIELD_ID } else { ADD_HOME_ID };
    let warning = say(&localizer, home_body(&asked.home));
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_DRY_RUN_HOME_CONFIRM),
        tone: Tone::Primary,
        action: Callback::new(move |()| add.run(asked.clone())),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_DRY_RUN_FOLDER_ADDING),
    };
    let error_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=open
            title=text(&localizer, msg::UI_DRY_RUN_HOME_TITLE)
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=returns_to.to_owned()
            described_by=HOME_BODY_ID.to_owned()
            working=working
            fallback=HEADING_ID.to_owned()
        >
            <div id=HOME_BODY_ID>
                <p>{warning}</p>
                <p>{text(&localizer, msg::UI_DRY_RUN_HOME_NARROWER)}</p>
            </div>
            {move || error.get().map(|sentence| dialog_refusal_alert(&error_localizer, sentence))}
        </ReviewDialog>
    }
}
