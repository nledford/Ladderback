//! The Settings page, the route `/settings` (§4.4), a tab for each section:
//! the language of the web UI (§8.4, decision 101), kept in this browser,
//! whose glue is `settings/language.rs`; the width of its pages (§4.5), kept
//! in this browser too, whose glue is `settings/page_width.rs`; its admin
//! password (security §10.1, decision 70), set with sign-in off and changed
//! in a session, whose glue is `settings/password.rs`; its API tokens
//! (decisions 93 and 127), listed, created, changed and revoked in a
//! session; what AI agents may do without a token (decision 127), whose
//! glue is `settings/ai_agents.rs`; the dry-run allowlist (decision 89),
//! whose glue is `settings/dry_run_allowlist.rs`; and the restart of
//! Ladderback (decision 94). The tabs, the address that opens each, the tab
//! an address changed while the page is open chooses and what each panel
//! holds are `settings_tabs.rs`'s; what the token form lacks, the requests
//! it sends, which field a refusal is about, each permission's group and
//! how a token's row reads are `api_tokens.rs`'s, and what the restart
//! section offers and waits for is `restart.rs`'s, all host-tested; this is
//! the DOM glue, which the `@ui` scenarios of `features/ui/settings.feature`,
//! `features/security/api_tokens.feature`,
//! `features/automation/ai_agents_settings.feature` and
//! `features/execution/restart_from_settings.feature` run in Chromium.
//!
//! A new token's whole text is shown once, in a dialog, and held in one
//! signal only while that dialog is open: leaving the dialog, however it is
//! left, clears it from the page (I4).

mod ai_agents;
// The Agents section's DOM glue, on a hub; `pairing.rs` decides for it.
#[cfg(feature = "hub")]
pub(crate) mod agents;
mod dry_run_allowlist;
mod language;
// The Machines tab's waiting and approved machines, on a hub.
#[cfg(feature = "hub")]
mod machines;
mod page_width;
mod password;
mod sign_in;

use ai_agents::AiAgentsSection;
use dry_run_allowlist::DryRunAllowlistSection;
use language::LanguageSection;
use page_width::PageWidthSection;
use password::AdminPassword;
use sign_in::SignInSection;

use std::rc::Rc;
use std::sync::Arc;

use api::{
    ApiTokenDto, ApiTokensDto, ErrorCode, IanaZone, MsgArg, PermissionsDto, RestartDto,
    RestartWhenDto,
};
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};
use leptos_router::hooks::use_url;

use crate::announcer::Politeness;
use crate::api_tokens::{
    CREATE_ID, CREATED_BODY_ID, Field, Form, HEADING_ID, LABEL_MAX_CHARS, LastUsed, ListReads,
    REVOKE_BODY_ID, ROWS, Said, TOKENS_PATH, TokenForm, change_id, change_label, change_path,
    change_request, change_title, changed, chosen_of, created, created_body, error_of, field_of,
    granted_of, new_token, permissions_from, problems_of, revoke_id, revoke_label, revoke_path,
    revoke_title, revoked,
};
use crate::components::ACTIONS;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::skeleton::rules::Awaited;
use crate::components::tabs::{Orientation, TabSet, TabsName};
use crate::components::{
    Banner, Button, Column, ColumnsChoice, Confirmation, ErrorSummary, Name, RadioGroup,
    ReviewDialog, Sentences, State, StepUp, StepUpField, SummaryItem, Tab, Table, Tabs, TextField,
    Toast, ToastTone, Toasts, Working, copy_text, dialog_refusal_alert, hand_focus_when_gone,
    refusal_alert, say_refusals, text_field,
};
use crate::components::{first_load, paced};
use crate::live::reply;
use crate::page_width::PageWidth;
use crate::repositories::Sentence;
use crate::restart::{
    self, ANSWER_PATIENCE, PENDING_EVERY, RESTART_PATH, Stage, WAIT_EVERY, Wait, Waited,
    confirm_said, offers_now, request, running_said, status_of,
};
use crate::routes::{PAGE_HEADING_ID, PageHeading, instant, refused_sentence, text_with};
use crate::session::{Standing, waiting_password, waiting_tone};
use crate::settings_tabs::{Section, tab_named};
use crate::variants::{Rem, Size, Tone};
use crate::ws::{self, Got, Phase, Posted};
use crate::{page, text};

