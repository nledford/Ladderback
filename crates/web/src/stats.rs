//! A repository's stats on the Snapshots page (technical design §4.4, §16
//! decision 104), decided on the host: the route, the rows of the table, when
//! the reading shown was taken, and what the section holds and says while a
//! read is out and once it is answered. `snapshots.rs` is its glue.

use std::str::FromStr;

use api::{IanaZone, MsgArg, RepositoryStatsDto, StatsReadingDto};
use l10n::{MessageId, msg};

use crate::announcer::Politeness;
use crate::maintenance::Refusal;
use crate::wording::Words;

/// The route that tells a repository's stats as last read (`GET`), and
/// reads them now (`POST`).
pub(crate) fn stats_path(alias: &str) -> String {
    format!("/api/v1/repositories/{alias}/stats")
}

/// One row of the stats table: what it is, and its figure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) header: MessageId,
    pub(crate) figure: Words,
}

fn words(id: MessageId, args: Vec<(&'static str, MsgArg)>) -> Words {
    Words { id, args }
}

fn row(header: MessageId, figure: Words) -> Row {
    Row { header, figure }
}

/// The rows of the stats table, in the order the figures are read: the
/// snapshots, their total size, the size stored and what compression saved.
/// A repository that stores nothing has no compression to tell, and says so.
pub(crate) fn rows(reading: &StatsReadingDto) -> Vec<Row> {
    let size = |bytes| words(msg::UI_STATS_SIZE, vec![("bytes", MsgArg::Bytes(bytes))]);
    let compression = match reading.compression_saved_percent {
        Some(percent) => words(
            msg::UI_STATS_SAVED,
            vec![("percent", MsgArg::Count(u64::from(percent)))],
        ),
        None => words(msg::UI_STATS_SAVED_NONE, Vec::new()),
    };
    vec![
        row(
            msg::UI_STATS_SNAPSHOTS,
            words(
                msg::UI_STATS_COUNT,
                vec![("count", MsgArg::Count(reading.snapshots))],
            ),
        ),
        row(msg::UI_STATS_TOTAL_SIZE, size(reading.total_size)),
        row(msg::UI_STATS_STORED_SIZE, size(reading.stored_size)),
        row(msg::UI_STATS_COMPRESSION, compression),
    ]
}

/// When the reading was taken, read in the viewer's zone `zone` (§8.4); none
/// when the daemon sent no time the page can read, rather than a guess.
pub(crate) fn taken(reading: &StatsReadingDto, zone: Option<&IanaZone>) -> Option<Words> {
    let at = jiff::Timestamp::from_str(&reading.taken_at).ok()?;
    Some(words(
        msg::UI_STATS_TAKEN,
        vec![(
            "when",
            MsgArg::Timestamp {
                utc_ms: at.as_millisecond(),
                zone: zone.cloned(),
            },
        )],
    ))
}

/// What the section shows in place of its table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// Nothing yet: the first look at the stats is on its way.
    Loading,
    /// The stats were never read.
    Never,
    /// The table of the reading.
    Table,
    /// No line: the looks so far were refused or got no answer, so whether
    /// the stats were ever read isn't known.
    Nothing,
}

/// What the section knows of the reading kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kept {
    /// No look has been answered yet.
    #[default]
    Loading,
    /// A look, or a read, told it.
    Known,
    /// The looks so far were refused or got no answer.
    Unknown,
}

/// What the section announces, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) politeness: Politeness,
    pub(crate) words: Words,
}

/// What the stats section holds: the reading it shows, what it knows of the
/// one kept, whether a read is out, and the refusal to show. Each change
/// says what the section then announces, so what is shown and said is
/// decided here and not where the elements are. Each look is numbered when
/// it is sent, so the answer of one sent before a read was answered, which
/// can arrive after it, never replaces the read's newer reading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatsPanel {
    pub(crate) latest: Option<StatsReadingDto>,
    kept: Kept,
    pub(crate) reading: bool,
    pub(crate) refusal: Option<Refusal>,
    /// The refusal shown is a look's, which a later look's answer clears.
    look_refused: bool,
    /// How many looks were sent.
    looks: u64,
    /// The looks sent before a read was last answered.
    looks_before_read: u64,
}

