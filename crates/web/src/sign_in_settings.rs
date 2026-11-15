//! The decisions behind Settings' "Signing in" section (security §10.1,
//! decision 70): the six choices of **Stay signed in for**, shortest first,
//! the words each is shown in, the name the wire and the radio button give
//! each, the request a save sends, and the steps the page takes on the
//! daemon's answer to a save or to **Sign out everywhere**. Host-tested
//! here; the DOM glue that holds the signals, sends the requests and takes
//! the steps is `settings/sign_in.rs`.

use api::{SessionLengthDto, SignInSettingsDto};
use l10n::{MessageId, msg};

use crate::repositories::Sentence;

/// The route that reads and sets the sign-in settings, from the API's root.
pub(crate) const SETTINGS_PATH: &str = "/api/v1/settings/sign-in";

/// The `id` of the section's heading.
pub(crate) const HEADING_ID: &str = "signing-in-heading";

/// The `name` of the radio group of **Stay signed in for**, and the start of
/// each of its radio buttons' `id`.
pub(crate) const LENGTH_GROUP: &str = "session-length";

/// The choices of **Stay signed in for**, in the order the section offers
/// them: shortest first.
pub(crate) const LENGTHS: &[SessionLengthDto] = &[];

/// The words a choice is shown in.
pub(crate) fn length_label(length: SessionLengthDto) -> MessageId {
    let _ = length;
    todo!()
}

/// The name the wire gives a choice, which is also its radio button's value.
pub(crate) fn length_value(length: SessionLengthDto) -> &'static str {
    let _ = length;
    todo!()
}

/// The choice a radio button's value names, if it names one.
fn length_of(value: &str) -> Option<SessionLengthDto> {
    let _ = value;
    todo!()
}

/// The radio buttons of **Stay signed in for**, in order: each choice's
/// value and its words, worded by `say`.
pub(crate) fn options(say: impl Fn(MessageId) -> String) -> Vec<(&'static str, String)> {
    let _ = say;
    todo!()
}

/// The request that saves the radio button whose value is `chosen`, or
/// none when it names no choice (before the settings are read).
pub(crate) fn request(chosen: &str) -> Option<SignInSettingsDto> {
    let _ = (chosen, length_of);
    todo!()
}

/// What the section asked of the daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Save the choice of **Stay signed in for**.
    Save,
    /// End every session: **Sign out everywhere**.
    SignOutEverywhere,
}

/// What the daemon answered a [`Change`] with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// It is done.
    Done,
    /// The daemon refused it, as `Sentence` says.
    Refused(Sentence),
    /// No answer came.
    Unreachable,
}

/// A step the page takes on an [`Answer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Say the choice is saved, on the page and to a screen reader.
    SaySaved,
    /// Show the sign-in page: the daemon ended every session, this page's
    /// too, which the page has already forgotten.
    SignIn,
    /// Say this problem: below the save button, or in the dialog.
    Show(Sentence),
}

