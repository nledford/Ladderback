//! The decisions behind restarting Ladderback from Settings (§4.4, decision
//! NN): what the section offers for what the daemon says, what the dialog
//! says and sends, and the wait for the daemon to come back, which reloads
//! the page only once it has seen the daemon go down and answer again, so the
//! page never reloads into the daemon that is stopping. Host-tested here; the
//! DOM glue that holds the signals, sends the requests and polls is
//! `settings.rs`.

use std::time::Duration;

use api::{MsgArg, RestartDto, RestartRequestDto, RestartWhenDto};
use l10n::{MessageId, msg};

use crate::api_tokens::Said;

/// The route that reads, asks for and cancels a restart, from the API's root.
pub(crate) const RESTART_PATH: &str = "/api/v1/restart";

/// The `id` of the section's heading, which takes focus when the control
/// that held it goes, as the restart button does once the wait begins.
pub(crate) const HEADING_ID: &str = "restart-heading";

/// The `id` of the button that opens the dialog, where focus returns once
/// the dialog closes.
pub(crate) const BUTTON_ID: &str = "restart-button";

/// The `id` of what the dialog says about the running operations, which
/// describes it.
pub(crate) const BODY_ID: &str = "restart-body";

/// How often the section reads the route again while a restart waits for the
/// running operations, so it sees the restart come or be cancelled.
pub(crate) const PENDING_EVERY: Duration = Duration::from_secs(1);

/// How often the page asks whether the daemon answers while it restarts.
pub(crate) const WAIT_EVERY: Duration = Duration::from_millis(250);

/// How long one such question may take before it counts as no answer: a
/// daemon that is stopping may hold a connection open without answering.
pub(crate) const ANSWER_PATIENCE: Duration = Duration::from_secs(2);

/// How long the page waits for the daemon to answer again before it says it
/// hasn't, and where to look; it goes on asking.
pub(crate) const NOT_BACK_AFTER_MS: i64 = 60_000;

/// What the restart section shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The first read hasn't answered.
    Loading,
    /// The first read had no answer.
    Unreachable,
    /// No service manager runs the daemon, so no restart is offered.
    Unavailable,
    /// A restart is offered, with this many operations running.
    Offered { running: u32 },
    /// A restart waits for the running operations to finish.
    Pending,
    /// The daemon has begun to restart; the page waits for it to answer
    /// again, having already seen it go down or not.
    Restarting { seen_down: bool },
}

impl Stage {
    /// What a read of the route shows; `None` is no answer.
    pub(crate) fn of(read: Option<&RestartDto>) -> Self {
        let _ = read;
        todo!()
    }

    /// What a read again shows while a restart is pending: no answer is the
    /// daemon going down for it, which the wait has then seen.
    pub(crate) fn while_pending(read: Option<&RestartDto>) -> Self {
        let _ = read;
        todo!()
    }

    /// What the section shows once the daemon has taken a request to
    /// restart `when`, answering `told`: the wait, for a restart now or one
    /// with nothing running to wait for, and otherwise what the daemon says,
    /// a restart pending.
    pub(crate) fn after_ask(when: RestartWhenDto, told: &RestartDto) -> Self {
        let _ = (when, told);
        todo!()
    }
}

/// The request that asks for a restart `when`.
pub(crate) fn request(when: RestartWhenDto) -> RestartRequestDto {
    let _ = when;
    todo!()
}

/// What the dialog says about the `running` operations, and the choices it
/// offers when there are some.
pub(crate) fn running_said(running: u32) -> Said {
    let _ = running;
    todo!()
}

/// The dialog's confirm button with `running` operations: a plain restart
/// with none, which has nothing to wait for, and otherwise one once they
/// finish. Either asks for a restart once they finish.
pub(crate) fn confirm_said(running: u32) -> Said {
    let _ = running;
    todo!()
}

/// Whether the dialog also offers a restart now, which stops the `running`
/// operations: only when some are.
pub(crate) fn offers_now(running: u32) -> bool {
    let _ = running;
    todo!()
}

/// The page's wait for the daemon to come back once it has begun to
/// restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wait {
    since_ms: i64,
    seen_down: bool,
}

/// What the page does after one question to the daemon while it waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waited {
    /// Ask again after [`WAIT_EVERY`]; `not_back` once the daemon has been
    /// waited for too long, which the page then says.
    Again { not_back: bool },
    /// The daemon went down and answers again: the page reloads.
    Reload,
}

impl Wait {
    /// A wait begun at `now_ms`, which has `seen_down` the daemon already.
    pub(crate) fn new(now_ms: i64, seen_down: bool) -> Self {
        let _ = (now_ms, seen_down);
        todo!()
    }

    /// What follows a question asked at `now_ms`, which the daemon
    /// `answered` or not. An answer before the daemon was seen down is the
    /// daemon that is stopping, so only one after it reloads the page.
    pub(crate) fn after(&mut self, answered: bool, now_ms: i64) -> Waited {
        let _ = (answered, now_ms);
        todo!()
    }
}

/// What the section says in its status for `stage`: whether a restart waits,
/// or the wait, and `not_back` once that has lasted too long. `None` says
/// nothing.
pub(crate) fn status_of(stage: Stage, not_back: bool) -> Option<MessageId> {
    let _ = (stage, not_back);
    todo!()
}

#[cfg(test)]
mod tests;