impl StatsPanel {
    /// What the section shows in place of its table.
    pub(crate) fn shown(&self) -> Shown {
        match (&self.latest, self.kept) {
            (Some(_), _) => Shown::Table,
            (None, Kept::Known) => Shown::Never,
            (None, Kept::Loading) => Shown::Loading,
            (None, Kept::Unknown) => Shown::Nothing,
        }
    }

    /// A look at the last reading kept is sent: its number, which its
    /// answer comes back with.
    pub(crate) fn look(&mut self) -> u64 {
        self.looks += 1;
        self.looks
    }

    /// Whether the answer of the look numbered `look` can tell anything
    /// newer than what is shown.
    fn current(&self, look: u64) -> bool {
        !self.reading && look > self.looks_before_read
    }

    /// The look numbered `look` was answered. A look sent before a read
    /// was answered, or answered while one is out, tells nothing newer than
    /// the read, so it changes nothing shown. A refusal a look showed goes.
    pub(crate) fn looked(&mut self, look: u64, stats: RepositoryStatsDto) {
        if !self.current(look) {
            return;
        }
        self.kept = Kept::Known;
        self.latest = stats.latest;
        if std::mem::take(&mut self.look_refused) {
            self.refusal = None;
        }
    }

    /// The look numbered `look` was refused: the refusal is shown, unless a
    /// read is out, whose own answer then tells, or was answered since. What
    /// was known of the reading kept stays.
    pub(crate) fn look_refused(&mut self, look: u64, refusal: Refusal) {
        if !self.current(look) {
            return;
        }
        self.look_unanswered();
        self.refusal = Some(refusal);
        self.look_refused = true;
    }

    /// A look got no answer: the page's heading says the daemon is out of
    /// reach, and the section stops loading. What was known stays.
    pub(crate) fn look_unanswered(&mut self) {
        if self.kept == Kept::Loading {
            self.kept = Kept::Unknown;
        }
    }

    /// A person asks to read the stats now: what to announce, politely, or
    /// `None` while a read is out, which is not asked twice. The refusal
    /// shown is stale once something new is asked.
    pub(crate) fn asks(&mut self) -> Option<Said> {
        if self.reading {
            return None;
        }
        self.reading = true;
        self.refusal = None;
        Some(politely(msg::UI_STATS_READING))
    }

    /// The read was answered with `reading`, which the section shows from
    /// now on: what to announce, politely.
    pub(crate) fn read(&mut self, reading: StatsReadingDto) -> Said {
        self.reading = false;
        self.latest = Some(reading);
        self.looks_before_read = self.looks;
        politely(msg::UI_STATS_DONE)
    }

    /// The read was refused: the refusal is shown, as an alert, and the
    /// reading shown before stays. A later look leaves it shown.
    pub(crate) fn refused(&mut self, refusal: Refusal) {
        self.reading = false;
        self.refusal = Some(refusal);
        self.look_refused = false;
    }

    /// The read got no answer: the page's heading says the daemon is out of
    /// reach, and the section can be asked again.
    pub(crate) fn silent(&mut self) {
        self.reading = false;
    }
}