/// The steps the page takes on `answer` to `change`, worded by `say`. A
/// reader may leave Settings before the answer comes: then the section is
/// no longer `shown` and has nothing left to say, but a done Sign out
/// everywhere still shows the sign-in page, since the daemon ended this
/// page's session too.
pub(crate) fn steps(
    change: Change,
    answer: Answer,
    shown: bool,
    say: impl Fn(MessageId) -> String,
) -> Vec<Step> {
    let _ = (change, answer, shown, say);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every choice the wire knows, shortest first. A new one fails to
    /// compile in `known` until it is listed here.
    const EVERY: [SessionLengthDto; 6] = [
        SessionLengthDto::UntilBrowserCloses,
        SessionLengthDto::SevenDays,
        SessionLengthDto::ThirtyDays,
        SessionLengthDto::NinetyDays,
        SessionLengthDto::OneYear,
        SessionLengthDto::UntilSignOut,
    ];

    fn known(length: SessionLengthDto) -> bool {
        match length {
            SessionLengthDto::UntilBrowserCloses
            | SessionLengthDto::SevenDays
            | SessionLengthDto::ThirtyDays
            | SessionLengthDto::NinetyDays
            | SessionLengthDto::OneYear
            | SessionLengthDto::UntilSignOut => EVERY.contains(&length),
        }
    }

    #[test]
    fn every_choice_is_offered_once_shortest_first() {
        assert!(EVERY.into_iter().all(known));
        assert_eq!(LENGTHS, &EVERY[..]);
    }

    #[test]
    fn a_choice_s_value_is_its_name_on_the_wire() {
        for length in EVERY {
            let wire = serde_json::to_string(&length).unwrap();
            assert_eq!(wire, format!("\"{}\"", length_value(length)), "{length:?}");
        }
    }

    #[test]
    fn a_choice_is_read_back_from_its_value() {
        for length in EVERY {
            assert_eq!(length_of(length_value(length)), Some(length));
        }
    }

    #[test]
    fn a_value_that_names_no_choice_is_none() {
        assert_eq!(length_of(""), None);
        assert_eq!(length_of("2-days"), None);
        assert_eq!(length_of("30 days"), None);
    }

    #[test]
    fn each_choice_has_words_of_its_own() {
        assert_eq!(
            length_label(SessionLengthDto::UntilBrowserCloses),
            msg::UI_SESSION_LENGTH_UNTIL_BROWSER_CLOSES
        );
        assert_eq!(
            length_label(SessionLengthDto::SevenDays),
            msg::UI_SESSION_LENGTH_7_DAYS
        );
        assert_eq!(
            length_label(SessionLengthDto::ThirtyDays),
            msg::UI_SESSION_LENGTH_30_DAYS
        );
        assert_eq!(
            length_label(SessionLengthDto::NinetyDays),
            msg::UI_SESSION_LENGTH_90_DAYS
        );
        assert_eq!(
            length_label(SessionLengthDto::OneYear),
            msg::UI_SESSION_LENGTH_1_YEAR
        );
        assert_eq!(
            length_label(SessionLengthDto::UntilSignOut),
            msg::UI_SESSION_LENGTH_UNTIL_SIGN_OUT
        );
    }

    /// What `options` and `steps` ask the catalog for, as text a test can
    /// compare.
    fn said(id: MessageId) -> String {
        format!("{id:?}")
    }

    fn refused() -> Answer {
        Answer::Refused(Sentence::new("refused", None))
    }

    fn unreachable() -> Step {
        Step::Show(Sentence::new(&said(msg::UI_SESSION_UNREACHABLE), None))
    }

    #[test]
    fn the_radio_buttons_are_every_choice_in_order_with_its_value_and_words() {
        let expected = EVERY
            .into_iter()
            .map(|length| (length_value(length), said(length_label(length))))
            .collect::<Vec<_>>();
        assert_eq!(options(said), expected);
    }

    #[test]
    fn a_save_sends_the_choice_its_radio_button_names() {
        for length in EVERY {
            assert_eq!(
                request(length_value(length)),
                Some(SignInSettingsDto {
                    session_length: length
                })
            );
        }
    }

    #[test]
    fn nothing_is_sent_before_a_choice_is_read() {
        assert_eq!(request(""), None);
        assert_eq!(request("2-days"), None);
    }

    #[test]
    fn a_done_save_says_it_is_saved_while_the_section_is_shown() {
        assert_eq!(
            steps(Change::Save, Answer::Done, true, said),
            vec![Step::SaySaved]
        );
    }

    #[test]
    fn a_refused_or_unanswered_change_is_said_while_the_section_is_shown() {
        for change in [Change::Save, Change::SignOutEverywhere] {
            assert_eq!(
                steps(change, refused(), true, said),
                vec![Step::Show(Sentence::new("refused", None))],
                "{change:?}"
            );
            assert_eq!(
                steps(change, Answer::Unreachable, true, said),
                vec![unreachable()],
                "{change:?}"
            );
        }
    }

    #[test]
    fn a_done_sign_out_everywhere_shows_the_sign_in_page_even_after_the_reader_left() {
        for shown in [true, false] {
            assert_eq!(
                steps(Change::SignOutEverywhere, Answer::Done, shown, said),
                vec![Step::SignIn],
                "shown: {shown}"
            );
        }
    }

    #[test]
    fn once_the_reader_left_settings_a_save_or_a_failed_sign_out_everywhere_says_nothing() {
        for answer in [Answer::Done, refused(), Answer::Unreachable] {
            assert_eq!(steps(Change::Save, answer, false, said), vec![], "save");
        }
        for answer in [refused(), Answer::Unreachable] {
            assert_eq!(
                steps(Change::SignOutEverywhere, answer, false, said),
                vec![],
                "sign out everywhere"
            );
        }
    }
}
