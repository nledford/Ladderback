//! Asking the person at the terminal before a command goes ahead.

use std::io;

use l10n::{Localizer, MessageId};

use crate::output::to_stderr;
use crate::say;

/// The words of one question: what is asked, ending in `[y/N]`; what is
/// said when the answer is not `y`; and what is said when nobody is there
/// to answer.
pub(crate) struct Question {
    pub ask: MessageId,
    pub declined: MessageId,
    pub no_terminal: MessageId,
}

/// Whether the person said `y`: at a terminal they are asked; piped, only
/// `--yes` goes ahead.
pub(crate) fn confirmed(
    localizer: &Localizer,
    question: &Question,
    at_terminal: bool,
    yes: bool,
) -> io::Result<bool> {
    if yes {
        return Ok(true);
    }
    if !at_terminal {
        say(localizer, question.no_terminal, &[])?;
        return Ok(false);
    }
    to_stderr(&format!("{} ", localizer.message(question.ask, &[])))?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let yes = matches!(answer.trim(), "y" | "Y");
    if !yes {
        say(localizer, question.declined, &[])?;
    }
    Ok(yes)
}