fn politely(id: MessageId) -> Said {
    Said {
        politeness: Politeness::Polite,
        words: words(id, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use api::ErrorCode;

    use super::*;

    fn reading(compression_saved_percent: Option<u8>) -> StatsReadingDto {
        StatsReadingDto {
            taken_at: "2026-11-15T09:00:00.123Z".to_owned(),
            snapshots: 3,
            total_size: 3000,
            stored_size: 1200,
            compression_saved_percent,
        }
    }

    fn refusal() -> Refusal {
        Refusal {
            code: ErrorCode::EngineUnavailable,
            args: Vec::new(),
            detail: Some("the repository can't be reached".to_owned()),
        }
    }

    fn kept(reading: Option<StatsReadingDto>) -> RepositoryStatsDto {
        RepositoryStatsDto { latest: reading }
    }

    #[test]
    fn the_route_names_the_repository() {
        assert_eq!(stats_path("demo"), "/api/v1/repositories/demo/stats");
    }

    #[test]
    fn the_rows_are_the_four_figures_in_order() {
        assert_eq!(
            rows(&reading(Some(50))),
            vec![
                row(
                    msg::UI_STATS_SNAPSHOTS,
                    words(msg::UI_STATS_COUNT, vec![("count", MsgArg::Count(3))])
                ),
                row(
                    msg::UI_STATS_TOTAL_SIZE,
                    words(msg::UI_STATS_SIZE, vec![("bytes", MsgArg::Bytes(3000))])
                ),
                row(
                    msg::UI_STATS_STORED_SIZE,
                    words(msg::UI_STATS_SIZE, vec![("bytes", MsgArg::Bytes(1200))])
                ),
                row(
                    msg::UI_STATS_COMPRESSION,
                    words(msg::UI_STATS_SAVED, vec![("percent", MsgArg::Count(50))])
                ),
            ]
        );
    }

    #[test]
    fn a_repository_that_stores_nothing_says_so_for_compression() {
        let rows = rows(&reading(None));
        assert_eq!(
            rows.last(),
            Some(&row(
                msg::UI_STATS_COMPRESSION,
                words(msg::UI_STATS_SAVED_NONE, Vec::new())
            ))
        );
    }

    #[test]
    fn the_reading_is_told_as_taken_in_the_viewers_zone() {
        let zone = IanaZone::try_from("Europe/Berlin".to_owned()).ok();
        assert_eq!(
            taken(&reading(None), zone.as_ref()),
            Some(words(
                msg::UI_STATS_TAKEN,
                vec![(
                    "when",
                    MsgArg::Timestamp {
                        utc_ms: 1_794_733_200_123,
                        zone,
                    },
                )],
            ))
        );
    }

    #[test]
    fn a_time_the_page_cant_read_is_left_out() {
        let mut reading = reading(None);
        reading.taken_at = "yesterday".to_owned();
        assert_eq!(taken(&reading, None), None);
    }

    fn polite(id: MessageId) -> Said {
        Said {
            politeness: Politeness::Polite,
            words: words(id, Vec::new()),
        }
    }

    /// A reading taken after [`reading`]'s.
    fn newer() -> StatsReadingDto {
        StatsReadingDto {
            taken_at: "2026-11-15T10:00:00Z".to_owned(),
            snapshots: 4,
            ..reading(Some(40))
        }
    }

    #[test]
    fn the_section_loads_until_looked_at_then_shows_the_reading_or_that_there_is_none() {
        let mut panel = StatsPanel::default();
        assert_eq!(panel.shown(), Shown::Loading);
        let look = panel.look();
        panel.looked(look, kept(None));
        assert_eq!(panel.shown(), Shown::Never);
        let look = panel.look();
        panel.looked(look, kept(Some(reading(Some(50)))));
        assert_eq!(panel.shown(), Shown::Table);
        assert_eq!(panel.latest, Some(reading(Some(50))));
    }

    #[test]
    fn a_read_is_announced_then_shown_and_announced_again_politely() {
        let mut panel = StatsPanel::default();
        let look = panel.look();
        panel.looked(look, kept(None));
        assert_eq!(panel.asks(), Some(polite(msg::UI_STATS_READING)));
        assert!(panel.reading);
        assert_eq!(panel.read(reading(Some(50))), polite(msg::UI_STATS_DONE));
        assert!(!panel.reading);
        assert_eq!(panel.latest, Some(reading(Some(50))));
        assert_eq!(panel.shown(), Shown::Table);
    }

    #[test]
    fn a_read_answered_before_any_look_shows_its_reading() {
        let mut panel = StatsPanel::default();
        panel.asks();
        panel.read(reading(None));
        assert_eq!(panel.shown(), Shown::Table);
    }

    #[test]
    fn a_second_press_while_a_read_is_out_asks_nothing() {
        let mut panel = StatsPanel::default();
        panel.asks();
        assert_eq!(panel.asks(), None);
        assert!(panel.reading);
    }

    #[test]
    fn a_look_answered_while_a_read_is_out_leaves_what_is_shown() {
        let mut panel = StatsPanel::default();
        let look = panel.look();
        panel.looked(look, kept(Some(reading(Some(50)))));
        panel.asks();
        let look = panel.look();
        panel.looked(look, kept(None));
        assert_eq!(panel.latest, Some(reading(Some(50))));
        panel.look_refused(look, refusal());
        assert_eq!(panel.refusal, None);
    }

    #[test]
    fn a_look_sent_before_a_read_was_answered_never_replaces_its_reading() {
        let mut panel = StatsPanel::default();
        let before = panel.look();
        panel.asks();
        panel.read(newer());
        panel.looked(before, kept(Some(reading(Some(50)))));
        assert_eq!(panel.latest, Some(newer()), "an older reading came back");
        panel.look_refused(before, refusal());
        assert_eq!(panel.refusal, None);
        // A look sent since tells what was kept since, a check's reading.
        let after = panel.look();
        panel.looked(after, kept(Some(reading(Some(50)))));
        assert_eq!(panel.latest, Some(reading(Some(50))));
    }

    #[test]
    fn a_refused_look_is_shown_without_saying_the_stats_were_never_read() {
        let mut panel = StatsPanel::default();
        let look = panel.look();
        panel.look_refused(look, refusal());
        assert_eq!(panel.refusal, Some(refusal()));
        assert_eq!(panel.shown(), Shown::Nothing);
        let look = panel.look();
        panel.looked(look, kept(None));
        assert_eq!(panel.refusal, None, "the look answered since clears it");
        assert_eq!(panel.shown(), Shown::Never);
    }

    #[test]
    fn a_refused_look_leaves_what_an_earlier_look_told() {
        let mut panel = StatsPanel::default();
        let look = panel.look();
        panel.looked(look, kept(None));
        let look = panel.look();
        panel.look_refused(look, refusal());
        assert_eq!(panel.shown(), Shown::Never);
    }

    #[test]
    fn an_unanswered_look_stops_the_loading_and_leaves_what_was_known() {
        let mut panel = StatsPanel::default();
        panel.look();
        panel.look_unanswered();
        assert_eq!(panel.shown(), Shown::Nothing);
        let look = panel.look();
        panel.looked(look, kept(None));
        panel.look();
        panel.look_unanswered();
        assert_eq!(panel.shown(), Shown::Never);
    }

    #[test]
    fn a_refused_read_keeps_the_reading_shown_and_can_be_asked_again() {
        let mut panel = StatsPanel::default();
        let look = panel.look();
        panel.looked(look, kept(Some(reading(Some(50)))));
        let look = panel.look();
        panel.look_refused(look, refusal());
        panel.asks();
        panel.refused(refusal());
        assert_eq!(panel.refusal, Some(refusal()));
        assert_eq!(panel.latest, Some(reading(Some(50))));
        assert!(!panel.reading);
        let look = panel.look();
        panel.looked(look, kept(Some(reading(Some(50)))));
        assert_eq!(
            panel.refusal,
            Some(refusal()),
            "a look leaves a read's refusal"
        );
        assert!(panel.asks().is_some());
        assert_eq!(panel.refusal, None, "a new read clears the refusal shown");
    }

    #[test]
    fn an_unanswered_read_can_be_asked_again() {
        let mut panel = StatsPanel::default();
        panel.asks();
        panel.silent();
        assert!(!panel.reading);
        assert!(panel.asks().is_some());
    }
}
