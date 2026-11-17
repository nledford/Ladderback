//! The toast's arrival announcement and lifetime (technical design §4.5): a
//! toast is announced politely once — assertive is only for destructive
//! results — stays on screen for [`TOAST_MS`] and goes unannounced. The
//! decisions are host-decidable, so they are tested here;
//! `components/toast.rs` applies them to the DOM.

use std::cell::Cell;
use std::time::Duration;

use crate::announcer::Politeness;

/// How long a toast stays on screen before it goes, unannounced.
pub(crate) const TOAST_MS: u64 = 3_000;

/// A toast's arrival: how it is announced and how long it stays.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arrival {
    /// Toasts are polite; only destructive results are assertive (§4.5).
    pub politeness: Politeness,
    /// How long the toast stays on screen before it goes, unannounced.
    pub shown_for: Duration,
}

/// How a toast's arrival is announced and how long it stays.
pub(crate) fn arrival() -> Arrival {
    Arrival {
        politeness: Politeness::Polite,
        shown_for: Duration::from_millis(TOAST_MS),
    }
}

/// Counts toast arrivals, so an older toast's expiry can no longer clear the
/// text a newer toast is showing: a toast that arrives while another is on
/// screen still gets its whole lifetime.
#[derive(Debug, Default)]
pub(crate) struct ToastTimers {
    generation: Cell<u64>,
}

impl ToastTimers {
    /// Marks a toast's arrival and returns its generation.
    pub(crate) fn arrive(&self) -> u64 {
        let next = self.generation.get().checked_add(1).unwrap_or(0);
        self.generation.set(next);
        next
    }

    /// Whether the toast of `generation` is still the newest one shown.
    pub(crate) fn still_current(&self, generation: u64) -> bool {
        self.generation.get() == generation
    }
}

/// The tones a toast is drawn in, each with an icon of its own and a word
/// that names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastTone {
    /// Information. The default.
    #[default]
    Primary,
    /// Something ended well.
    Success,
    /// Something needs a look.
    Warning,
    /// Something failed.
    Error,
}

impl ToastTone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Primary, Self::Success, Self::Warning, Self::Error];

    pub fn tone(self) -> crate::variants::Tone {
        todo!()
    }

    pub fn glyph(self) -> lepticons_data::LucideGlyph {
        todo!()
    }

    pub fn label(self) -> l10n::MessageId {
        todo!()
    }

    pub fn politeness(self) -> Politeness {
        todo!()
    }

    pub fn lifetime(self) -> Option<Duration> {
        todo!()
    }
}

pub fn toast_class(_tone: ToastTone) -> String {
    todo!()
}

/// What holds a toast on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Pointer,
    Focus,
}

/// A wait for the page to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timer {
    pub token: u64,
    pub after: Duration,
}

/// A toast's time on screen.
#[derive(Debug, Default)]
pub struct Lifetime;

impl Lifetime {
    pub fn arrive(&mut self, _tone: ToastTone) -> Option<Timer> {
        todo!()
    }

    pub fn hold(&mut self, _hold: Hold) {
        todo!()
    }

    pub fn release(&mut self, _hold: Hold) -> Option<Timer> {
        todo!()
    }

    pub fn expires(&self, _token: u64) -> bool {
        todo!()
    }

