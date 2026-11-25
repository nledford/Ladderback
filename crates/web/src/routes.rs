//! What the app's pages share (technical design §4.5, "Route changes"): the
//! title of the page a path shows, the page a path nothing serves shows, and
//! the helpers pages use. Each page has its own `<h1>`, the heading
//! focus moves to when a reader navigates to it, so it carries
//! `tabindex="-1"`. A page itself is a module of its own (technical design
//! §8.3); the route table is `main.rs`'s.

use std::sync::Arc;

use api::{ErrorCode, MsgArg, PlanSummaryDto};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use serde::Serialize;
use std::str::FromStr;

use crate::components::Link;
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::full_disk_access::describing_the_heading;
use crate::full_disk_access_warning::FullDiskAccessFound;
use crate::repositories::Sentence;
use crate::session::SignInOffNotice;
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
                        <Link to=to localizer=localizer placement=LinkPlacement::InText>
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
    } else if cfg!(feature = "hub") && path == "/dashboard" {
        msg::UI_DASHBOARD_TITLE
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
    #[cfg(feature = "hub")]
    fn the_hubs_dashboard_is_titled_with_the_product_name() {
        assert_eq!(title_of("/dashboard"), "Dashboard – Ladderback");
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
                ("Runs only when you start it".to_owned(), None)
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
            testkit::skip!(
                "this build's l10n has no host that writes instants: no `icu`, which a release builds the web UI without"
            );
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
