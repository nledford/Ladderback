use super::*;

#[test]
fn a_wrong_confirmation_clears_the_error_then_shows_its_own_once_that_is_drawn() {
    assert_eq!(
        changes(Some("Wrong password.")).collect::<Vec<_>>(),
        [Change::Clear, Change::Draw, Change::Show("Wrong password.")]
    );
}

#[test]
fn any_other_answer_clears_the_error_an_earlier_wrong_try_left() {
    // A refusal for another reason (a sign-in wait), the action done, or no
    // answer: nothing about what was typed, so the field shows no error.
    assert_eq!(changes(None::<&str>).collect::<Vec<_>>(), [Change::Clear]);
}
