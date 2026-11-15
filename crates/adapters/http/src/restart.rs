//! The restart route, `/api/v1/restart` (§4.2, decision NN): Settings asks
//! the daemon to restart, now or once its running operations finish, and
//! reads whether one is offered and whether one waits. The daemon only
//! stops, with the status its service manager starts it again on, so a
//! restart is offered only when a service manager runs it. The route isn't
//! in the API tokens' scope table, so only a session (or anyone, with
//! sign-in off) reaches it.

use std::sync::Arc;
use std::time::Duration;

use api::{ErrorCode, RestartDto, RestartRequestDto, RestartWhenDto};
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tokio::sync::watch;

use crate::sign_in::SessionOf;
use crate::{AppState, refused};

/// The route's path.
pub(crate) const RESTART_PATH: &str = "/api/v1/restart";

/// How often a restart that waits for the running operations looks again
/// whether any is left.
const IDLE_RECHECK: Duration = Duration::from_millis(500);

/// The daemon's operations as a restart sees them: how many are running,
/// and the hold that starts no new one while a restart waits.
pub trait Work: Send + Sync {
    /// Starts no new operation from now on, neither a queued backup nor a
    /// maintenance run: a restart waits for the running ones, and the queued
    /// ones wait for the daemon that starts next.
    fn hold(&self);
    /// Lets go of the hold, as a restart cancelled does: the queued
    /// operations start again.
    fn release(&self);
    /// How many operations are running.
    fn running(&self) -> usize;
}

/// Who asked for a restart, as the log says when the daemon stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartAsker {
    /// The admin, in a session, with sign-in on.
    SignedInAdmin,
    /// Anyone who can reach the daemon, with sign-in off.
    SignInOff,
}

/// A restart asked for: when, by whom, and, for one that waits, whether it
/// has come (the running operations finished and the daemon is stopping),
/// after which a cancel is too late.
#[derive(Clone, Copy, Debug)]
struct Asked {
    when: RestartWhenDto,
    asker: RestartAsker,
    come: bool,
}

/// Whether the daemon offers a restart, and the restart asked for, if any.
/// Clones share what was asked.
#[derive(Clone)]
pub struct Restart {
    offered: bool,
    work: Arc<dyn Work>,
    asked: Arc<watch::Sender<Option<Asked>>>,
}

/// No operations: what a daemon that offers no restart counts.
struct NoWork;

impl Work for NoWork {
    fn hold(&self) {}
    fn release(&self) {}
    fn running(&self) -> usize {
        0
    }
}

impl Default for Restart {
    /// Offered by no service manager, with no operations to count.
    fn default() -> Self {
        Self::new(false, Arc::new(NoWork))
    }
}

impl Restart {
    /// A restart, `offered` when a service manager runs the daemon, which
    /// waits for `work`'s running operations when asked to.
    pub fn new(offered: bool, work: Arc<dyn Work>) -> Self {
        Self {
            offered,
            work,
            asked: Arc::new(watch::channel(None).0),
        }
    }

    /// Takes a request: a restart now replaces one that waits, and a
    /// request to wait leaves one asked for now as it is, so a second
    /// request is harmless. Waiting first holds the work, so no new
    /// operation starts between the count and the restart. The hold and the
    /// ask change together, under the channel's lock, so a cancel can't
    /// come between them.
    fn ask(&self, when: RestartWhenDto, asker: RestartAsker) {
        self.asked.send_if_modified(|asked| match asked {
            Some(Asked {
                when: RestartWhenDto::Now,
                ..
            }) => false,
            Some(Asked {
                when: RestartWhenDto::Finished,
                ..
            }) if when == RestartWhenDto::Finished => false,
            _ => {
                if when == RestartWhenDto::Finished {
                    self.work.hold();
                }
                *asked = Some(Asked {
                    when,
                    asker,
                    come: false,
                });
                true
            }
        });
    }

    /// Cancels a restart that waits, and lets go of its hold, so the queued
    /// operations start again; says whether there was one. A restart asked
    /// for now, or one that waited and has come, is already under way and
    /// stays, hold and all; with none asked for there is nothing to cancel.
    fn cancel(&self) -> bool {
        self.asked.send_if_modified(|asked| match asked {
            Some(Asked {
                when: RestartWhenDto::Finished,
                come: false,
                ..
            }) => {
                *asked = None;
                self.work.release();
                true
            }
            _ => false,
        })
    }

    /// What the route says: offered, pending (a restart asked for once the
    /// running operations finish that hasn't come or been cancelled,
    /// whatever runs, so Settings says it waits until the daemon stops), and
    /// the running count.
    fn told(&self) -> RestartDto {
        let running = self.work.running();
        let waits = matches!(
            *self.asked.borrow(),
            Some(Asked {
                when: RestartWhenDto::Finished,
                ..
            })
        );
        RestartDto {
            offered: self.offered,
            pending: waits,
            running: u32::try_from(running).unwrap_or(u32::MAX),
        }
    }

