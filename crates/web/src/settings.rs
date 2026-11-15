//! The Settings page, the route `/settings` (§4.4): its API tokens (security
//! §10.1, decision 93), listed, created and revoked in a session, and the
//! restart of Ladderback (decision NN). What the form lacks, the request it
//! sends, which field a refusal is about and how a token's row reads are
//! `api_tokens.rs`'s, and what the restart section offers and waits for is
//! `restart.rs`'s, all host-tested; this is the DOM glue, which the `@ui`
//! scenarios of `features/security/api_tokens.feature` and
//! `features/execution/restart_from_settings.feature` run in Chromium.
//!
//! A new token's whole text is shown once, in a dialog, and held in one
//! signal only while that dialog is open: leaving the dialog, however it is
//! left, clears it from the page (I4).

use std::sync::Arc;

use api::{
    ApiTokenDto, ApiTokenScopeDto, ApiTokensDto, IanaZone, MsgArg, RestartDto, RestartWhenDto,
};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos::wasm_bindgen::closure::Closure;
use leptos::wasm_bindgen::{JsCast, JsValue};

use crate::announcer::Politeness;
use crate::api_tokens::{
    CREATE_ID, CREATED_BODY_ID, Field, HEADING_ID, LABEL_MAX_CHARS, LastUsed, REVOKE_BODY_ID,
    SCOPES, Said, TOKENS_PATH, TokenForm, created, created_body, error_of, field_of, problems_of,
    revoke_id, revoke_label, revoke_path, revoke_title, revoked, scope_hint, scope_id, scope_name,
};
use crate::components::{
    Checkbox, Confirmation, ErrorSummary, FormField, ReviewDialog, SummaryItem, Working,
    hand_focus_when_gone, sentence_view,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::restart::{
    self, ANSWER_PATIENCE, PENDING_EVERY, RESTART_PATH, Stage, WAIT_EVERY, Wait, Waited,
    confirm_said, offers_now, request, running_said, status_of,
};
use crate::routes::{PageHeading, instant, refused_sentence, text_with};
use crate::session::{Standing, waiting_password};
use crate::text;
use crate::ws::{self, Got, Phase, Posted};

/// The text of `said` in the page's language.
fn say(localizer: &Localizer, (id, args): Said) -> String {
    text_with(localizer, id, &args)
}

/// The `id` of the scopes' hint, which describes their group.
const SCOPES_HINT_ID: &str = "api-token-scopes-hint";

/// The `id` of the scopes' error, which describes their group while it shows.
const SCOPES_ERROR_ID: &str = "api-token-scopes-error";

/// The `id` of the field that holds a new token's whole text.
const SECRET_ID: &str = "api-token-secret";

/// The Settings page. With sign-in off, it asks the daemon on opening whether
/// an admin password has been set since it started, which waits for a restart
/// to turn sign-in on (decision 70), and says so above the API tokens.
#[component]
pub(crate) fn Settings(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_SETTINGS_HEADING);
    let waiting = RwSignal::new(None::<MessageId>);
    let off = use_context::<RwSignal<Standing>>()
        .is_some_and(|standing| matches!(standing.get_untracked(), Standing::Off));
    if off {
        spawn_local(async move {
            let read = ws::session().await;
            waiting.set(waiting_password(read.as_ref()));
        });
    }
    let waiting_localizer = Arc::clone(&localizer);
    let restart_localizer = Arc::clone(&localizer);
    view! {
        <PageHeading>{heading}</PageHeading>
        {move || waiting.get().map(|id| view! { <p>{text(&waiting_localizer, id)}</p> })}
        <ApiTokens localizer=localizer />
        <RestartSection localizer=restart_localizer />
    }
}

/// What a person has typed and chosen in the form, one signal per part.
#[derive(Clone, Copy)]
struct Inputs {
    label: RwSignal<String>,
    scopes: [RwSignal<bool>; 4],
}

impl Inputs {
    fn new() -> Self {
        Self {
            label: RwSignal::new(String::new()),
            scopes: SCOPES.map(|_| RwSignal::new(false)),
        }
    }

