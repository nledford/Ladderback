//! Where the page stands with sign-in (security §10.1), decided on the host:
//! what the daemon's answer to `GET /api/v1/session` means for what the page
//! shows, which refusal says the session has ended, what the sign-in form
//! lacks before it is sent, and what a sign-in came back with. The fetches
//! and the DOM around them are `ws.rs` and `sign_in.rs`, which the `@ui`
//! scenarios of `features/security/sign_in.feature` run in a browser.

use api::{
    AdminPasswordDto, AvailableUpdateDto, BuildDto, ErrorCode, ExposureDto, MsgArg, SessionDto,
    SignInStateDto,
};
use l10n::{MessageId, msg};
use serde::Deserialize;

use crate::backend::succeeded;

/// Where the page stands with sign-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Standing {
    /// The daemon has not answered yet: the page shows nothing but its frame.
    #[default]
    Asking,
    /// No admin password is set: the pages are shown, and say who can use
    /// them (see [`SignInOffNotice`]).
    Off,
    /// Sign-in is on and the page has no live session: the sign-in page is
    /// shown in place of the page asked for.
    SignedOut,
    /// The page has a live session: the pages are shown.
    SignedIn,
    /// The daemon could not say: the pages are shown, and each says in its
    /// own way that the daemon can't be reached, or asks for sign-in when a
    /// request of theirs is refused for want of a session.
    Unread,
}

impl Standing {
    /// What the daemon's answer to `GET /api/v1/session` means; `None` is no
    /// answer the page could read.
    pub(crate) fn of(read: Option<&SessionDto>) -> Self {
        match read.map(|session| session.sign_in) {
            Some(SignInStateDto::Off) => Self::Off,
            Some(SignInStateDto::SignedOut) => Self::SignedOut,
            Some(SignInStateDto::SignedIn) => Self::SignedIn,
            None => Self::Unread,
        }
    }

    /// Whether the page shows the pages of the app, rather than the sign-in
    /// page or nothing yet.
    pub(crate) fn shows_pages(self) -> bool {
        matches!(self, Self::Off | Self::SignedIn | Self::Unread)
    }

    /// Whether a failed Sign out (`failed`) is still said on this standing.
    /// The failure is the session's, so the page forgets it once it has
    /// none, as after Sign out everywhere or a new password.
    pub(crate) fn keeps_sign_out_failure(self, failed: bool) -> bool {
        failed && self == Self::SignedIn
    }
}

/// Which notice says that sign-in is off, and where the page shows it
/// (§10.1). Sign-in is decided when the daemon starts, so the first answer
/// decides it for as long as the page is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SignInOffNotice {
    /// Sign-in is on, or the daemon could not say: no notice.
    #[default]
    None,
    /// Sign-in is off on loopback: the start page says that anyone on this
    /// machine can use Ladderback.
    StartPage,
    /// Sign-in is off and the daemon answers on the network: every page says
    /// that anyone who can reach it can use it.
    EveryPage,
}

impl SignInOffNotice {
    /// The notice the daemon's answer to `GET /api/v1/session` calls for;
    /// `None` is no answer the page could read.
    pub(crate) fn of(read: Option<&SessionDto>) -> Self {
        match read.map(|session| (session.sign_in, session.exposure)) {
            Some((SignInStateDto::Off, ExposureDto::Loopback)) => Self::StartPage,
            Some((SignInStateDto::Off, ExposureDto::Network)) => Self::EveryPage,
            Some((SignInStateDto::SignedOut | SignInStateDto::SignedIn, _)) | None => Self::None,
        }
    }

    /// Whether each page's heading is described by this notice, so a screen
    /// reader reads it after the heading that takes focus on arrival.
    pub(crate) fn describes_the_heading(self) -> bool {
        self == Self::EveryPage
    }
}

/// What Settings says of an admin password set since the daemon started with
/// sign-in off (decision 70): that a restart turns sign-in on, or that the
/// file is damaged and the password must be set again; `None` while nothing
/// waits, or with no answer the page could read.
pub(crate) fn waiting_password(read: Option<&SessionDto>) -> Option<MessageId> {
    match read?.admin_password? {
        AdminPasswordDto::Set => Some(msg::UI_ADMIN_PASSWORD_WAITING),
        AdminPasswordDto::Unreadable => Some(msg::UI_ADMIN_PASSWORD_DAMAGED),
    }
}

/// Whether the sign-in form offers **Keep me signed in on this device**, as
/// a signed-out answer says (`keep-signed-in-offered`): it doesn't under
/// **Stay signed in for** Until I close the browser. A signed-out answer
/// that doesn't say, from a daemon from before the choice, offers it, as the
/// daemon then reads it. `None` is an answer that isn't signed out, which
/// says nothing of the form.
pub(crate) fn keep_signed_in_offered(read: &SessionDto) -> Option<bool> {
    (read.sign_in == SignInStateDto::SignedOut)
        .then_some(read.keep_signed_in_offered != Some(false))
}