/// The text of `said` in the page's language.
fn say(localizer: &Localizer, (id, args): Said) -> String {
    text_with(localizer, id, &args)
}

/// The `id` of the permissions' hint, which describes their group.
const PERMISSIONS_HINT_ID: &str = "api-token-permissions-hint";

/// The `id` of the permissions' error, which describes their group while it
/// shows.
const PERMISSIONS_ERROR_ID: &str = "api-token-permissions-error";

/// The `id` of what the dialog that changes a token's permissions says,
/// which describes it.
const CHANGE_BODY_ID: &str = "api-token-change-body";

/// The `id` of the admin password's field in that dialog.
const CHANGE_PASSWORD_ID: &str = "api-token-change-password";

/// The `id` of the field that holds a new token's whole text.
const SECRET_ID: &str = "api-token-secret";

/// The Settings page: a tab for each section (`settings_tabs.rs`), a column
/// beside the section where the page has room and a row above it where it
/// hasn't, starting on the one the page's address names, else the first.
/// Choosing a tab gives the address that tab's name, so loading the page
/// again comes back to it. With sign-in off, it asks the daemon on opening
/// whether an admin password has been set since it started, which waits for
/// a restart to turn sign-in on (decision 70), and says so above the tabs.
/// The admin password's and the signing-in sections are offered with
/// sign-in off and in a session; with the daemon unread, the page can't say
/// which form they would be.
///
/// On a hub, its Machines tab shows when the page has read the hub's key,
/// so the tabs are drawn once that read has answered or waited its while,
/// and only once: a later read never draws them again, which would lose
/// what their panels hold, as an API token's one-time secret (decision
/// 121).
#[component]
pub(crate) fn Settings(localizer: Arc<Localizer>) -> impl IntoView {
    #[cfg(feature = "hub")]
    let hub = agents::hub_known();
    #[cfg(not(feature = "hub"))]
    let hub = || Some(false);
    let drawn = Memo::new(move |was: Option<&Option<bool>>| was.copied().flatten().or_else(hub));
    move || {
        drawn
            .get()
            .map(|hub| view! { <SettingsTabs localizer=Arc::clone(&localizer) hub=hub /> })
    }
}

