//! What the step-up field's own error goes through once the daemon has
//! answered a try (§4.5, "Step-up field"), decided on the host.

/// One change to the step-up field's own error.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change<W> {
    /// The error is cleared, and the field is no longer marked invalid.
    Clear,
    /// The page draws the field as it stands, so the error set next is a
    /// change a screen reader hears, even when its words are the same.
    Draw,
    /// The field shows the error `W` and is marked invalid.
    Show(W),
}

/// The changes the field's own error goes through, in order, once the daemon
/// has answered a try, where `wrong` holds the words of a refusal of what was
/// typed in the field. Every answer clears the error an earlier try left: a
/// wrong confirmation then shows its own, once the clearing is drawn, so a
/// second wrong try is heard again; any other answer (the action done,
/// refused for another reason such as a sign-in wait, or unanswered) leaves
/// the field with no error, so it never shows a wrong confirmation beside a
/// refusal that isn't one.
pub(crate) fn changes<W>(wrong: Option<W>) -> impl Iterator<Item = Change<W>> {
    let draw = wrong.is_some().then_some(Change::Draw);
    [Some(Change::Clear), draw, wrong.map(Change::Show)]
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests;
