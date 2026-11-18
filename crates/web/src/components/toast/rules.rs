//! The toast's rules (technical design §4.5): its tone's icon, the word
//! that names it, how it is said and drawn, and how long it stays. They are
//! host-decidable, so they are tested here; `components/toast.rs` applies
//! them to the DOM.

use std::time::Duration;

use l10n::MessageId;
use lepticons_data::LucideGlyph;

use crate::announcer::Politeness;
use crate::components::alert::rules::AlertTone;
use crate::variants::{StatusTone, Tone};

/// How long a toast that goes by itself stays once nothing holds it.
pub(crate) const TOAST_MS: u64 = 6_000;

/// The tones a toast is drawn in, the alert's (components §4.5): each has
/// the alert's icon for it and the word that names it, so one tone has one
/// shape across the set. `Secondary` says no tone and `Inherit` would draw
/// the edge in the text's color, so neither is offered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastTone {
    /// Information. The default.
    #[default]
    Primary,
    /// Something the reader did ended well.
    Success,
    /// Something the reader did ended, but needs a look.
    Warning,
    /// Something the reader asked for failed.
    Error,
}

impl ToastTone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Primary, Self::Success, Self::Warning, Self::Error];

    /// The alert's tone of the same name, whose icon and word it shares.
    fn alert(self) -> AlertTone {
        match self {
            Self::Primary => AlertTone::Primary,
            Self::Success => AlertTone::Success,
            Self::Warning => AlertTone::Warning,
            Self::Error => AlertTone::Error,
        }
    }

    /// Its tone in the shared vocabulary, whose class sets the `--tone` its
    /// edge takes.
    pub fn tone(self) -> Tone {
        self.alert().tone()
    }

    /// The Lucide glyph its icon shows.
    pub fn glyph(self) -> LucideGlyph {
        self.alert().glyph()
    }

    /// The word that names its icon for a screen reader ("Error"), so the
    /// tone is heard before the words.
    pub fn label(self) -> MessageId {
        self.alert().label()
    }

    /// How the announcer says it: politely, and assertively only for a
    /// failure of what the reader asked for, which needs them now.
    pub fn politeness(self) -> Politeness {
        match self {
            Self::Error => Politeness::Assertive,
            Self::Primary | Self::Success | Self::Warning => Politeness::Polite,
        }
    }

    /// How long it stays once nothing holds it, or `None` when it needs the
    /// reader, a warning or a failure, and so stays until dismissed.
    pub fn lifetime(self) -> Option<Duration> {
        match self {
            Self::Primary | Self::Success => Some(Duration::from_millis(TOAST_MS)),
            Self::Warning | Self::Error => None,
        }
    }

    /// Its fill and words: a status tone's pair (decision 108), information
    /// on Ledger Gray in the text's color.
    fn fill(self) -> &'static str {
        match self {
            Self::Primary => "bg-surface text-fg",
            Self::Success => StatusTone::Success.pair(),
            Self::Warning => StatusTone::Warning.pair(),
            Self::Error => StatusTone::Error.pair(),
        }
    }
}

/// How a toast is drawn, in Tailwind utilities (decision 117), logical ones
/// only: edged 1px all round in its tone (`--tone`) with the small corner,
/// its icon, words and Dismiss button on one line that wraps its words.
/// Under forced colors the fill is dropped, and the edge, in the system's
/// text color, keeps its box in view.
const LOOK: &str = "flex items-start gap-2 my-2 py-2 px-3 rounded-(--radius-small) \
    border border-solid border-(--tone) forced-colors:border-[color:CanvasText]";

/// The classes of the toast: the toast's own, which names it, its tone's,
/// its fill and words, and its look.
pub fn toast_class(tone: ToastTone) -> String {
    format!("toast {} {} {LOOK}", tone.tone().class(), tone.fill())
}

/// What holds a toast on screen while the reader is on it (2.2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// The pointer rests on it.
    Pointer,
    /// Focus is in it.
    Focus,
}

/// A wait the page starts: once `after` has passed, the toast goes if
/// [`Lifetime::expires`] still says so of `token`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timer {
    pub token: u64,
    pub after: Duration,
}

/// A toast's time on screen: one toast at a time, a new one in the place of
/// the one shown. Its wait runs only while nothing holds it and starts over
/// whole once both holds have let go; every new wait voids the one before.
#[derive(Debug, Default)]
pub struct Lifetime {
    /// How long the toast shown stays once nothing holds it.
    lifetime: Option<Duration>,
    pointer: bool,
    focus: bool,
    /// The token of the wait running, if one is.
    running: Option<u64>,
    /// The last token handed out.
    last: u64,
}

impl Lifetime {
    /// A toast arrives, in the place of any shown: the wait it needs, if it
    /// goes by itself and nothing holds it.
    pub fn arrive(&mut self, tone: ToastTone) -> Option<Timer> {
        self.lifetime = tone.lifetime();
        self.start()
    }

    /// The pointer or focus is on the toast: its wait stops.
    pub fn hold(&mut self, hold: Hold) {
        *self.held(hold) = true;
        self.running = None;
    }

    /// The pointer or focus has left the toast: once neither is on it, the
    /// wait it needs, whole.
    pub fn release(&mut self, hold: Hold) -> Option<Timer> {
        if !std::mem::take(self.held(hold)) {
            return None;
        }
        self.start()
    }

    /// Whether the wait of `token` is the one running, so the toast goes.
    pub fn expires(&self, token: u64) -> bool {
        self.running == Some(token)
    }

    /// The toast went: nothing is shown, so nothing holds it or waits.
    pub fn gone(&mut self) {
        *self = Self {
            last: self.last,
            ..Self::default()
        };
    }

    fn held(&mut self, hold: Hold) -> &mut bool {
        match hold {
            Hold::Pointer => &mut self.pointer,
            Hold::Focus => &mut self.focus,
        }
    }

    /// A new wait, voiding the one before, unless the toast stays or is held.
    fn start(&mut self) -> Option<Timer> {
        self.running = None;
        let after = self.lifetime.filter(|_| !self.pointer && !self.focus)?;
        self.last = self.last.wrapping_add(1);
        self.running = Some(self.last);
        Some(Timer {
            token: self.last,
            after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