/// Settings' heading and tabs, the Agents tab among them when `hub`.
#[component]
fn SettingsTabs(localizer: Arc<Localizer>, hub: bool) -> impl IntoView {
    let heading = text(&localizer, msg::UI_SETTINGS_HEADING);
    let waiting = RwSignal::new(None::<MessageId>);
    let standing = use_context::<RwSignal<Standing>>().map(|standing| standing.get_untracked());
    let off = matches!(standing, Some(Standing::Off));
    let signed_in = matches!(standing, Some(Standing::SignedIn));
    if off {
        spawn_local(async move {
            let read = ws::session().await;
            waiting.set(waiting_password(read.as_ref()));
        });
    }
    let mut sections = Vec::new();
    if let Some(width) = use_context::<RwSignal<PageWidth>>() {
        let panel = view! { <PageWidthSection localizer=Arc::clone(&localizer) width=width /> };
        sections.push((
            Section::PageWidth,
            msg::UI_PAGE_WIDTH_HEADING,
            panel.into_any(),
        ));
    }
    if matches!(standing, Some(Standing::Off | Standing::SignedIn)) {
        let password =
            view! { <AdminPassword localizer=Arc::clone(&localizer) on=!off waiting=waiting /> };
        let signing_in = view! { <SignInSection localizer=Arc::clone(&localizer) on=!off /> };
        sections.push((
            Section::AdminPassword,
            msg::UI_ADMIN_PASSWORD_HEADING,
            password.into_any(),
        ));
        sections.push((
            Section::SigningIn,
            msg::UI_SIGNING_IN_HEADING,
            signing_in.into_any(),
        ));
    }
    let tokens = view! { <ApiTokens localizer=Arc::clone(&localizer) /> };
    sections.push((
        Section::ApiTokens,
        msg::UI_API_TOKENS_HEADING,
        tokens.into_any(),
    ));
    // Whether the daemon has read what AI agents may do, so the AI agents'
    // panel holds its choices and its first choice is the stop after its tab.
    let ai_agents_read = RwSignal::new(false);
    let ai_agents =
        view! { <AiAgentsSection localizer=Arc::clone(&localizer) on=!off read=ai_agents_read /> };
    sections.push((
        Section::AiAgents,
        msg::UI_AI_AGENTS_TAB,
        ai_agents.into_any(),
    ));
    #[cfg(feature = "hub")]
    if hub {
        let agents = view! { <agents::Agents localizer=Arc::clone(&localizer) on=signed_in /> };
        sections.push((Section::Agents, msg::UI_AGENTS_HEADING, agents.into_any()));
    }
    #[cfg(not(feature = "hub"))]
    let _ = hub;
    let restart = view! { <RestartSection localizer=Arc::clone(&localizer) /> };
    let allowlist = view! { <DryRunAllowlistSection localizer=Arc::clone(&localizer) /> };
    sections.push((
        Section::DryRunAllowlist,
        msg::UI_DRY_RUN_ALLOWLIST_HEADING,
        allowlist.into_any(),
    ));
    sections.push((
        Section::Restart,
        msg::UI_RESTART_HEADING,
        restart.into_any(),
    ));

    let named = Section::named(&page::address_fragment());
    let tab = |section: Section, words: MessageId, panel: AnyView| {
        let holds = Signal::derive(move || section.holds(signed_in, ai_agents_read.get()));
        Tab::new(text(&localizer, words), holds, panel).selected_at_start(named == Some(section))
    };
    let language = view! { <LanguageSection localizer=Arc::clone(&localizer) /> };
    let first = tab(
        Section::Language,
        msg::UI_LANGUAGE_HEADING,
        language.into_any(),
    );
    let shown: Rc<[Section]> = std::iter::once(Section::Language)
        .chain(sections.iter().map(|(section, _, _)| *section))
        .collect();
    let rest = sections
        .into_iter()
        .map(|(section, words, panel)| tab(section, words, panel))
        .collect();
    let selected = RwSignal::new(0_usize);
    // The address changed while the page is open, its fragment alone, as
    // one typed in the address bar does: the tab it names is chosen, focus
    // left where it is, and one that names no tab shown leaves the tab as it
    // is.
    let tabs_shown = Rc::clone(&shown);
    let fragment_changed = window_event_listener(ev::hashchange, move |_| {
        if let Some(index) = tab_named(&page::address_fragment(), &tabs_shown) {
            selected.set(index);
        }
    });
    on_cleanup(move || fragment_changed.remove());
    // A link on Settings to a tab of Settings, such as the network's notice
    // that sign-in is off (#1373), changes the address through the router,
    // which fires no `hashchange` and keeps the page as it is: the router's
    // address says so, and the tab it names is chosen as above. Only while
    // the address is still Settings': a link away leaves the tabs alone.
    let url = use_url();
    let settings_path = url.with_untracked(|url| url.path().to_owned());
    let tabs_linked = Rc::clone(&shown);
    Effect::new(move |seen: Option<()>| {
        let here = url.with(|url| url.path() == settings_path);
        if seen.is_some()
            && here
            && let Some(index) = tab_named(&page::address_fragment(), &tabs_linked)
        {
            selected.set(index);
        }
    });
    // A tab chosen names its section in the address; the tab the page
    // starts on is named there already, or is the first, which the address
    // needn't name. A tab the address chose is named there already too, and
    // naming it again changes nothing.
    Effect::new(move |seen: Option<usize>| {
        let now = selected.get();
        if seen.is_some_and(|seen| seen != now)
            && let Some(section) = shown.get(now)
        {
            page::name_section(section.fragment());
        }
        now
    });
    let waiting_localizer = Arc::clone(&localizer);
    view! {
        <PageHeading>{heading}</PageHeading>
        {move || {
            waiting
                .get()
                .map(|id| {
                    let words = text(&waiting_localizer, id);
                    view! {
                        <Banner localizer=Arc::clone(&waiting_localizer) tone=waiting_tone(id)>
                            {words}
                        </Banner>
                    }
                })
        }}
        <Tabs
            id="settings"
            name=TabsName::LabelledBy(PAGE_HEADING_ID.to_owned())
            orientation=Orientation::Responsive
            tabs=TabSet::new(first, rest)
            selected=selected
        />
    }
}

