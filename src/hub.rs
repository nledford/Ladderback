//! The hub's terminal commands (§16, decision 121, "From a terminal"):
//! `pairing-token` on the hub, and `pair` on the agent. Only a build with
//! the `hub` feature has them.

use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::pairing::{
    AgentPairing, AgentPairingError, HubPairing, HubPairingError, PairFailure, PairWithHub,
};
use app::ports::{AgentRegistry as _, LinkError, Signing as _};
use domain::fleet::{
    Fingerprint, Hostname, HubAddress, PairingToken, RouteRefused, TokenId, TokenTls,
};
use domain::{SignedDuration, Timestamp};
use l10n::{Localizer, PosixEnv, msg};
use platform::data_dir_lock::{DataDirLock, LockError};
use platform::identity::FileIdentity;
use platform::random::OsRandom;
use platform::{SystemClock, dirs, terminal};
use signing_adapter::AwsLcSigning;
use store::SqliteStore;
use tls_adapter::WebSocketHubConnector;

use crate::output::{to_stderr, to_stdout};
use crate::{finish, say, text};

/// The database's file in the data folder.
const DATABASE: &str = "ladderback.db";

/// The most bytes a pasted token's line may have: a token is at most a few
/// hundred characters, and a paste may carry spaces and quotes around it.
const PASTE_MAX_BYTES: usize = 2048;

/// What `pairing-token` was asked to do.
pub(crate) enum TokenAsk {
    Make {
        address: String,
        ttl_minutes: Option<i64>,
    },
    List,
    Revoke(String),
}

/// The hub's pairing over `store` and the key files in `data_dir`.
pub(crate) fn hub_pairing(data_dir: &Path, store: Arc<SqliteStore>) -> HubPairing {
        todo!()
    }

fn runtime(localizer: &Localizer) -> io::Result<Result<tokio::runtime::Runtime, ExitCode>> {
        todo!()
    }

/// `ladderback pairing-token`: makes one token, lists the unused ones, or
/// revokes one. It writes the hub's store from its own process, so it runs
/// while the daemon is stopped (§9: one process per data folder), and it
/// never waits for the pairing.
pub(crate) fn pairing_token(ask: TokenAsk) -> ExitCode {
        todo!()
    }

fn run_pairing_token(localizer: &Localizer, ask: TokenAsk) -> io::Result<ExitCode> {
        todo!()
    }

async fn on_the_hub(localizer: &Localizer, data_dir: &Path, ask: TokenAsk) -> io::Result<ExitCode> {
        todo!()
    }

async fn make(
    localizer: &Localizer,
    pairing: &HubPairing,
    store: &SqliteStore,
    address: &str,
    ttl_minutes: Option<i64>,
) -> io::Result<ExitCode> {
        todo!()
    }

async fn list(localizer: &Localizer, store: &SqliteStore) -> io::Result<ExitCode> {
        todo!()
    }

async fn revoke(localizer: &Localizer, pairing: &HubPairing, id: &str) -> io::Result<ExitCode> {
        todo!()
    }

fn is_loopback(address: &HubAddress) -> bool {
        todo!()
    }

fn hub_refused(localizer: &Localizer, error: &HubPairingError) -> io::Result<ExitCode> {
        todo!()
    }

fn refused(localizer: &Localizer, detail: &str) -> io::Result<ExitCode> {
        todo!()
    }

/// What `pair` was asked: the address to use instead of the token's, the
/// hostname to pair under instead of the machine's, and whether a pairing
/// with no terminal to ask on goes ahead.
pub(crate) struct PairAsk {
    pub(crate) address: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) yes: bool,
}

/// `ladderback pair`: reads a token at the terminal without echo, or as
/// standard input, says what it read, and once the person confirms pairs
/// this machine with the hub, which holds it pending.
pub(crate) fn pair(ask: PairAsk) -> ExitCode {
        todo!()
    }

fn run_pair(localizer: &Localizer, ask: PairAsk) -> io::Result<ExitCode> {
        todo!()
    }

/// Whether the person said `y`: at a terminal they are asked; piped, only
/// `--yes` goes ahead.
fn confirmed(localizer: &Localizer, at_terminal: bool, yes: bool) -> io::Result<bool> {
        todo!()
    }

async fn on_the_agent(
    localizer: &Localizer,
    data_dir: &Path,
    token: PairingToken,
    hostname: Hostname,
) -> io::Result<ExitCode> {
        todo!()
    }

/// An error code, its arguments, and the detail the system gave.
type Said = (ErrorCode, Vec<(&'static str, MsgArg)>, Option<String>);

/// What `pair` says a failure with. Nothing names anything of the token but
/// its address (I4).
fn failed(failure: &PairFailure, address: &str) -> Said {
        todo!()
    }

/// The error code a hub's wire name names, if this build knows it.
fn wire_code(name: &str) -> Option<ErrorCode> {
        todo!()
    }