    fn read(self) -> TokenForm {
        TokenForm {
            label: self.label.get_untracked(),
            scopes: SCOPES
                .into_iter()
                .zip(self.scopes)
                .filter(|(_, chosen)| chosen.get_untracked())
                .map(|(scope, _)| scope)
                .collect(),
        }
    }

    fn clear(self) {
        self.label.set(String::new());
        for chosen in self.scopes {
            chosen.set(false);
        }
    }
}

/// What is wrong with the form: the summary, and the error each part shows,
/// set together so they never disagree.
#[derive(Clone, Copy)]
struct Problems {
    summary: RwSignal<Vec<SummaryItem>>,
    label: RwSignal<Option<String>>,
    scopes: RwSignal<Option<String>>,
}

impl Problems {
    fn new() -> Self {
        Self {
            summary: RwSignal::new(Vec::new()),
            label: RwSignal::new(None),
            scopes: RwSignal::new(None),
        }
    }

    fn show(self, items: Vec<SummaryItem>) {
        self.label.set(error_of(&items, Field::Label));
        self.scopes.set(error_of(&items, Field::Scopes));
        self.summary.set(items);
    }
}

/// The API tokens' section: the list, the form that creates one, the dialog
/// that shows a new one's whole text once, and the dialog that asks before
/// one is revoked. With sign-in off the daemon refuses the list
/// (`sign-in-off`), and the section says why in its place, with no form.
#[component]
fn ApiTokens(localizer: Arc<Localizer>) -> impl IntoView {
    let base = StoredValue::new(ws::origin());
    let phase = RwSignal::new(Phase::Loading);
    // The tokens kept; `None` until the read answers, and while it is refused.
    let tokens = RwSignal::<Option<Vec<ApiTokenDto>>>::new(None);
    // A read the daemon refused (sign-in off, tokens it can't read): said
    // calmly in the list's place, as the daemon said it.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    let load_localizer = Arc::clone(&localizer);
    let load = Callback::new(move |()| {
        let url = format!("{}{TOKENS_PATH}", base.get_value());
        let localizer = Arc::clone(&load_localizer);
        spawn_local(async move {
            let read = ws::get::<ApiTokensDto>(&url).await;
            // The reader may have left the page before the answer came.
            if phase.try_get_untracked().is_none() {
                return;
            }
            match read {
                Some(Got::Read(read)) => {
                    refused.set(None);
                    tokens.set(Some(read.tokens));
                    phase.set(Phase::Ready);
                }
                Some(Got::Refused(code, args, detail)) => {
                    refused.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    tokens.set(None);
                    phase.set(Phase::Ready);
                }
                None => phase.set(Phase::Unreachable),
            }
        });
    });
    load.run(());
    // Whether the form is offered: only once the tokens could be read. A
    // memo, so a read again after a change doesn't rebuild the form.
    let readable = Memo::new(move |_| tokens.with(Option::is_some));

    // The new token's dialog: its label, and its whole text, held only
    // while the dialog is open.
    let created_label = RwSignal::new(String::new());
    let secret = RwSignal::<Option<String>>::new(None);
    let secret_open = RwSignal::new(false);
    let copy_said = RwSignal::<Option<String>>::new(None);
    Effect::new(move |_| {
        if !secret_open.get() {
            secret.set(None);
            copy_said.set(None);
        }
    });

    let inputs = Inputs::new();
    let problems = Problems::new();
    let busy = RwSignal::new(false);
    let submit_localizer = Arc::clone(&localizer);
    let submit = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let form = inputs.read();
        if let Some(items) = problems_of(&form, |id, args| text_with(&submit_localizer, id, args)) {
            problems.show(items);
            return;
        }
        busy.set(true);
        reply(
            Politeness::Polite,
            &text(&submit_localizer, msg::UI_API_TOKEN_CREATING),
        );
        let localizer = Arc::clone(&submit_localizer);
        let url = format!("{}{TOKENS_PATH}", base.get_value());
        spawn_local(async move {
            let posted = ws::post(&url, &form.request()).await;
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            match posted {
                Some(Posted::Done(answer)) => match created(&answer) {
                    Some(made) => {
                        problems.show(Vec::new());
                        inputs.clear();
                        created_label.set(made.token.label);
                        secret.set(Some(made.secret));
                        secret_open.set(true);
                        load.run(());
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => problems.show(vec![SummaryItem {
                    field: field_of(code).map(Field::id),
                    sentence: refused_sentence(&localizer, code, &args, detail.as_deref()),
                }]),
                None => phase.set(Phase::Unreachable),
            }
        });
    });

    // The revoke dialog: the token it is about, and what revoking came to.
    let revoking = RwSignal::<Option<ApiTokenDto>>::new(None);
    let revoke_open = RwSignal::new(false);
    let revoke_busy = RwSignal::new(false);
    let revoke_error = RwSignal::<Option<Sentence>>::new(None);
    let ask_revoke = Callback::new(move |token: ApiTokenDto| {
        revoke_error.set(None);
        revoking.set(Some(token));
        revoke_open.set(true);
    });
    let revoke_localizer = Arc::clone(&localizer);
    let revoke = Callback::new(move |token: ApiTokenDto| {
        if revoke_busy.get_untracked() {
            return;
        }
        revoke_busy.set(true);
        revoke_error.set(None);
        let localizer = Arc::clone(&revoke_localizer);
        let url = format!("{}{}", base.get_value(), revoke_path(&token.id));
        spawn_local(async move {
            let answer = ws::delete(&url).await;
            if revoke_busy.try_get_untracked().is_none() {
                return;
            }
            revoke_busy.set(false);
            match answer {
                Some(Posted::Done(_)) => {
                    revoke_open.set(false);
                    reply(Politeness::Polite, &say(&localizer, revoked(&token.label)));
                    load.run(());
                }
                Some(Posted::Refused(code, args, detail)) => revoke_error.set(Some(
                    refused_sentence(&localizer, code, &args, detail.as_deref()),
                )),
                None => revoke_error.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_SESSION_UNREACHABLE),
                    None,
                ))),
            }
        });
    });

    let heading = text(&localizer, msg::UI_API_TOKENS_HEADING);
    let body = text(&localizer, msg::UI_API_TOKENS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let none = text(&localizer, msg::UI_API_TOKENS_NONE);
    let list_localizer = Arc::clone(&localizer);
    let form_localizer = Arc::clone(&localizer);
    let revoke_dialog_localizer = Arc::clone(&localizer);
    let viewer = l10n::viewer_zone();
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            <p>{body}</p>
            {move || match phase.get() {
                Phase::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
                Phase::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
                Phase::Ready => {
                    match (refused.get(), tokens.get()) {
                        (Some(sentence), _) => view! { <p>{sentence_view(sentence)}</p> }.into_any(),
                        (None, Some(list)) if !list.is_empty() => {
                            tokens_table(&list_localizer, list, ask_revoke, viewer.as_ref())
                                .into_any()
                        }
                        _ => view! { <p>{none.clone()}</p> }.into_any(),
                    }
                }
            }}
            {move || {
                readable
                    .get()
                    .then(|| {
                        view! {
                            <CreateForm
                                localizer=Arc::clone(&form_localizer)
                                inputs=inputs
                                problems=problems
                                busy=busy
                                submit=submit
                            />
                        }
                    })
            }}
            <CreatedDialog
                localizer=Arc::clone(&localizer)
                open=secret_open
                label=created_label
                secret=secret
                said=copy_said
            />
            {move || {
                revoking
                    .get()
                    .map(|token| {
                        view! {
                            <RevokeDialog
                                localizer=Arc::clone(&revoke_dialog_localizer)
                                token=token
                                open=revoke_open
                                busy=revoke_busy
                                error=revoke_error
                                revoke=revoke
                            />
                        }
                    })
            }}
        </section>
    }
}