/// The value each permission's group holds, one signal per group, in
/// [`ROWS`]' order: a token's form, its change, or what AI agents may do
/// without a token.
#[derive(Clone, Copy)]
struct Chosen([RwSignal<&'static str>; 7]);

impl Chosen {
    fn new(permissions: &PermissionsDto) -> Self {
        Self(chosen_of(permissions).map(RwSignal::new))
    }

    /// Makes each group hold what `permissions` grant.
    fn set(self, permissions: &PermissionsDto) {
        for (group, value) in self.0.into_iter().zip(chosen_of(permissions)) {
            group.set(value);
        }
    }

    /// The permissions the groups hold.
    fn read(self) -> PermissionsDto {
        permissions_from(self.0.map(|group| group.get_untracked()))
    }
}

/// One radio group per permission (decision 127), named for it, with a
/// hint saying what it opens, and its two choices: no access, or its level.
#[component]
fn PermissionGroups(localizer: Arc<Localizer>, form: Form, chosen: Chosen) -> impl IntoView {
    ROWS.into_iter()
        .zip(chosen.0)
        .map(|(row, value)| {
            let options = row
                .choices()
                .into_iter()
                .map(|(code, said)| (code, say(&localizer, said)))
                .collect();
            view! {
                <RadioGroup
                    name=row.group(form)
                    legend=say(&localizer, row.name())
                    description=say(&localizer, row.hint())
                    options=options
                    chosen=value
                />
            }
        })
        .collect_view()
}

/// What a person has typed and chosen in the form, one signal per part.
#[derive(Clone, Copy)]
struct Inputs {
    label: RwSignal<String>,
    permissions: Chosen,
}

impl Inputs {
    /// An empty label, and a new token's permissions (decision 127).
    fn new() -> Self {
        Self {
            label: RwSignal::new(String::new()),
            permissions: Chosen::new(&new_token()),
        }
    }

    fn read(self) -> TokenForm {
        TokenForm {
            label: self.label.get_untracked(),
            permissions: self.permissions.read(),
        }
    }

    fn clear(self) {
        self.label.set(String::new());
        self.permissions.set(&new_token());
    }
}

/// What is wrong with the form: the summary, and the error each part shows,
/// set together so they never disagree.
#[derive(Clone, Copy)]
struct Problems {
    summary: RwSignal<Vec<SummaryItem>>,
    label: RwSignal<Option<String>>,
    permissions: RwSignal<Option<String>>,
}

impl Problems {
    fn new() -> Self {
        Self {
            summary: RwSignal::new(Vec::new()),
            label: RwSignal::new(None),
            permissions: RwSignal::new(None),
        }
    }

    fn show(self, items: Vec<SummaryItem>) {
        self.label.set(error_of(&items, Field::Label));
        self.permissions.set(error_of(&items, Field::Permissions));
        self.summary.set(items);
    }
}

/// What the dialog that changes a token's permissions holds: the token it
/// is about, its groups, the admin password typed and its error, a refusal
/// that is about no field, and whether the daemon is saving.
#[derive(Clone, Copy)]
struct Change {
    token: RwSignal<Option<ApiTokenDto>>,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    chosen: Chosen,
    password: RwSignal<String>,
    password_error: RwSignal<Option<String>>,
    error: RwSignal<Option<Sentence>>,
}

impl Change {
    fn new() -> Self {
        Self {
            token: RwSignal::new(None),
            open: RwSignal::new(false),
            busy: RwSignal::new(false),
            chosen: Chosen::new(&PermissionsDto::default()),
            password: RwSignal::new(String::new()),
            password_error: RwSignal::new(None),
            error: RwSignal::new(None),
        }
    }

    /// Opens the dialog on `token`, its groups holding what it holds now.
    fn ask(self, token: ApiTokenDto) {
        self.chosen.set(&token.permissions);
        self.password.set(String::new());
        self.password_error.set(None);
        self.error.set(None);
        self.token.set(Some(token));
        self.open.set(true);
    }
}

/// The API tokens' section: the list, the toast that answers a token
/// revoked or changed, which nothing else in it confirms, the form that
/// creates one, the dialog that shows a new one's whole text once, the
/// dialog that changes a token's permissions, and the dialog that asks
/// before one is revoked. With sign-in off the daemon refuses the list
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
    // The reads of the list, so an answer from before a newer read or a
    // change of a token's permissions doesn't bring the old list back.
    let reads = StoredValue::new(ListReads::default());
    let load = Callback::new(move |()| {
        let url = format!("{}{TOKENS_PATH}", base.get_value());
        let localizer = Arc::clone(&load_localizer);
        let mine = reads.try_update_value(ListReads::start).unwrap_or_default();
        spawn_local(async move {
            let read = ws::get::<ApiTokensDto>(&url).await;
            // The reader may have left the page before the answer came.
            if phase.try_get_untracked().is_none() {
                return;
            }
            if !reads
                .try_update_value(|reads| reads.counts(mine))
                .unwrap_or(false)
            {
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
    // The new token's whole text, empty while no dialog shows one.
    let secret = RwSignal::new(String::new());
    let secret_open = RwSignal::new(false);
    let copy_said = RwSignal::<Option<String>>::new(None);
    Effect::new(move |_| {
        if !secret_open.get() {
            secret.set(String::new());
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
                        secret.set(made.secret);
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
    let toasts = Toasts::new();
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
                    toasts.show(ToastTone::Success, say(&localizer, revoked(&token.label)));
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

    // The change dialog: once saved, the token's row holds its new
    // permissions, read back from the daemon's answer.
    let changing = Change::new();
    let ask_change = Callback::new(move |token: ApiTokenDto| changing.ask(token));
    let change_localizer = Arc::clone(&localizer);
    let change = Callback::new(move |token: ApiTokenDto| {
        if changing.busy.get_untracked() {
            return;
        }
        changing.busy.set(true);
        changing.error.set(None);
        let localizer = Arc::clone(&change_localizer);
        let url = format!("{}{}", base.get_value(), change_path(&token.id));
        let body = change_request(changing.chosen.read(), changing.password.get_untracked());
        spawn_local(async move {
            let answer = ws::put(&url, &body).await;
            if changing.busy.try_get_untracked().is_none() {
                return;
            }
            let kept = match answer {
                Some(Posted::Done(answer)) => serde_json::from_str::<PermissionsDto>(&answer).ok(),
                Some(Posted::Refused(ErrorCode::CurrentPasswordWrong, args, detail)) => {
                    changing.busy.set(false);
                    // Cleared first, so a second wrong try is said again.
                    changing.password_error.set(None);
                    tick().await;
                    changing.password_error.try_set(Some(
                        refused_sentence(
                            &localizer,
                            ErrorCode::CurrentPasswordWrong,
                            &args,
                            detail.as_deref(),
                        )
                        .plain(),
                    ));
                    return;
                }
                Some(Posted::Refused(code, args, detail)) => {
                    changing.busy.set(false);
                    changing.error.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    return;
                }
                None => None,
            };
            let Some(permissions) = kept else {
                // No answer, or not the one the route gives.
                changing.busy.set(false);
                changing.error.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_SESSION_UNREACHABLE),
                    None,
                )));
                return;
            };
            changing.password.set(String::new());
            changing.password_error.set(None);
            tokens.update(|tokens| {
                if let Some(kept) = tokens.iter_mut().flatten().find(|kept| kept.id == token.id) {
                    kept.permissions = permissions;
                }
            });
            // A read still unanswered, as after a revoke, may hold the old
            // permissions: a new one overtakes it.
            if reads.try_with_value(ListReads::changed).unwrap_or(false) {
                load.run(());
            }
            // Once the row is drawn again, so the dialog hands focus to its
            // new Change button; busy until then, so the confirm button
            // takes no press.
            request_animation_frame(move || {
                changing.busy.try_set(false);
                changing.open.try_set(false);
                toasts.show(ToastTone::Success, say(&localizer, changed(&token.label)));
            });
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
    let change_dialog_localizer = Arc::clone(&localizer);
    let viewer = l10n::viewer_zone();
    let (phase, pace) = paced(move || phase.get(), |phase| *phase == Phase::Loading);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            <p>{body}</p>
            // Dismissed, a toast hands focus to the heading just before it.
            <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=HEADING_ID />
            {move || match phase.get() {
                Phase::Loading => first_load(loading.clone(), Awaited::table(5), pace).into_any(),
                Phase::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
                Phase::Ready => {
                    match (refused.get(), tokens.get()) {
                        (Some(sentence), _) => refusal_alert(&list_localizer, sentence).into_any(),
                        (None, Some(list)) if !list.is_empty() => {
                            tokens_table(
                                    &list_localizer,
                                    list,
                                    ask_change,
                                    ask_revoke,
                                    viewer.as_ref(),
                                )
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
            {move || {
                changing
                    .token
                    .get()
                    .map(|token| {
                        view! {
                            <ChangeDialog
                                localizer=Arc::clone(&change_dialog_localizer)
                                token=token
                                changing=changing
                                change=change
                            />
                        }
                    })
            }}
        </section>
    }
}

/// The table of the tokens kept, named by the section's heading: each
/// token's label as its row's header, its permissions (one said as it is,
/// several as a list, #430), when it was created and last used, and its
/// Change permissions and Revoke buttons, named for it. Each button hands
/// focus to the section's heading when its row goes while it holds focus,
/// as after a revoke.
fn tokens_table(
    localizer: &Arc<Localizer>,
    tokens: Vec<ApiTokenDto>,
    ask_change: Callback<ApiTokenDto>,
    ask_revoke: Callback<ApiTokenDto>,
    viewer: Option<&IanaZone>,
) -> impl IntoView + use<> {
    let now_ms = ws::now_ms();
    let label = Column::new(
        "label",
        text(localizer, msg::UI_API_TOKENS_COLUMN_LABEL),
        Rem::tenths(80),
        |row: Signal<ApiTokenDto>| (move || row.with(|token| token.label.clone())).into_any(),
    );
    let permissions_localizer = Arc::clone(localizer);
    let permissions = Column::new(
        "permissions",
        text(localizer, msg::UI_API_TOKENS_COLUMN_PERMISSIONS),
        // Wide enough for "Configuration" on one line; the label's column gave
        // up the 2 rem, so the table still fits its panel.
        Rem::tenths(100),
        move |row: Signal<ApiTokenDto>| {
            row.with_untracked(|token| permissions_view(&permissions_localizer, &token.permissions))
        },
    );
    let created_localizer = Arc::clone(localizer);
    let zone = viewer.cloned();
    let created = Column::new(
        "created",
        text(localizer, msg::UI_API_TOKENS_COLUMN_CREATED),
        Rem::tenths(90),
        move |row: Signal<ApiTokenDto>| {
            let raw = row.with_untracked(|token| token.created_at.clone());
            let said = instant(&created_localizer, &raw, zone.as_ref());
            view! { <time datetime=raw>{said}</time> }.into_any()
        },
    );
    let used_localizer = Arc::clone(localizer);
    let last_used = Column::new(
        "last-used",
        text(localizer, msg::UI_API_TOKENS_COLUMN_LAST_USED),
        Rem::tenths(80),
        move |row: Signal<ApiTokenDto>| {
            let raw = row.with_untracked(|token| token.last_used_at.clone());
            let said = match LastUsed::of(raw.as_deref(), now_ms).said() {
                Ok(said) => say(&used_localizer, said),
                Err(raw) => raw,
            };
            match raw {
                Some(raw) => view! { <time datetime=raw>{said}</time> }.into_any(),
                None => said.into_any(),
            }
        },
    );
    let actions_localizer = Arc::clone(localizer);
    let actions = Column::new(
        "actions",
        text(localizer, msg::UI_COLUMN_ACTIONS),
        Rem::tenths(90),
        move |row: Signal<ApiTokenDto>| {
            token_actions(
                &actions_localizer,
                row.get_untracked(),
                ask_change,
                ask_revoke,
            )
        },
    );
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_API_TOKENS_NONE),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=Name::Heading(HEADING_ID.to_owned())
            columns_choice=ColumnsChoice::new("api-tokens", Arc::clone(localizer))
            row_header=label
            columns=vec![permissions, created, last_used, actions]
            rows=Signal::stored(tokens)
            key=|token: &ApiTokenDto| token.id.clone()
            state=State::Ready
            sentences=sentences
        />
    }
}

/// A token's Change permissions and Revoke buttons, named for it.
fn token_actions(
    localizer: &Localizer,
    token: ApiTokenDto,
    ask_change: Callback<ApiTokenDto>,
    ask_revoke: Callback<ApiTokenDto>,
) -> AnyView {
    let (change_text, revoke_text) = (
        say(localizer, change_label(&token.label)),
        say(localizer, revoke_label(&token.label)),
    );
    let change_button = NodeRef::<leptos::html::Button>::new();
    let revoke_button = NodeRef::<leptos::html::Button>::new();
    hand_focus_when_gone(change_button, HEADING_ID.to_owned());
    hand_focus_when_gone(revoke_button, HEADING_ID.to_owned());
    let (change_id_of, revoke_id_of) = (change_id(&token.id), revoke_id(&token.id));
    let change_for = token.clone();
    view! {
        <div class=ACTIONS>
            <Button
                content=Content::IconAndText {
                    glyph: LucideGlyph::Pencil.into(),
                    label: change_text.into(),
                }
                size=Size::Small
                id=change_id_of
                node_ref=change_button
                on_press=Callback::new(move |()| ask_change.run(change_for.clone()))
            />
            <Button
                content=Content::IconAndText {
                    glyph: LucideGlyph::Trash.into(),
                    label: revoke_text.into(),
                }
                tone=Tone::Error
                size=Size::Small
                id=revoke_id_of
                node_ref=revoke_button
                on_press=Callback::new(move |()| ask_revoke.run(token.clone()))
            />
        </div>
    }
    .into_any()
}

/// A token's permissions in its row: one said as it is, several as a list.
fn permissions_view(localizer: &Localizer, permissions: &PermissionsDto) -> AnyView {
    let names: Vec<String> = granted_of(permissions)
        .into_iter()
        .map(|row| say(localizer, row.granted()))
        .collect();
    match names.as_slice() {
        [one] => one.clone().into_any(),
        _ => view! { <ul>{names.into_iter().map(|name| view! { <li>{name}</li> }).collect_view()}</ul> }
        .into_any(),
    }
}

/// The form that creates a token: its label, its permissions (a radio group
/// each, with a hint naming what it opens, starting at a new token's), and
/// the error summary that takes focus when the form is refused.
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
                <TextField
                    id=Field::Label.id()
                    localizer=Arc::clone(&localizer)
                    label=field_text(msg::UI_API_TOKEN_LABEL_LABEL)
                    description=label_hint
                    error=problems.label
                    value=inputs.label
                    required=true
                />
                <fieldset
                    class="choices"
                    aria-describedby=move || {
                        if problems.permissions.get().is_some() {
                            format!("{PERMISSIONS_HINT_ID} {PERMISSIONS_ERROR_ID}")
                        } else {
                            PERMISSIONS_HINT_ID.to_owned()
                        }
                    }
                >
                    <legend>{field_text(msg::UI_API_TOKEN_PERMISSIONS_LEGEND)}</legend>
                    <p id=PERMISSIONS_HINT_ID>{field_text(msg::UI_API_TOKEN_PERMISSIONS_HINT)}</p>
                    {move || {
                        problems
                            .permissions
                            .get()
                            .map(|error| {
                                view! {
                                    <p id=PERMISSIONS_ERROR_ID class="form-error">
                                        {error}
                                    </p>
                                }
                            })
                    }}
                    <PermissionGroups
                        localizer=Arc::clone(&localizer)
                        form=Form::Create
                        chosen=inputs.permissions
                    />
                </fieldset>
                <p class=ACTIONS>
                    <Button
                        content=Content::Text(field_text(msg::UI_API_TOKEN_CREATE).into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        kind=Kind::Submit
                        id=CREATE_ID
                        busy=busy
                    />
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
    secret: RwSignal<String>,
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
            <TextField
                id=SECRET_ID
                localizer=Arc::clone(&localizer)
                label=text(&localizer, msg::UI_API_TOKEN_CREATED_FIELD)
                value=secret
                kind=text_field::Kind::Code
                access=text_field::Access::ReadOnly
                token=true
            />
            <p class=ACTIONS>
                <Button
                    content=Content::Text(text(&localizer, msg::UI_API_TOKEN_COPY).into())
                    node_ref=copy_button
                    on_press=Callback::new(move |()| {
                        let token = secret.get_untracked();
                        if !token.is_empty() {
                            copy_text(&token, said, copied.clone(), refused.clone());
                        }
                    })
                />
            </p>
            <p aria-live="polite" aria-atomic="true">
                {move || said.get()}
            </p>
        </ReviewDialog>
    }
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
        tone: Tone::Error,
        action: Callback::new(move |()| revoke.run(token.clone())),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_API_TOKEN_REVOKING),
    };
    let error_localizer = Arc::clone(&localizer);
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
            {move || error.get().map(|sentence| dialog_refusal_alert(&error_localizer, sentence))}
        </ReviewDialog>
    }
}

/// The dialog that changes the token's permissions, named for it: its
/// groups, starting at what the token holds, and the admin password, which
/// saving takes (decision 127). Cancel first, so a stray Enter backs out.
/// A wrong password is the field's error, said in the dialog; any other
/// refusal is an alert inside it. Once saved, focus returns to the token's
/// Change permissions button.
#[component]
fn ChangeDialog(
    localizer: Arc<Localizer>,
    token: ApiTokenDto,
    changing: Change,
    change: Callback<ApiTokenDto>,
) -> impl IntoView {
    let title = say(&localizer, change_title(&token.label));
    let returns_to = change_id(&token.id);
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_API_TOKEN_CHANGE_CONFIRM),
        tone: Tone::Primary,
        action: Callback::new(move |()| change.run(token.clone())),
    };
    let working = Working {
        active: changing.busy.into(),
        status: text(&localizer, msg::UI_API_TOKEN_CHANGING),
    };
    let error_localizer = Arc::clone(&localizer);
    view! {
        <ReviewDialog
            open=changing.open
            title=title
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=returns_to
            described_by=CHANGE_BODY_ID.to_owned()
            working=working
            fallback=HEADING_ID.to_owned()
        >
            <p id=CHANGE_BODY_ID>{text(&localizer, msg::UI_API_TOKEN_CHANGE_BODY)}</p>
            <PermissionGroups
                localizer=Arc::clone(&localizer)
                form=Form::Change
                chosen=changing.chosen
            />
            <StepUpField
                id=CHANGE_PASSWORD_ID
                localizer=Arc::clone(&localizer)
                step_up=StepUp::Password
                value=changing.password
                error=changing.password_error
            />
            {move || {
                changing
                    .error
                    .get()
                    .map(|sentence| dialog_refusal_alert(&error_localizer, sentence))
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

/// The restart section (decision 94): offered only when a service manager
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
    let pending_localizer = Arc::clone(&localizer);
    // A cancel refused while the restart waits is shown in the section and
    // said assertively, as the page's answer; one refused in the dialog is
    // said by the dialog's own alert alone.
    let pending_refused = Signal::derive(move || {
        (section.stage.get() == Stage::Pending)
            .then(|| error.get())
            .flatten()
    });
    say_refusals(pending_refused);
    let (stage, pace) = paced(
        move || section.stage.get(),
        |stage| *stage == Stage::Loading,
    );
    view! {
        <section aria-labelledby=restart::HEADING_ID>
            <h2 id=restart::HEADING_ID tabindex="-1">
                {heading}
            </h2>
            {move || {
                let localizer = &stage_localizer;
                match stage.get() {
                    Stage::Loading => {
                        first_load(text(localizer, msg::UI_LOADING), Awaited::Text, pace).into_any()
                    }
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
                                <Button
                                    content=Content::Text(
                                        text(localizer, msg::UI_RESTART_BUTTON).into(),
                                    )
                                    id=restart::BUTTON_ID
                                    node_ref=button
                                    on_press=Callback::new(move |()| offer.run(()))
                                />
                            </p>
                        }
                            .into_any()
                    }
                    Stage::Pending => {
                        let button = NodeRef::<leptos::html::Button>::new();
                        hand_focus_when_gone(button, restart::HEADING_ID.to_owned());
                        view! {
                            <p>
                                <Button
                                    content=Content::Text(
                                        text(localizer, msg::UI_RESTART_CANCEL).into(),
                                    )
                                    node_ref=button
                                    busy=busy
                                    on_press=Callback::new(move |()| cancel.run(()))
                                />
                            </p>
                        }
                            .into_any()
                    }
                    Stage::Restarting { .. } => ().into_any(),
                }
            }}
            <p>{move || status.get().map(|id| text(&status_localizer, id))}</p>
            {move || {
                pending_refused.get().map(|sentence| refusal_alert(&pending_localizer, sentence))
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
        tone: Tone::Primary,
        action: Callback::new(move |()| ask.run(RestartWhenDto::Finished)),
    };
    let working = Working {
        active: busy.into(),
        status: text(&localizer, msg::UI_RESTART_ASKING),
    };
    let now = offers_now(running).then(|| Confirmation {
        label: text(&localizer, msg::UI_RESTART_NOW),
        tone: Tone::Error,
        action: Callback::new(move |()| ask.run(RestartWhenDto::Now)),
    });
    let error_localizer = Arc::clone(&localizer);
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
            {move || error.get().map(|sentence| dialog_refusal_alert(&error_localizer, sentence))}
        </ReviewDialog>
    }
}