/// Whether a sign-in asks to be kept: only when the box is `checked` and
/// the form still `offered` it. A box checked while it was offered asks for
/// nothing once it isn't.
pub(crate) fn asks_to_keep(checked: bool, offered: bool) -> bool {
    checked && offered
}

/// What the page's title says once its standing has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Title {
    /// The sign-in page's own title.
    SignIn,
    /// The title of the page the address names, shown once signed in.
    Address,
}

impl Title {
    /// The title the page takes when its standing turns from `previous` (none
    /// on the first answer) to `now`; `None` keeps the title the page has,
    /// which the address named when it loaded.
    pub(crate) fn on(previous: Option<Standing>, now: Standing) -> Option<Self> {
        if now == Standing::SignedOut {
            Some(Self::SignIn)
        } else if previous == Some(Standing::SignedOut) {
            Some(Self::Address)
        } else {
            None
        }
    }
}

/// The code of a refusal, as the daemon writes it.
#[derive(Deserialize)]
struct Refusal {
    code: ErrorCode,
    #[serde(default)]
    args: std::collections::BTreeMap<String, MsgArg>,
}

/// What the page does with the answer to a request it sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resend {
    /// Nothing: the answer is the request's, for its page to read.
    No,
    /// Asks to sign in again, then sends the request anew: the session has
    /// ended (`sign-in-required`). The daemon refuses that before any route
    /// runs, so the request did nothing.
    OnceSignedIn,
    /// Reads the session's current CSRF token, then sends the request once
    /// more: it carried a token the session doesn't have
    /// (`csrf-token-invalid`), as after signing in again in another tab,
    /// which replaced the session the tabs share. The daemon refuses that
    /// before any route runs too.
    WithFreshToken,
}

impl Resend {
    /// What follows an answer of `status` with `body`; `token_read` is
    /// whether the page has already read a fresh token for this request,
    /// which it does once: a second refusal is the request's answer.
    pub(crate) fn after(status: u16, body: &str, token_read: bool) -> Self {
        let code = || {
            serde_json::from_str::<Refusal>(body)
                .ok()
                .map(|refusal| refusal.code)
        };
        match status {
            401 if code() == Some(ErrorCode::SignInRequired) => Self::OnceSignedIn,
            403 if !token_read && code() == Some(ErrorCode::CsrfTokenInvalid) => {
                Self::WithFreshToken
            }
            _ => Self::No,
        }
    }
}

/// Who caused a request (security §10.1): a person, by opening a page, a
/// form's action or "Stay signed in", or the page on its own, as a re-read
/// after an event or a poll is. Only a person's request uses the session,
/// so a page left open lets it end unused; the page marks its own with
/// `api::BACKGROUND_HEADER` so the daemon doesn't count them either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    ByPerson,
    OnItsOwn,
}

/// Where the reader stands with the dialog asking to sign in again: how
/// many times they have closed it without signing in, and whether they have
/// since the page last signed in. A request refused because its session
/// ended opens the dialog only when it was sent after the last of those
/// closings: one still on its way when the dialog closed ends with its
/// refusal, as the requests that waited do. Once the reader has closed the
/// dialog, only a request the reader asked for opens it again; the reads the
/// page sends on its own end with their refusal until it signs in (#903).
/// While the dialog is open, every refused request waits in it: nothing new
/// opens under the reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dismissals {
    count: u32,
    since_sign_in: bool,
}

impl Dismissals {
    /// Where the reader stands after closing the dialog once more.
    pub(crate) fn one_more(self) -> Self {
        todo!()
    }

    /// Where the reader stands after the page signed in.
    pub(crate) fn signed_in(self) -> Self {
        todo!()
    }

    /// Whether a request `asked` so and sent while the reader stood at
    /// `sent`, refused now because its session ended, waits for a sign-in:
    /// in the dialog, `open` already or opened for it.
    pub(crate) fn waits_for_sign_in(self, sent: Self, asked: Asked, open: bool) -> bool {
        todo!()
    }
}

/// What the sign-in form lacks before it is sent: the admin password, when
/// nothing was typed. What was typed is sent as it is, spaces and all: a
/// password is never trimmed.
pub(crate) fn lacking(password: &str) -> Option<MessageId> {
    password
        .is_empty()
        .then_some(msg::UI_SIGN_IN_PASSWORD_REQUIRED)
}

/// How long before a session would end unused the page warns, and offers to
/// keep it (WCAG 2.2.1 Timing Adjustable). Two minutes is six times the 20
/// seconds 2.2.1 asks for: a screen reader has read the warning and someone
/// using a switch or their voice has reached its button with time to spare,
/// and a session left alone still ends at its 12 hours, not long before.
pub(crate) const WARNING_LEAD_MS: i64 = 120_000;