    pub fn gone(&mut self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toasts_arrival_is_announced_politely_and_shown_for_three_seconds() {
        let plan = arrival();
        assert_eq!(plan.politeness, Politeness::Polite);
        assert_eq!(plan.shown_for, Duration::from_millis(3_000));
    }

    #[test]
    fn an_older_toasts_expiry_no_longer_clears_the_text() {
        let timers = ToastTimers::default();
        let first = timers.arrive();
        let second = timers.arrive();
        assert!(!timers.still_current(first));
        assert!(timers.still_current(second));
    }

    use crate::components::alert::rules::AlertTone;
    use crate::variants::{StatusTone, Tone};

    const SIX_SECONDS: Duration = Duration::from_secs(6);

    #[test]
    fn a_toast_is_information_in_ladder_blue_unless_its_page_names_another_tone() {
        assert_eq!(ToastTone::default(), ToastTone::Primary);
        let tones = ToastTone::ALL.map(ToastTone::tone);
        assert_eq!(
            tones,
            [Tone::Primary, Tone::Success, Tone::Warning, Tone::Error]
        );
    }

    #[test]
    fn each_tone_shows_the_alerts_icon_for_it_named_by_the_alerts_word() {
        let alerts = [
            AlertTone::Primary,
            AlertTone::Success,
            AlertTone::Warning,
            AlertTone::Error,
        ];
        for (toast, alert) in ToastTone::ALL.into_iter().zip(alerts) {
            assert_eq!(
                (toast.glyph(), toast.label()),
                (alert.glyph(), alert.label()),
                "{toast:?}"
            );
        }
    }

    #[test]
    fn only_a_failure_is_said_assertively() {
        let said = ToastTone::ALL.map(ToastTone::politeness);
        assert_eq!(
            said,
            [
                Politeness::Polite,
                Politeness::Polite,
                Politeness::Polite,
                Politeness::Assertive
            ]
        );
    }

    #[test]
    fn information_and_success_go_after_six_seconds_and_a_warning_or_a_failure_stays() {
        let lifetimes = ToastTone::ALL.map(ToastTone::lifetime);
        assert_eq!(
            lifetimes,
            [Some(SIX_SECONDS), Some(SIX_SECONDS), None, None]
        );
    }

    #[test]
    fn a_status_toast_lies_on_its_pair_and_information_on_ledger_gray() {
        let fills = [
            "bg-surface text-fg",
            StatusTone::Success.pair(),
            StatusTone::Warning.pair(),
            StatusTone::Error.pair(),
        ];
        for (tone, fill) in ToastTone::ALL.into_iter().zip(fills) {
            let class = toast_class(tone);
            let classes: Vec<&str> = class.split_whitespace().collect();
            assert_eq!(classes.first(), Some(&"toast"), "{class}");
            assert!(classes.contains(&tone.tone().class()), "{class}");
            for utility in fill.split_whitespace() {
                assert!(classes.contains(&utility), "{class} lacks {utility}");
            }
        }
    }

    #[test]
    fn a_toast_that_goes_by_itself_goes_when_its_time_is_up() {
        let mut life = Lifetime::default();
        let timer = life
            .arrive(ToastTone::Success)
            .expect("a success goes by itself");
        assert_eq!(timer.after, SIX_SECONDS);
        assert!(life.expires(timer.token));
        life.gone();
        assert!(
            !life.expires(timer.token),
            "a toast already gone goes again"
        );
    }

    #[test]
    fn a_toast_that_stays_starts_no_timer() {
        let mut life = Lifetime::default();
        assert_eq!(life.arrive(ToastTone::Warning), None);
        assert_eq!(life.arrive(ToastTone::Error), None);
    }

    #[test]
    fn a_held_toast_waits_and_has_its_whole_time_again_once_released() {
        for hold in [Hold::Pointer, Hold::Focus] {
            let mut life = Lifetime::default();
            let first = life
                .arrive(ToastTone::Primary)
                .expect("information goes by itself");
            life.hold(hold);
            assert!(!life.expires(first.token), "{hold:?}: went while held");
            let again = life
                .release(hold)
                .expect("released, it goes by itself again");
            assert_eq!(again.after, SIX_SECONDS);
            assert!(
                !life.expires(first.token),
                "{hold:?}: the first wait still counts"
            );
            assert!(life.expires(again.token), "{hold:?}");
        }
    }

    #[test]
    fn a_toast_held_by_both_waits_until_both_let_go() {
        let mut life = Lifetime::default();
        let _ = life.arrive(ToastTone::Success);
        life.hold(Hold::Pointer);
        life.hold(Hold::Focus);
        assert_eq!(life.release(Hold::Pointer), None);
        let timer = life.release(Hold::Focus).expect("both let go");
        assert!(life.expires(timer.token));
    }

    #[test]
    fn a_release_without_a_hold_keeps_the_wait_running() {
        let mut life = Lifetime::default();
        let timer = life
            .arrive(ToastTone::Success)
            .expect("a success goes by itself");
        assert_eq!(life.release(Hold::Pointer), None);
        assert!(life.expires(timer.token));
    }

    #[test]
    fn a_new_toast_voids_the_wait_of_the_one_it_replaces() {
        let mut life = Lifetime::default();
        let older = life
            .arrive(ToastTone::Success)
            .expect("a success goes by itself");
        let newer = life
            .arrive(ToastTone::Primary)
            .expect("information goes by itself");
        assert!(!life.expires(older.token));
        assert!(life.expires(newer.token));
        let _ = life.arrive(ToastTone::Error);
        assert!(
            !life.expires(newer.token),
            "a failure went with an older toast's time"
        );
    }

    #[test]
    fn a_toast_arriving_under_a_hold_waits_for_its_release() {
        let mut life = Lifetime::default();
        let _ = life.arrive(ToastTone::Warning);
        life.hold(Hold::Focus);
        assert_eq!(life.arrive(ToastTone::Success), None);
        let timer = life
            .release(Hold::Focus)
            .expect("released, it goes by itself");
        assert!(life.expires(timer.token));
    }

    #[test]
    fn a_toast_gone_lets_go_of_its_holds() {
        let mut life = Lifetime::default();
        let _ = life.arrive(ToastTone::Error);
        life.hold(Hold::Pointer);
        life.gone();
        let timer = life
            .arrive(ToastTone::Success)
            .expect("the next goes by itself");
        assert!(life.expires(timer.token));
    }

    #[test]
    fn a_dismissed_toasts_wait_never_takes_away_the_next() {
        let mut life = Lifetime::default();
        let first = life.arrive(ToastTone::Success).expect("it goes by itself");
        life.gone();
        let next = life
            .arrive(ToastTone::Success)
            .expect("the next goes by itself");
        assert!(!life.expires(first.token));
        assert!(life.expires(next.token));
    }
}