/// The table of the tokens kept, named by the section's heading.
fn tokens_table(
    localizer: &Localizer,
    tokens: Vec<ApiTokenDto>,
    ask_revoke: Callback<ApiTokenDto>,
    viewer: Option<&IanaZone>,
) -> impl IntoView + use<> {
    let now_ms = ws::now_ms();
    let rows = tokens
        .into_iter()
        .map(|token| token_row(localizer, token, ask_revoke, now_ms, viewer))
        .collect_view();
    view! {
        <table class="api-tokens" aria-labelledby=HEADING_ID>
            <thead>
                <tr>
                    <th scope="col">{text(localizer, msg::UI_API_TOKENS_COLUMN_LABEL)}</th>
                    <th scope="col">{text(localizer, msg::UI_API_TOKENS_COLUMN_SCOPES)}</th>
                    <th scope="col">{text(localizer, msg::UI_API_TOKENS_COLUMN_CREATED)}</th>
                    <th scope="col">{text(localizer, msg::UI_API_TOKENS_COLUMN_LAST_USED)}</th>
                    <th scope="col">{text(localizer, msg::UI_COLUMN_ACTIONS)}</th>
                </tr>
            </thead>
            <tbody>{rows}</tbody>
        </table>
    }
}

/// A token's row: its label as the row's header, its scopes (one said as
/// it is, several as a list, #430), when it was created and last used, and
/// its Revoke button, named for it. The button hands focus to the section's
/// heading when its row goes while it holds focus, as after a revoke.
fn token_row(
    localizer: &Localizer,
    token: ApiTokenDto,
    ask_revoke: Callback<ApiTokenDto>,
    now_ms: i64,
    viewer: Option<&IanaZone>,
) -> impl IntoView + use<> {
    let scopes = scopes_view(localizer, &token.scopes);
    let created_at = instant(localizer, &token.created_at, viewer);
    let last_used = match LastUsed::of(token.last_used_at.as_deref(), now_ms).said() {
        Ok(said) => say(localizer, said),
        Err(raw) => raw,
    };
    let last_used = match token.last_used_at.clone() {
        Some(raw) => view! { <time datetime=raw>{last_used}</time> }.into_any(),
        None => last_used.into_any(),
    };
    let label = say(localizer, revoke_label(&token.label));
    let button = NodeRef::<leptos::html::Button>::new();
    hand_focus_when_gone(button, HEADING_ID.to_owned());
    let id = revoke_id(&token.id);
    let header = token.label.clone();
    let created_raw = token.created_at.clone();
    view! {
        <tr>
            <th scope="row">{header}</th>
            <td>{scopes}</td>
            <td>
                <time datetime=created_raw>{created_at}</time>
            </td>
            <td>{last_used}</td>
            <td>
                <button
                    type="button"
                    id=id
                    node_ref=button
                    on:click=move |_| ask_revoke.run(token.clone())
                >
                    {label}
                </button>
            </td>
        </tr>
    }
}

