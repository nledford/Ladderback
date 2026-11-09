//! The keyboard focus wrap decision of the modal dialog (technical design
//! §4.5): which keypresses must move focus from one end of the dialog's
//! focusables to the other, keeping keyboard focus inside the open dialog.
//! The decision is host-decidable, so it is tested here; `components.rs`
//! applies it to the DOM.

/// Which end of the dialog's focusables keyboard focus wraps to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FocusWrap {
    /// Tab on the last focusable wraps to the first.
    First,
    /// Shift-Tab on the first focusable wraps to the last.
    Last,
}

/// Decides whether a keypress in an open dialog must wrap keyboard focus:
/// Tab on the last focusable wraps to the first, Shift-Tab on the first
/// wraps to the last, and anything else leaves focus where the browser put
/// it. A dialog with a single focusable never wraps: focus has nowhere to
/// leave to.
pub(crate) fn focus_wrap(
    key_tab: bool,
    shift: bool,
    single: bool,
    on_last: bool,
    on_first: bool,
) -> Option<FocusWrap> {
    if !key_tab || single {
        return None;
    }
    if on_last && !shift {
        Some(FocusWrap::First)
    } else if on_first && shift {
        Some(FocusWrap::Last)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_on_the_last_focusable_wraps_to_the_first() {
        assert_eq!(
            focus_wrap(true, false, false, true, false),
            Some(FocusWrap::First)
        );
    }

    #[test]
    fn shift_tab_on_the_first_focusable_wraps_to_the_last() {
        assert_eq!(
            focus_wrap(true, true, false, false, true),
            Some(FocusWrap::Last)
        );
    }

    #[test]
    fn tab_in_the_middle_of_the_dialog_wraps_nothing() {
        assert_eq!(focus_wrap(true, false, false, false, false), None);
    }

    #[test]
    fn tab_on_the_first_focusable_is_left_to_the_browser() {
        assert_eq!(focus_wrap(true, false, false, false, true), None);
    }

    #[test]
    fn shift_tab_on_the_last_focusable_is_left_to_the_browser() {
        assert_eq!(focus_wrap(true, true, false, true, false), None);
    }

    #[test]
    fn a_dialog_with_one_focusable_wraps_nothing_however_tab_is_pressed() {
        assert_eq!(focus_wrap(true, false, true, true, true), None);
        assert_eq!(focus_wrap(true, true, true, true, true), None);
    }

    #[test]
    fn a_key_that_is_not_tab_wraps_nothing() {
        assert_eq!(focus_wrap(false, false, false, true, false), None);
        assert_eq!(focus_wrap(false, true, false, false, true), None);
    }
}