/// The longest the page waits on one timer: a browser runs a timer of more
/// than 2³¹ − 1 milliseconds (under 25 days) at once, and a session's
/// lifetime is 30 days, so a longer wait is taken a day at a time.
pub(crate) const LONGEST_WAIT_MS: u32 = 86_400_000;

fn millis(seconds: u64) -> i64 {
    i64::try_from(seconds)
        .unwrap_or(i64::MAX)
        .saturating_mul(1000)
}

/// A live session, as a sign-in or the session route gave it: the CSRF token
/// its changed requests carry, its lease, and what the daemon tells of
/// itself, which the header shows once the page may see the pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Signed {
    pub(crate) csrf: String,
    pub(crate) lease: Lease,
    pub(crate) told: Option<Told>,
}

impl Signed {
    /// The live session `read` says the page has, in answer to a request
    /// sent at `sent`; `None` when it says the page has none, or leaves out
    /// its CSRF token. A session with no idle limit (kept signed in) or no
    /// end after its sign-in (Until I sign out, Until I close the browser)
    /// leaves that out, and lasts as long as can be counted by it.
    pub(crate) fn of(read: &SessionDto, sent: i64) -> Option<Self> {
        if read.sign_in != SignInStateDto::SignedIn {
            return None;
        }
        Some(Self {
            csrf: read.csrf_token.clone()?,
            lease: Lease::new(
                read.idle_seconds.map_or(i64::MAX, millis),
                read.ends_in_seconds.map_or(i64::MAX, millis),
                sent,
            ),
            told: Told::of(read),
        })
    }
}

/// What the daemon tells a page that may see the pages of itself, which the
/// header shows: the build it runs (decision 90) and the update its release
/// check found, when there is one (decision 107).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    pub(crate) build: BuildDto,
    pub(crate) update: Option<AvailableUpdateDto>,
}

impl Told {
    /// What `read` tells; `None` when it leaves out the build, as for a page
    /// yet to sign in, whatever else it says: the update comes only with
    /// the build.
    pub(crate) fn of(read: &SessionDto) -> Option<Self> {
        read.build.clone().map(|build| Self {
            build,
            update: read.available_update.clone(),
        })
    }
}

/// A live session by the page's clock, in milliseconds since the epoch: when
/// the page last used it, how long it lasts unused, and when it ends however
/// it is used. A request is counted as used when it is sent, a little before
/// the daemon counts it, so the page never thinks a session lasts longer than
/// it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lease {
    idle_ms: i64,
    used_at: i64,
    ends_at: i64,
}

/// Where a lease stands at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stand {
    /// Nothing to say until `until`.
    Quiet { until: i64 },
    /// The session ends unused at `until`: the page warns, and offers to keep
    /// it.
    Warning { until: i64 },
    /// The session has ended: the page asks to sign in again.
    Ended,
}

impl Lease {
    /// The lease of a session that lasts `idle_ms` unused and has
    /// `ends_in_ms` left however it is used, as the daemon said in answer to
    /// a request sent at `sent`, which was a use. The daemon counted from
    /// when the request reached it, a little later.
    fn new(idle_ms: i64, ends_in_ms: i64, sent: i64) -> Self {
        Self {
            idle_ms,
            used_at: sent,
            ends_at: sent.saturating_add(ends_in_ms),
        }
    }

    /// The lease once a request sent at `at` has used the session again.
    pub(crate) fn used(self, at: i64) -> Self {
        Self {
            used_at: self.used_at.max(at),
            ..self
        }
    }

    /// Where the lease stands at `now`. Once a session would end unused no
    /// sooner than its lifetime ends it, using it can't keep it, so there is
    /// nothing to offer: 2.2.1's 20-hour exception covers that end, and the
    /// page says nothing until it comes.
    pub(crate) fn at(self, now: i64) -> Stand {
        let idle_ends = self.used_at.saturating_add(self.idle_ms);
        if idle_ends >= self.ends_at {
            return if now < self.ends_at {
                Stand::Quiet {
                    until: self.ends_at,
                }
            } else {
                Stand::Ended
            };
        }
        let warn_at = idle_ends.saturating_sub(WARNING_LEAD_MS);
        if now < warn_at {
            Stand::Quiet { until: warn_at }
        } else if now < idle_ends {
            Stand::Warning { until: idle_ends }
        } else {
            Stand::Ended
        }
    }
}

/// How long a timer set at `now` waits for `until`: nothing once it has
/// passed, and at most [`LONGEST_WAIT_MS`], after which the page looks again.
pub(crate) fn wait_ms(now: i64, until: i64) -> u32 {
    let wait = until
        .saturating_sub(now)
        .clamp(0, i64::from(LONGEST_WAIT_MS));
    u32::try_from(wait).unwrap_or(LONGEST_WAIT_MS)
}