/// A token's scopes in its row: one said as it is, several as a list.
fn scopes_view(localizer: &Localizer, scopes: &[ApiTokenScopeDto]) -> AnyView {
    let names: Vec<String> = scopes
        .iter()
        .map(|scope| say(localizer, scope_name(*scope)))
        .collect();
    match names.as_slice() {
        [one] => one.clone().into_any(),
        _ => view! { <ul>{names.into_iter().map(|name| view! { <li>{name}</li> }).collect_view()}</ul> }
        .into_any(),
    }
}

/// The form that creates a token: its label, its scopes (a group of
/// checkboxes, each with a hint naming what it allows), and the error
/// summary that takes focus when the form is refused.
#[component]
fn CreateForm(
    localizer: Arc<Localizer>,
    inputs: Inputs,
    problems: Problems,
    busy: RwSignal<bool>,
    submit: Callback<()>,
) -> impl IntoView {
    let summary_localizer = Arc::clone(&localizer);
    let field_text = |id| text(&localizer, id);
    let label_hint = text_with(
        &localizer,
        msg::UI_API_TOKEN_LABEL_HINT,
        &[("max", MsgArg::Count(LABEL_MAX_CHARS))],
    );
    let creating = text(&localizer, msg::UI_API_TOKEN_CREATING);
    let checkboxes = SCOPES
        .into_iter()
        .zip(inputs.scopes)
        .map(|(scope, checked)| {
            view! {
                <Checkbox
                    id=scope_id(scope)
                    label=say(&localizer, scope_name(scope))
                    description=say(&localizer, scope_hint(scope))
                    checked=checked
                />
            }
        })
        .collect_view();
    view! {
        <section aria-labelledby="api-token-form-heading">
            <h3 id="api-token-form-heading">{field_text(msg::UI_API_TOKEN_FORM_HEADING)}</h3>
            {move || {
                let items = problems.summary.get();
                (!items.is_empty())
                    .then(|| {
                        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                        let heading = text_with(
                            &summary_localizer,
                            msg::UI_API_TOKEN_PROBLEMS,
                            &[("count", MsgArg::Count(count))],
                        );
                        view! { <ErrorSummary heading=heading items=items advice=None /> }
                    })
            }}
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    submit.run(());
                }
            >
                <FormField
                    id=Field::Label.id()
                    label=field_text(msg::UI_API_TOKEN_LABEL_LABEL)
                    description=label_hint
                    error=problems.label
                    value=inputs.label
                />
                <fieldset
                    class="choices"
                    aria-describedby=move || {
                        if problems.scopes.get().is_some() {
                            format!("{SCOPES_HINT_ID} {SCOPES_ERROR_ID}")
                        } else {
                            SCOPES_HINT_ID.to_owned()
                        }
                    }
                >
                    <legend>{field_text(msg::UI_API_TOKEN_SCOPES_LEGEND)}</legend>
                    <p id=SCOPES_HINT_ID>{field_text(msg::UI_API_TOKEN_SCOPES_HINT)}</p>
                    {move || {
                        problems
                            .scopes
                            .get()
                            .map(|error| {
                                view! {
                                    <p id=SCOPES_ERROR_ID class="form-error">
                                        {error}
                                    </p>
                                }
                            })
                    }}
                    {checkboxes}
                </fieldset>
                <p class="actions">
                    <button type="submit" id=CREATE_ID>
                        {field_text(msg::UI_API_TOKEN_CREATE)}
                    </button>
                </p>
            </form>
            {move || busy.get().then(|| view! { <p>{creating.clone()}</p> })}
        </section>
    }
}

