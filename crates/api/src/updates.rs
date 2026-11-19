//! A newer release and restarting the daemon on the wire: the Updates context
//! (technical design §3.1).

use serde::{Deserialize, Serialize};

/// A newer release of Ladderback than the one the daemon runs, which the
/// header shows (decision 107): its version, and the address of its notes,
/// built from the version, never from what the release feed answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AvailableUpdateDto {
    pub version: String,
    pub release_notes: String,
}

/// What `GET /api/v1/restart` answers, and `POST` once it has taken a
/// request (decision 94): whether Settings offers a restart, which it does
/// only when a service manager runs the daemon and starts it again once it
/// exits; whether a restart asked for once the running operations finish is
/// pending, neither come nor cancelled, whatever runs (it stays pending
/// until the daemon stops); and how many operations are running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestartDto {
    pub offered: bool,
    pub pending: bool,
    pub running: u32,
}

/// The body of `POST /api/v1/restart`: when the daemon restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestartRequestDto {
    pub when: RestartWhenDto,
}

/// When a restart happens: once no operation is running, starting no queued
/// one meanwhile (`finished`); or at once, stopping the running operations
/// in order, as any stop does (`now`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartWhenDto {
    Finished,
    Now,
}