/// What a sign-in came back with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// A session started.
    SignedIn(Signed),
    /// Sign-in is off: there is nothing to sign in to.
    Off,
    /// A typed refusal, to be said in the catalog's words.
    Refused(ErrorCode, Vec<(String, MsgArg)>),
    /// No answer the page could read.
    Unreadable,
}

impl Answer {
    /// What `POST /api/v1/session`, sent at `sent`, answered with `status`
    /// and `body`.
    pub(crate) fn of(status: u16, body: &str, sent: i64) -> Self {
        if succeeded(status) {
            let Ok(read) = serde_json::from_str::<SessionDto>(body) else {
                return Self::Unreadable;
            };
            if read.sign_in == SignInStateDto::Off {
                return Self::Off;
            }
            return Signed::of(&read, sent).map_or(Self::Unreadable, Self::SignedIn);
        }
        match serde_json::from_str::<Refusal>(body) {
            Ok(refusal) => Self::Refused(refusal.code, refusal.args.into_iter().collect()),
            Err(_) => Self::Unreadable,
        }
    }
}

/// Whether a sign-in refused with `code` is about what was typed, so the
/// field is marked invalid: a wrong password is; a wait, or a password the
/// daemon could not check, is not.
pub(crate) fn about_the_password(code: ErrorCode) -> bool {
    code == ErrorCode::SignInFailed
}