/// The dialog that shows a new token's whole text, the only time it is
/// shown: it says so, holds the text in a field that can be selected by
/// hand, and starts on the button that copies it, so a stray Enter copies
/// rather than throwing the text away. Leaving it clears the text from the
/// page (the section's effect on `open`), and focus returns to the button
/// that created the token.
#[component]
fn CreatedDialog(
    localizer: Arc<Localizer>,
    open: RwSignal<bool>,
    label: RwSignal<String>,
    secret: RwSignal<Option<String>>,
    said: RwSignal<Option<String>>,
) -> impl IntoView {
    let copy_button = NodeRef::<leptos::html::Button>::new();
    let copied = text(&localizer, msg::UI_API_TOKEN_COPIED);
    let refused = text(&localizer, msg::UI_API_TOKEN_COPY_FAILED);
    let body_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=open
            title=text(&localizer, msg::UI_API_TOKEN_CREATED_TITLE)
            dismiss_label=text(&localizer, msg::UI_REVIEW_CLOSE)
            confirm=None
            returns_to=CREATE_ID.to_owned()
            described_by=CREATED_BODY_ID.to_owned()
            fallback=HEADING_ID.to_owned()
            starts_on=copy_button
        >
            <p id=CREATED_BODY_ID>{move || say(&body_localizer, created_body(&label.get()))}</p>
            <div class="field">
                <label for=SECRET_ID>{text(&localizer, msg::UI_API_TOKEN_CREATED_FIELD)}</label>
                <input
                    id=SECRET_ID
                    class="token"
                    type="text"
                    readonly
                    autocomplete="off"
                    spellcheck="false"
                    prop:value=move || secret.get().unwrap_or_default()
                />
            </div>
            <p class="actions">
                <button
                    type="button"
                    node_ref=copy_button
                    on:click=move |_| {
                        if let Some(token) = secret.get_untracked() {
                            copy(&token, said, copied.clone(), refused.clone());
                        }
                    }
                >
                    {text(&localizer, msg::UI_API_TOKEN_COPY)}
                </button>
            </p>
            <p aria-live="polite" aria-atomic="true">
                {move || said.get()}
            </p>
        </ReviewDialog>
    }
}