    /// Returns once the daemon is to restart, with who asked: at once for a
    /// restart now, and for one that waits once no operation is running,
    /// marking it come under the channel's lock, so no cancel lands between
    /// the count and the stop. Never when none is asked for.
    pub async fn due(&self) -> RestartAsker {
        let mut asked = self.asked.subscribe();
        loop {
            // Marks what is asked now as seen before reading it, so a change
            // after the read wakes `changed()` below.
            drop(asked.borrow_and_update());
            let mut due = None;
            let mut waiting = false;
            // Marking a restart come changes nothing a reader acts on, so it
            // notifies no one (`false`).
            self.asked.send_if_modified(|asked| {
                match asked {
                    Some(Asked {
                        when: RestartWhenDto::Now,
                        asker,
                        ..
                    }) => due = Some(*asker),
                    Some(Asked {
                        when: RestartWhenDto::Finished,
                        asker,
                        come,
                    }) => {
                        if *come || self.work.running() == 0 {
                            *come = true;
                            due = Some(*asker);
                        } else {
                            waiting = true;
                        }
                    }
                    None => {}
                }
                false
            });
            if let Some(asker) = due {
                return asker;
            }
            tokio::select! {
                // `self` holds the sender, so this never ends in an error.
                _ = asked.changed() => {}
                () = tokio::time::sleep(IDLE_RECHECK), if waiting => {}
            }
        }
    }
}

/// `GET /api/v1/restart`: whether a restart is offered, whether one waits,
/// and how many operations are running.
pub(crate) async fn read(State(state): State<AppState>) -> Response {
    axum::Json(state.restart.told()).into_response()
}

/// `POST /api/v1/restart`: asks the daemon to restart, now or once its
/// running operations finish, and answers 202 with what the route then says.
/// Refused with `restart-unavailable` when no service manager runs the
/// daemon to start it again.
pub(crate) async fn ask(
    State(state): State<AppState>,
    session: Option<axum::Extension<SessionOf>>,
    body: Result<axum::Json<RestartRequestDto>, JsonRejection>,
) -> Response {
    if !state.restart.offered {
        return refused(StatusCode::CONFLICT, ErrorCode::RestartUnavailable);
    }
    let Ok(axum::Json(RestartRequestDto { when })) = body else {
        return refused(StatusCode::BAD_REQUEST, ErrorCode::InvalidBody);
    };
    let asker = asker_of(session.as_ref());
    state.restart.ask(when, asker);
    let told = state.restart.told();
    if told.pending && told.running > 0 {
        log::info!(
            "a restart was asked for ({}); it waits for {} running operations to finish, and starts no new one meanwhile",
            asked_by(asker),
            told.running
        );
    }
    (StatusCode::ACCEPTED, axum::Json(told)).into_response()
}

/// `DELETE /api/v1/restart`: cancels a restart that waits for the running
/// operations, so the queued ones start again, and answers with what the
/// route then says. A restart asked for now, or one that waited and has
/// come, is already under way and stays.
/// Refused with `restart-unavailable` where none is offered.
pub(crate) async fn cancel(
    State(state): State<AppState>,
    session: Option<axum::Extension<SessionOf>>,
) -> Response {
    if !state.restart.offered {
        return refused(StatusCode::CONFLICT, ErrorCode::RestartUnavailable);
    }
    if state.restart.cancel() {
        log::info!(
            "the restart that waited was cancelled ({}); queued operations start again",
            cancelled_by(asker_of(session.as_ref()))
        );
    }
    axum::Json(state.restart.told()).into_response()
}

/// Who asked for a restart, as the log says it.
pub fn asked_by(asker: RestartAsker) -> &'static str {
    match asker {
        RestartAsker::SignedInAdmin => "restart asked by the signed-in admin",
        RestartAsker::SignInOff => "restart asked with sign-in off",
    }
}

/// Who sends a request: the admin in a session, or anyone with sign-in off.
fn asker_of(session: Option<&axum::Extension<SessionOf>>) -> RestartAsker {
    match session {
        Some(_) => RestartAsker::SignedInAdmin,
        None => RestartAsker::SignInOff,
    }
}

/// Who cancelled a restart, as the log says it.
fn cancelled_by(asker: RestartAsker) -> &'static str {
    match asker {
        RestartAsker::SignedInAdmin => "by the signed-in admin",
        RestartAsker::SignInOff => "with sign-in off",
    }
}

#[cfg(test)]
mod tests;