/// Whether a Sign out the daemon refused with `code` has signed out all the
/// same: `sessions-unavailable` says the database still holds the session,
/// but the daemon has ended it and the browser has forgotten its cookie, so
/// the page forgets it too and shows the sign-in page. Any other refusal
/// left the session live.
pub(crate) fn signed_out_all_the_same(code: ErrorCode) -> bool {
    code == ErrorCode::SessionsUnavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(sign_in: SignInStateDto, csrf: Option<&str>) -> SessionDto {
        SessionDto {
            sign_in,
            csrf_token: csrf.map(str::to_owned),
            idle_seconds: None,
            ends_in_seconds: None,
            exposure: api::ExposureDto::Loopback,
            build: None,
            available_update: None,
            admin_password: None,
            keep_signed_in_offered: None,
        }
    }

    fn on_the_network(sign_in: SignInStateDto) -> SessionDto {
        SessionDto {
            exposure: api::ExposureDto::Network,
            ..session(sign_in, None)
        }
    }

    #[test]
    fn a_failed_sign_out_is_kept_only_while_the_page_is_signed_in() {
        let all = [
            Standing::Asking,
            Standing::Off,
            Standing::SignedOut,
            Standing::SignedIn,
            Standing::Unread,
        ];
        let keeping = |failed| {
            all.into_iter()
                .filter(|standing| standing.keeps_sign_out_failure(failed))
                .collect::<Vec<_>>()
        };
        assert_eq!(keeping(true), [Standing::SignedIn]);
        assert_eq!(keeping(false), []);
    }

    #[test]
    fn a_signed_out_answer_says_whether_keep_me_signed_in_is_offered() {
        let offered = |said| SessionDto {
            keep_signed_in_offered: said,
            ..session(SignInStateDto::SignedOut, None)
        };
        assert_eq!(keep_signed_in_offered(&offered(Some(false))), Some(false));
        assert_eq!(keep_signed_in_offered(&offered(Some(true))), Some(true));
        // A daemon from before the choice doesn't say, and takes it.
        assert_eq!(keep_signed_in_offered(&offered(None)), Some(true));
    }

    #[test]
    fn an_answer_that_is_not_signed_out_says_nothing_of_keep_me_signed_in() {
        for state in [SignInStateDto::SignedIn, SignInStateDto::Off] {
            let read = SessionDto {
                keep_signed_in_offered: Some(false),
                ..session(state, Some("t"))
            };
            assert_eq!(keep_signed_in_offered(&read), None, "{state:?}");
        }
    }

    #[test]
    fn a_sign_in_asks_to_be_kept_only_with_the_box_checked_while_it_is_offered() {
        assert!(asks_to_keep(true, true));
        assert!(!asks_to_keep(true, false));
        assert!(!asks_to_keep(false, true));
        assert!(!asks_to_keep(false, false));
    }

    #[test]
    fn sign_in_off_is_noted_on_the_start_page_on_loopback_and_on_every_page_on_the_network() {
        assert_eq!(
            SignInOffNotice::of(Some(&session(SignInStateDto::Off, None))),
            SignInOffNotice::StartPage
        );
        assert_eq!(
            SignInOffNotice::of(Some(&on_the_network(SignInStateDto::Off))),
            SignInOffNotice::EveryPage
        );
        for sign_in in [SignInStateDto::SignedOut, SignInStateDto::SignedIn] {
            assert_eq!(
                SignInOffNotice::of(Some(&session(sign_in, None))),
                SignInOffNotice::None
            );
            assert_eq!(
                SignInOffNotice::of(Some(&on_the_network(sign_in))),
                SignInOffNotice::None
            );
        }
        assert_eq!(SignInOffNotice::of(None), SignInOffNotice::None);
    }

    #[test]
    fn only_the_notice_on_every_page_describes_the_heading() {
        assert!(SignInOffNotice::EveryPage.describes_the_heading());
        assert!(!SignInOffNotice::StartPage.describes_the_heading());
        assert!(!SignInOffNotice::None.describes_the_heading());
    }

    #[test]
    fn settings_says_an_admin_password_that_waits_for_a_restart_or_is_damaged() {
        let waiting = |admin_password| SessionDto {
            admin_password,
            ..session(SignInStateDto::Off, None)
        };
        assert_eq!(
            waiting_password(Some(&waiting(Some(AdminPasswordDto::Set)))),
            Some(msg::UI_ADMIN_PASSWORD_WAITING)
        );
        assert_eq!(
            waiting_password(Some(&waiting(Some(AdminPasswordDto::Unreadable)))),
            Some(msg::UI_ADMIN_PASSWORD_DAMAGED)
        );
        assert_eq!(waiting_password(Some(&waiting(None))), None);
        assert_eq!(waiting_password(None), None);
    }

    const HOUR: i64 = 3_600_000;
    const IDLE: i64 = 12 * HOUR;
    const LIFETIME: i64 = 30 * 24 * HOUR;
    const SIGNED_IN_AT: i64 = 1_800_000_000_000;

    fn fresh() -> Lease {
        Lease::new(IDLE, LIFETIME, SIGNED_IN_AT)
    }

    /// What a sign-in sent at [`SIGNED_IN_AT`] came back with.
    fn answer(status: u16, body: &str) -> Answer {
        Answer::of(status, body, SIGNED_IN_AT)
    }

    #[test]
    fn each_answer_of_the_daemon_is_a_standing() {
        assert_eq!(
            Standing::of(Some(&session(SignInStateDto::Off, None))),
            Standing::Off
        );
        assert_eq!(
            Standing::of(Some(&session(SignInStateDto::SignedOut, None))),
            Standing::SignedOut
        );
        assert_eq!(
            Standing::of(Some(&session(SignInStateDto::SignedIn, Some("t")))),
            Standing::SignedIn
        );
        assert_eq!(Standing::of(None), Standing::Unread);
    }

    #[test]
    fn the_pages_are_shown_unless_the_page_is_signed_out_or_still_asking() {
        assert!(Standing::Off.shows_pages());
        assert!(Standing::SignedIn.shows_pages());
        assert!(Standing::Unread.shows_pages());
        assert!(!Standing::SignedOut.shows_pages());
        assert!(!Standing::Asking.shows_pages());
    }

    #[test]
    fn the_sign_in_page_has_its_own_title_and_signing_in_gives_back_the_addresses() {
        assert_eq!(
            Title::on(Some(Standing::Asking), Standing::SignedOut),
            Some(Title::SignIn)
        );
        assert_eq!(
            Title::on(Some(Standing::SignedOut), Standing::SignedIn),
            Some(Title::Address)
        );
        assert_eq!(Title::on(Some(Standing::Asking), Standing::SignedIn), None);
        assert_eq!(Title::on(Some(Standing::Asking), Standing::Off), None);
        assert_eq!(Title::on(None, Standing::Asking), None);
    }

    #[test]
    fn only_a_sign_in_required_refusal_waits_for_a_sign_in() {
        let after = |status, body| Resend::after(status, body, false);
        assert_eq!(
            after(401, r#"{"code":"sign-in-required"}"#),
            Resend::OnceSignedIn
        );
        assert_eq!(after(401, r#"{"code":"sign-in-failed"}"#), Resend::No);
        assert_eq!(after(403, r#"{"code":"sign-in-required"}"#), Resend::No);
        assert_eq!(after(401, "not json"), Resend::No);
        // A session that ended after a fresh token was read still asks.
        assert_eq!(
            Resend::after(401, r#"{"code":"sign-in-required"}"#, true),
            Resend::OnceSignedIn
        );
    }

    #[test]
    fn a_csrf_refusal_reads_the_sessions_token_and_sends_once_more_only_once() {
        let csrf = r#"{"code":"csrf-token-invalid"}"#;
        assert_eq!(Resend::after(403, csrf, false), Resend::WithFreshToken);
        assert_eq!(Resend::after(403, csrf, true), Resend::No);
        assert_eq!(Resend::after(401, csrf, false), Resend::No);
        assert_eq!(
            Resend::after(403, r#"{"code":"origin-not-allowed"}"#, false),
            Resend::No
        );
        assert_eq!(Resend::after(200, csrf, false), Resend::No);
        assert_eq!(Resend::after(403, "not json", false), Resend::No);
    }

    #[test]
    fn before_the_dialog_is_closed_every_refusal_asks_to_sign_in_again() {
        let fresh = Dismissals::default();
        assert!(fresh.waits_for_sign_in(fresh, Asked::ByPerson, false));
        assert!(fresh.waits_for_sign_in(fresh, Asked::OnItsOwn, false));
    }

    #[test]
    fn a_request_on_its_way_when_the_dialog_closed_ends_with_its_refusal() {
        let sent = Dismissals::default();
        let closed = sent.one_more();
        assert!(!closed.waits_for_sign_in(sent, Asked::ByPerson, false));
        assert!(!closed.waits_for_sign_in(sent, Asked::OnItsOwn, false));
        // Closed twice: a request sent between the closings ends too.
        assert!(
            !closed
                .one_more()
                .waits_for_sign_in(closed, Asked::ByPerson, false)
        );
        // Signing in since doesn't bring it back.
        assert!(
            !closed
                .signed_in()
                .waits_for_sign_in(sent, Asked::ByPerson, false)
        );
    }

    #[test]
    fn after_the_dialog_closed_only_a_request_the_reader_asked_for_asks_again() {
        let closed = Dismissals::default().one_more();
        assert!(closed.waits_for_sign_in(closed, Asked::ByPerson, false));
        assert!(!closed.waits_for_sign_in(closed, Asked::OnItsOwn, false));
    }

    #[test]
    fn after_signing_in_the_pages_own_reads_ask_again() {
        let signed_in = Dismissals::default().one_more().signed_in();
        assert!(signed_in.waits_for_sign_in(signed_in, Asked::OnItsOwn, false));
        assert!(signed_in.waits_for_sign_in(signed_in, Asked::ByPerson, false));
    }

    #[test]
    fn while_the_dialog_is_open_every_refused_request_waits_in_it() {
        let sent = Dismissals::default();
        let closed = sent.one_more();
        assert!(closed.waits_for_sign_in(sent, Asked::OnItsOwn, true));
        assert!(closed.waits_for_sign_in(closed, Asked::OnItsOwn, true));
        assert!(closed.waits_for_sign_in(sent, Asked::ByPerson, true));
    }

    #[test]
    fn an_empty_password_is_lacking_and_any_other_is_sent_as_typed() {
        assert_eq!(lacking(""), Some(msg::UI_SIGN_IN_PASSWORD_REQUIRED));
        assert_eq!(lacking(" "), None);
        assert_eq!(lacking("demo-admin-pass"), None);
    }

    #[test]
    fn a_sign_in_answer_is_a_session_with_its_csrf_token_and_lease() {
        assert_eq!(
            answer(
                200,
                r#"{"sign-in":"signed-in","csrf-token":"abc","idle-seconds":43200,"ends-in-seconds":2592000}"#
            ),
            Answer::SignedIn(Signed {
                csrf: "abc".to_owned(),
                lease: fresh(),
                told: None,
            })
        );
    }

    #[test]
    fn a_sign_in_answer_carries_the_build_the_daemon_runs() {
        assert_eq!(
            answer(
                200,
                r#"{"sign-in":"signed-in","csrf-token":"abc","idle-seconds":43200,"ends-in-seconds":2592000,"version":"0.3.0","commit":"3f937cf0","dirty":false,"release":false}"#
            ),
            Answer::SignedIn(Signed {
                csrf: "abc".to_owned(),
                lease: fresh(),
                told: Some(Told {
                    build: BuildDto {
                        version: "0.3.0".to_owned(),
                        commit: Some("3f937cf0".to_owned()),
                        dirty: false,
                        release: false,
                    },
                    update: None,
                }),
            })
        );
    }

    #[test]
    fn the_available_update_is_told_only_with_the_build() {
        let update = r#""available-update":{"version":"0.4.0","release-notes":"https://github.com/nledford/Ladderback/releases/tag/v0.4.0"}"#;
        let with_build: SessionDto = serde_json::from_str(&format!(
            r#"{{"sign-in":"off","exposure":"loopback","version":"0.3.0","dirty":false,"release":true,{update}}}"#
        ))
        .unwrap();
        assert_eq!(
            Told::of(&with_build).and_then(|told| told.update),
            Some(AvailableUpdateDto {
                version: "0.4.0".to_owned(),
                release_notes: "https://github.com/nledford/Ladderback/releases/tag/v0.4.0"
                    .to_owned(),
            })
        );
        let without_build: SessionDto = serde_json::from_str(&format!(
            r#"{{"sign-in":"signed-out","exposure":"loopback",{update}}}"#
        ))
        .unwrap();
        assert_eq!(Told::of(&without_build), None);
    }

    #[test]
    fn a_signed_in_answer_is_unreadable_only_without_its_csrf_token() {
        assert_eq!(
            answer(
                200,
                r#"{"sign-in":"signed-in","idle-seconds":43200,"ends-in-seconds":60}"#
            ),
            Answer::Unreadable
        );
        assert!(
            matches!(
                answer(200, r#"{"sign-in":"signed-in","csrf-token":"abc"}"#),
                Answer::SignedIn(_)
            ),
            "a signed-in answer with its CSRF token and no limits is a session with no idle limit and no end"
        );
        assert_eq!(
            answer(200, r#"{"sign-in":"signed-out"}"#),
            Answer::Unreadable
        );
        assert_eq!(answer(200, "not json"), Answer::Unreadable);
    }

    /// The lease of a signed-in answer to a sign-in sent at
    /// [`SIGNED_IN_AT`] with `limits`, the fields after the CSRF token.
    fn lease_of(limits: &str) -> Lease {
        let body = format!(r#"{{"sign-in":"signed-in","csrf-token":"abc"{limits}}}"#);
        match answer(200, &body) {
            Answer::SignedIn(signed) => signed.lease,
            other => panic!("{body} is a live session, not {other:?}"),
        }
    }

    #[test]
    fn a_session_kept_signed_in_has_no_idle_limit_and_ends_without_a_warning() {
        let lease = lease_of(r#","ends-in-seconds":2592000"#);
        let ends_at = SIGNED_IN_AT + LIFETIME;
        assert_eq!(lease.at(SIGNED_IN_AT), Stand::Quiet { until: ends_at });
        assert_eq!(
            lease.at(SIGNED_IN_AT + IDLE + HOUR),
            Stand::Quiet { until: ends_at }
        );
        assert_eq!(lease.at(ends_at), Stand::Ended);
    }

    #[test]
    fn a_session_with_no_end_after_its_sign_in_ends_only_unused_and_warns_first() {
        let lease = lease_of(r#","idle-seconds":43200"#);
        let idle_ends = SIGNED_IN_AT + IDLE;
        assert_eq!(
            lease.at(SIGNED_IN_AT),
            Stand::Quiet {
                until: idle_ends - WARNING_LEAD_MS
            }
        );
        assert_eq!(lease.at(idle_ends - 1), Stand::Warning { until: idle_ends });
        assert_eq!(lease.at(idle_ends), Stand::Ended);
        let used = lease.used(SIGNED_IN_AT + 2 * LIFETIME);
        assert_eq!(
            used.at(SIGNED_IN_AT + 2 * LIFETIME + HOUR),
            Stand::Quiet {
                until: SIGNED_IN_AT + 2 * LIFETIME + IDLE - WARNING_LEAD_MS
            }
        );
    }

    #[test]
    fn a_session_kept_signed_in_until_sign_out_never_ends_by_the_page_s_clock() {
        let lease = lease_of("");
        for now in [
            SIGNED_IN_AT,
            SIGNED_IN_AT + IDLE,
            SIGNED_IN_AT + 20 * LIFETIME,
        ] {
            assert_eq!(lease.at(now), Stand::Quiet { until: i64::MAX });
            assert_eq!(wait_ms(now, i64::MAX), LONGEST_WAIT_MS);
        }
    }

    #[test]
    fn only_a_signed_in_answer_is_a_live_session() {
        let mut read = session(SignInStateDto::SignedOut, Some("abc"));
        read.idle_seconds = Some(60);
        read.ends_in_seconds = Some(90);
        assert_eq!(Signed::of(&read, SIGNED_IN_AT), None);
        read.sign_in = SignInStateDto::SignedIn;
        assert_eq!(
            Signed::of(&read, SIGNED_IN_AT).map(|signed| signed.lease),
            Some(Lease::new(60_000, 90_000, SIGNED_IN_AT))
        );
    }

    #[test]
    fn terms_too_long_to_count_in_milliseconds_last_as_long_as_can_be_counted() {
        assert_eq!(millis(u64::MAX), i64::MAX);
        assert_eq!(millis(u64::MAX / 1000), i64::MAX);
        assert_eq!(millis(2), 2_000);
    }

    #[test]
    fn a_session_just_signed_in_says_nothing_until_two_minutes_before_it_would_end_unused() {
        let warn_at = SIGNED_IN_AT + IDLE - WARNING_LEAD_MS;
        assert_eq!(WARNING_LEAD_MS, 120_000);
        assert_eq!(fresh().at(SIGNED_IN_AT), Stand::Quiet { until: warn_at });
        assert_eq!(fresh().at(warn_at - 1), Stand::Quiet { until: warn_at });
    }

    #[test]
    fn the_warning_lasts_from_two_minutes_before_the_idle_limit_until_it() {
        let idle_ends = SIGNED_IN_AT + IDLE;
        let warning = Stand::Warning { until: idle_ends };
        assert_eq!(fresh().at(idle_ends - WARNING_LEAD_MS), warning);
        assert_eq!(fresh().at(idle_ends - 1), warning);
        assert_eq!(fresh().at(idle_ends), Stand::Ended);
        assert_eq!(fresh().at(idle_ends + HOUR), Stand::Ended);
    }

    #[test]
    fn a_use_moves_the_idle_limit_and_an_older_one_never_takes_it_back() {
        let used = fresh().used(SIGNED_IN_AT + HOUR);
        let warn_at = SIGNED_IN_AT + HOUR + IDLE - WARNING_LEAD_MS;
        assert_eq!(
            used.at(SIGNED_IN_AT + HOUR),
            Stand::Quiet { until: warn_at }
        );
        assert_eq!(used.used(SIGNED_IN_AT), used);
    }

    #[test]
    fn a_session_its_lifetime_ends_first_ends_without_a_warning() {
        // Signed in 29 days and 13 hours ago: 11 hours of its life are left,
        // fewer than the 12 it would last unused.
        let lease = Lease::new(IDLE, 11 * HOUR, SIGNED_IN_AT);
        let ends_at = SIGNED_IN_AT + 11 * HOUR;
        assert_eq!(lease.at(SIGNED_IN_AT), Stand::Quiet { until: ends_at });
        assert_eq!(lease.at(ends_at - 1), Stand::Quiet { until: ends_at });
        assert_eq!(lease.at(ends_at), Stand::Ended);
        // Ending unused at the very moment its life ends is the same end.
        let lease = Lease::new(IDLE, IDLE, SIGNED_IN_AT);
        assert_eq!(
            lease.at(SIGNED_IN_AT + IDLE - 1),
            Stand::Quiet {
                until: SIGNED_IN_AT + IDLE
            }
        );
    }

    #[test]
    fn a_session_its_idle_limit_ends_just_before_its_lifetime_still_warns() {
        let lease = Lease::new(IDLE, IDLE + 1, SIGNED_IN_AT);
        let idle_ends = SIGNED_IN_AT + IDLE;
        assert_eq!(lease.at(idle_ends - 1), Stand::Warning { until: idle_ends });
    }

    #[test]
    fn a_timer_waits_until_the_moment_a_day_at_most_and_never_less_than_nothing() {
        assert_eq!(wait_ms(1_000, 1_500), 500);
        assert_eq!(wait_ms(1_000, 1_000), 0);
        assert_eq!(wait_ms(1_000, 0), 0);
        assert_eq!(wait_ms(0, i64::from(LONGEST_WAIT_MS)), LONGEST_WAIT_MS);
        assert_eq!(wait_ms(0, LIFETIME), LONGEST_WAIT_MS);
        assert_eq!(wait_ms(i64::MIN, i64::MAX), LONGEST_WAIT_MS);
        assert!(i64::from(LONGEST_WAIT_MS) < i64::from(i32::MAX));
    }

    #[test]
    fn a_sign_in_while_sign_in_is_off_says_so() {
        assert_eq!(answer(200, r#"{"sign-in":"off"}"#), Answer::Off);
    }

    #[test]
    fn a_refused_sign_in_is_its_code_and_arguments() {
        assert_eq!(
            answer(401, r#"{"code":"sign-in-failed"}"#),
            Answer::Refused(ErrorCode::SignInFailed, Vec::new())
        );
        assert_eq!(
            answer(
                429,
                r#"{"code":"sign-in-wait","args":{"seconds":{"type":"count","value":4}}}"#
            ),
            Answer::Refused(
                ErrorCode::SignInWait,
                vec![("seconds".to_owned(), MsgArg::Count(4))]
            )
        );
        assert_eq!(answer(503, "<html>"), Answer::Unreadable);
    }

    #[test]
    fn only_a_wrong_password_marks_the_field() {
        assert!(about_the_password(ErrorCode::SignInFailed));
        assert!(!about_the_password(ErrorCode::SignInWait));
        assert!(!about_the_password(ErrorCode::SignInUnavailable));
    }

    #[test]
    fn a_sign_out_the_database_could_not_forget_has_signed_out_all_the_same() {
        // By its wire name: the daemon's answer, as `ws` reads it.
        let unavailable = serde_json::from_str::<ErrorCode>(r#""sessions-unavailable""#);
        assert!(unavailable.is_ok_and(signed_out_all_the_same));
        assert!(!signed_out_all_the_same(ErrorCode::CsrfTokenInvalid));
        assert!(!signed_out_all_the_same(ErrorCode::OriginNotAllowed));
        assert!(!signed_out_all_the_same(ErrorCode::SignInUnavailable));
    }
}