/// Writes `token` to the clipboard, then says in the dialog's own polite
/// region (the announcer waits while a modal is open) whether it is there.
/// A page not served over HTTPS or from this computer has no clipboard, and
/// a browser may refuse it: then it says to copy the token by hand.
fn copy(token: &str, said: RwSignal<Option<String>>, copied: String, refused: String) {
    let clipboard = web_sys::window()
        .and_then(|window| {
            js_sys::Reflect::get(window.navigator().as_ref(), &"clipboard".into()).ok()
        })
        .and_then(|clipboard| clipboard.dyn_into::<web_sys::Clipboard>().ok());
    let Some(clipboard) = clipboard else {
        said.set(Some(refused));
        return;
    };
    said.set(None);
    let done = Closure::once(move |_: JsValue| {
        said.try_set(Some(copied));
    });
    let failed = Closure::once(move |_: JsValue| {
        said.try_set(Some(refused));
    });
    let _ = clipboard.write_text(token).then2(&done, &failed);
    // The promise calls one of them once, after this returns; each is a few
    // bytes, left to the page rather than dropped before it is called.
    done.forget();
    failed.forget();
}

/// The dialog that asks before the token is revoked, named for it: Cancel
/// first, so a stray Enter backs out, and the confirm button says what it
/// commits to. While the daemon revokes it, the dialog holds still and says
/// so; a refusal is an alert inside it.
#[component]
fn RevokeDialog(
    localizer: Arc<Localizer>,
    token: ApiTokenDto,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<Sentence>>,
    revoke: Callback<ApiTokenDto>,
) -> impl IntoView {
    let title = say(&localizer, revoke_title(&token.label));
    let returns_to = revoke_id(&token.id);
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_API_TOKEN_REVOKE_CONFIRM),
        action: Callback::new(move |()| revoke.run(token.clone())),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_API_TOKEN_REVOKING),
    };
    view! {
        <ReviewDialog
            open=open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=returns_to
            described_by=REVOKE_BODY_ID.to_owned()
            working=working
            fallback=HEADING_ID.to_owned()
        >
            <p id=REVOKE_BODY_ID>{text(&localizer, msg::UI_API_TOKEN_REVOKE_BODY)}</p>
            {move || {
                error.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
            }}
        </ReviewDialog>
    }
}

/// What the restart section holds: what it shows, whether the wait has
/// lasted too long, and which turn of the section a poll belongs to, so a
/// poll of a stage that has since changed stops.
#[derive(Clone, Copy)]
struct RestartState {
    base: StoredValue<String>,
    stage: RwSignal<Stage>,
    not_back: RwSignal<bool>,
    turn: StoredValue<u32>,
}

impl RestartState {
    fn url(self) -> String {
        format!("{}{RESTART_PATH}", self.base.get_value())
    }

    /// Shows `next`, and starts what it waits for: a read again each
    /// [`PENDING_EVERY`] while a restart is pending, and the wait for the
    /// daemon to come back once it restarts.
    fn enter(self, next: Stage) {
        let turn = self.turn.get_value().wrapping_add(1);
        self.turn.set_value(turn);
        self.stage.set(next);
        match next {
            Stage::Pending => spawn_local(self.watch_pending(turn)),
            Stage::Restarting { seen_down } => spawn_local(self.await_daemon(turn, seen_down)),
            Stage::Loading | Stage::Unreachable | Stage::Unavailable | Stage::Offered { .. } => {}
        }
    }

    /// Whether `turn` is still the section's, on a page still open.
    fn current(self, turn: u32) -> bool {
        self.turn.try_get_value() == Some(turn)
    }

    async fn watch_pending(self, turn: u32) {
        let url = self.url();
        loop {
            gloo_timers::future::sleep(PENDING_EVERY).await;
            if !self.current(turn) {
                return;
            }
            let read = ws::poll::<RestartDto>(&url, ANSWER_PATIENCE).await;
            if !self.current(turn) {
                return;
            }
            let next = Stage::while_pending(read.as_ref());
            if next != Stage::Pending {
                self.enter(next);
                return;
            }
        }
    }

    async fn await_daemon(self, turn: u32, seen_down: bool) {
        let mut wait = Wait::new(ws::now_ms(), seen_down);
        loop {
            let answered = ws::answers(ANSWER_PATIENCE).await;
            if !self.current(turn) {
                return;
            }
            match wait.after(answered, ws::now_ms()) {
                Waited::Reload => {
                    if let Some(window) = web_sys::window() {
                        let _ = window.location().reload();
                    }
                    return;
                }
                Waited::Again { not_back } => {
                    self.not_back.set(not_back);
                    gloo_timers::future::sleep(WAIT_EVERY).await;
                }
            }
        }
    }
}

/// The restart section (decision NN): offered only when a service manager
/// runs the daemon, and otherwise says why not. Its button reads how many
/// operations run and opens the dialog that asks. While a restart waits for
/// them it says so and offers to cancel it; once the daemon restarts it says
/// so in its status, and the page reloads once the daemon has gone down and
/// answers again (`restart.rs` decides each step).
#[component]
fn RestartSection(localizer: Arc<Localizer>) -> impl IntoView {
    let section = RestartState {
        base: StoredValue::new(ws::origin()),
        stage: RwSignal::new(Stage::Loading),
        not_back: RwSignal::new(false),
        turn: StoredValue::new(0),
    };
    let read = move || async move {
        match ws::get::<RestartDto>(&section.url()).await {
            Some(Got::Read(told)) => Stage::of(Some(&told)),
            Some(Got::Refused(..)) | None => Stage::of(None),
        }
    };
    spawn_local(async move {
        let next = read().await;
        if section.stage.try_get_untracked().is_some() {
            section.enter(next);
        }
    });

    // The dialog: the running count it is about, and what asking came to.
    let asking = RwSignal::<Option<u32>>::new(None);
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::<Option<Sentence>>::new(None);
    let offer = Callback::new(move |()| {
        spawn_local(async move {
            match read().await {
                Stage::Offered { running } => {
                    error.try_set(None);
                    asking.try_set(Some(running));
                    open.try_set(true);
                }
                next if section.stage.try_get_untracked().is_some() => section.enter(next),
                _ => {}
            }
        });
    });
    let ask_localizer = Arc::clone(&localizer);
    let ask = Callback::new(move |when: RestartWhenDto| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let localizer = Arc::clone(&ask_localizer);
        let url = section.url();
        spawn_local(async move {
            let posted = ws::post(&url, &request(when)).await;
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            let told = match posted {
                Some(Posted::Done(answer)) => serde_json::from_str::<RestartDto>(&answer).ok(),
                Some(Posted::Refused(code, args, detail)) => {
                    error.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    return;
                }
                None => None,
            };
            match told {
                Some(told) => {
                    open.set(false);
                    section.enter(Stage::after_ask(when, &told));
                }
                None => error.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_SESSION_UNREACHABLE),
                    None,
                ))),
            }
        });
    });
    let cancel_localizer = Arc::clone(&localizer);
    let cancel = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let localizer = Arc::clone(&cancel_localizer);
        let url = section.url();
        spawn_local(async move {
            let answer = ws::delete(&url).await;
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            match answer {
                Some(Posted::Done(answer)) => {
                    let told = serde_json::from_str::<RestartDto>(&answer).ok();
                    section.enter(Stage::of(told.as_ref()));
                }
                Some(Posted::Refused(code, args, detail)) => error.set(Some(refused_sentence(
                    &localizer,
                    code,
                    &args,
                    detail.as_deref(),
                ))),
                // No answer while a restart waits is the daemon going down
                // for it.
                None => section.enter(Stage::while_pending(None)),
            }
        });
    });

    // The status shows in the section and reaches screen readers through
    // the page's one polite region (§4.5), each time it says something new.
    let status = Memo::new(move |_| status_of(section.stage.get(), section.not_back.get()));
    let announce_localizer = Arc::clone(&localizer);
    Effect::new(move |_| {
        if let Some(id) = status.get() {
            reply(Politeness::Polite, &text(&announce_localizer, id));
        }
    });
    let heading = text(&localizer, msg::UI_RESTART_HEADING);
    let stage_localizer = Arc::clone(&localizer);
    let status_localizer = Arc::clone(&localizer);
    let dialog_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=restart::HEADING_ID>
            <h2 id=restart::HEADING_ID tabindex="-1">
                {heading}
            </h2>
            {move || {
                let localizer = &stage_localizer;
                match section.stage.get() {
                    Stage::Loading => view! { <p>{text(localizer, msg::UI_LOADING)}</p> }.into_any(),
                    Stage::Unreachable => {
                        view! { <p>{text(localizer, msg::UI_UNREACHABLE_BODY)}</p> }.into_any()
                    }
                    Stage::Unavailable => {
                        view! { <p>{text(localizer, msg::UI_RESTART_UNAVAILABLE)}</p> }.into_any()
                    }
                    Stage::Offered { .. } => {
                        let button = NodeRef::<leptos::html::Button>::new();
                        hand_focus_when_gone(button, restart::HEADING_ID.to_owned());
                        view! {
                            <p>{text(localizer, msg::UI_RESTART_BODY)}</p>
                            <p>
                                <button
                                    type="button"
                                    id=restart::BUTTON_ID
                                    node_ref=button
                                    on:click=move |_| offer.run(())
                                >
                                    {text(localizer, msg::UI_RESTART_BUTTON)}
                                </button>
                            </p>
                        }
                            .into_any()
                    }
                    Stage::Pending => {
                        let button = NodeRef::<leptos::html::Button>::new();
                        hand_focus_when_gone(button, restart::HEADING_ID.to_owned());
                        view! {
                            <p>
                                <button
                                    type="button"
                                    node_ref=button
                                    aria-disabled=move || busy.get().then_some("true")
                                    on:click=move |_| cancel.run(())
                                >
                                    {text(localizer, msg::UI_RESTART_CANCEL)}
                                </button>
                            </p>
                        }
                            .into_any()
                    }
                    Stage::Restarting { .. } => ().into_any(),
                }
            }}
            <p>{move || status.get().map(|id| text(&status_localizer, id))}</p>
            {move || {
                (section.stage.get() == Stage::Pending)
                    .then(|| error.get())
                    .flatten()
                    .map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
            }}
            {move || {
                asking
                    .get()
                    .map(|running| {
                        view! {
                            <RestartDialog
                                localizer=Arc::clone(&dialog_localizer)
                                running=running
                                open=open
                                busy=busy
                                error=error
                                ask=ask
                            />
                        }
                    })
            }}
        </section>
    }
}

/// The dialog that asks before Ladderback restarts: what becomes of the
/// running and the queued operations, Cancel first, so a stray Enter backs
/// out, then a restart once they finish (a plain restart with none
/// running), and a restart now after it while some run. While the daemon
/// takes the request, the dialog holds still and says so; a refusal is an
/// alert inside it.
#[component]
fn RestartDialog(
    localizer: Arc<Localizer>,
    running: u32,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<Sentence>>,
    ask: Callback<RestartWhenDto>,
) -> impl IntoView {
    let confirm = Confirmation {
        label: say(&localizer, confirm_said(running)),
        action: Callback::new(move |()| ask.run(RestartWhenDto::Finished)),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_RESTART_ASKING),
    };
    let now = offers_now(running).then(|| Confirmation {
        label: text(&localizer, msg::UI_RESTART_NOW),
        action: Callback::new(move |()| ask.run(RestartWhenDto::Now)),
    });
    view! {
        <ReviewDialog
            open=open
            title=text(&localizer, msg::UI_RESTART_TITLE)
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            also=now
            returns_to=restart::BUTTON_ID.to_owned()
            described_by=restart::BODY_ID.to_owned()
            working=working
            fallback=restart::HEADING_ID.to_owned()
        >
            <div id=restart::BODY_ID>
                <p>{say(&localizer, running_said(running))}</p>
                <p>{text(&localizer, msg::UI_RESTART_QUEUED)}</p>
            </div>
            {move || {
                error.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
            }}
        </ReviewDialog>
    }
}
