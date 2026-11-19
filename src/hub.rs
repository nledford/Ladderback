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
    Fingerprint, Hostname, HubAddress, HubRoute, PairingToken, RouteRefused, TokenId, TokenTls,
    is_loopback,
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
    HubPairing::new(
        store,
        Arc::new(FileIdentity::in_data_dir(data_dir)),
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
        Arc::new(SystemClock),
    )
}

fn runtime(localizer: &Localizer) -> io::Result<Result<tokio::runtime::Runtime, ExitCode>> {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => Ok(Ok(runtime)),
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            Ok(Err(ExitCode::FAILURE))
        }
    }
}

/// `ladderback pairing-token`: makes one token, lists the unused ones, or
/// revokes one. It writes the hub's store from its own process, so it runs
/// while the daemon is stopped (§9: one process per data folder), and it
/// never waits for the pairing.
pub(crate) fn pairing_token(ask: TokenAsk) -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, run_pairing_token(&localizer, ask))
}

fn run_pairing_token(localizer: &Localizer, ask: TokenAsk) -> io::Result<ExitCode> {
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let runtime = match runtime(localizer)? {
        Ok(runtime) => runtime,
        Err(code) => return Ok(code),
    };
    runtime.block_on(on_the_hub(localizer, &data_dir, ask))
}

async fn on_the_hub(localizer: &Localizer, data_dir: &Path, ask: TokenAsk) -> io::Result<ExitCode> {
    if let Err(error) = std::fs::create_dir_all(data_dir) {
        return refused(localizer, &error.to_string());
    }
    let _lock = match DataDirLock::acquire(data_dir) {
        Ok(lock) => lock,
        Err(LockError::InUse { .. }) => {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_DAEMON_RUNNING,
                &[("path", text(data_dir.display()))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
        Err(LockError::Unavailable { detail }) => return refused(localizer, &detail),
    };
    let store = match SqliteStore::open(&data_dir.join(DATABASE)).await {
        Ok(store) => Arc::new(store),
        Err(error) => return refused(localizer, &error.to_string()),
    };
    let pairing = hub_pairing(data_dir, Arc::clone(&store));
    let said = match ask {
        TokenAsk::Make {
            address,
            ttl_minutes,
        } => make(localizer, &pairing, &store, &address, ttl_minutes).await,
        TokenAsk::List => list(localizer, &store).await,
        TokenAsk::Revoke(id) => revoke(localizer, &pairing, &id).await,
    };
    store.close().await;
    said
}

async fn make(
    localizer: &Localizer,
    pairing: &HubPairing,
    store: &SqliteStore,
    address: &str,
    ttl_minutes: Option<i64>,
) -> io::Result<ExitCode> {
    let Ok(address) = HubAddress::parse(address) else {
        say(
            localizer,
            msg::CLI_PAIRING_TOKEN_BAD_ADDRESS,
            &[("address", text(address))],
        )?;
        return Ok(ExitCode::from(2));
    };
    // The route `pair` will take: a token it would refuse isn't made, and a
    // plain address off loopback is warned about, as `pair` refuses it.
    let route = match HubRoute::of(&address, TokenTls::Ca) {
        Ok(route) => Some(route),
        Err(RouteRefused::PlainOffLoopback) => None,
        Err(RouteRefused::Malformed) => {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_BAD_ADDRESS,
                &[("address", text(address.as_str()))],
            )?;
            return Ok(ExitCode::from(2));
        }
    };
    let life = match ttl_minutes.map(|minutes| minutes.checked_mul(60)) {
        None => None,
        Some(Some(seconds)) => Some(SignedDuration::from_secs(seconds)),
        Some(None) => {
            say(localizer, msg::CLI_PAIRING_TOKEN_BAD_TTL, &[])?;
            return Ok(ExitCode::from(2));
        }
    };
    let keys = match pairing.keys().await {
        Ok(keys) => keys,
        Err(error) => return hub_refused(localizer, &error),
    };
    // Built-in TLS isn't served yet, so every token is of a CA-checked
    // address: `https://` behind a proxy, or plain `http://` on loopback.
    let token = match pairing
        .make_token(&keys, address.clone(), TokenTls::Ca, life)
        .await
    {
        Ok(token) => token,
        Err(error) => return hub_refused(localizer, &error),
    };
    match route {
        None => say(localizer, msg::CLI_PAIRING_TOKEN_PLAIN, &[])?,
        Some(route) if is_loopback(&route.host) => {
            say(localizer, msg::CLI_PAIRING_TOKEN_LOOPBACK, &[])?;
        }
        Some(_) => {}
    }
    say(
        localizer,
        msg::CLI_PAIRING_TOKEN_HUB_KEY,
        &[("fingerprint", text(Fingerprint::of_key(keys.key())))],
    )?;
    to_stdout(&format!("{}\n", token.to_text().expose()))?;
    let id = match AwsLcSigning.public_key(token.pairing_seed()) {
        Ok(key) => TokenId::of_pairing_key(&key),
        Err(error) => return refused(localizer, &format!("{error:?}")),
    };
    let expires = match store.unused_tokens(Timestamp::now()).await {
        Ok(tokens) => tokens
            .iter()
            .find(|record| record.id() == id)
            .map(|record| record.expires_at.to_string())
            .unwrap_or_default(),
        Err(error) => return refused(localizer, &format!("{error:?}")),
    };
    say(
        localizer,
        msg::CLI_PAIRING_TOKEN_MADE,
        &[("id", text(id)), ("expires", text(expires))],
    )?;
    Ok(ExitCode::SUCCESS)
}

async fn list(localizer: &Localizer, store: &SqliteStore) -> io::Result<ExitCode> {
    let tokens = match store.unused_tokens(Timestamp::now()).await {
        Ok(tokens) => tokens,
        Err(error) => return refused(localizer, &format!("{error:?}")),
    };
    if tokens.is_empty() {
        say(localizer, msg::CLI_PAIRING_TOKEN_NONE, &[])?;
    }
    for token in tokens {
        to_stdout(&format!("{}  {}\n", token.id(), token.expires_at))?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn revoke(localizer: &Localizer, pairing: &HubPairing, id: &str) -> io::Result<ExitCode> {
    let Some(token_id) = TokenId::parse(id) else {
        say(
            localizer,
            msg::CLI_PAIRING_TOKEN_NO_SUCH,
            &[("id", text(id))],
        )?;
        return Ok(ExitCode::from(2));
    };
    match pairing.revoke_token(token_id).await {
        Ok(true) => {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_REVOKED,
                &[("id", text(token_id))],
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Ok(false) => {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_NO_SUCH,
                &[("id", text(token_id))],
            )?;
            Ok(ExitCode::FAILURE)
        }
        Err(error) => hub_refused(localizer, &error),
    }
}

fn hub_refused(localizer: &Localizer, error: &HubPairingError) -> io::Result<ExitCode> {
    let id = match error {
        HubPairingError::TokenLife => msg::CLI_PAIRING_TOKEN_BAD_TTL,
        HubPairingError::TooManyUnusedTokens => msg::CLI_PAIRING_TOKEN_TOO_MANY,
        _ => return refused(localizer, &format!("{error:?}")),
    };
    say(localizer, id, &[])?;
    Ok(ExitCode::FAILURE)
}

fn refused(localizer: &Localizer, detail: &str) -> io::Result<ExitCode> {
    say(
        localizer,
        msg::CLI_HUB_COMMAND_FAILED,
        &[("detail", text(detail))],
    )?;
    Ok(ExitCode::FAILURE)
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
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, run_pair(&localizer, ask))
}

fn run_pair(localizer: &Localizer, ask: PairAsk) -> io::Result<ExitCode> {
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let at_terminal = terminal::stdin_is_terminal();
    if at_terminal {
        to_stderr(&format!(
            "{} ",
            localizer.message(msg::CLI_PAIR_PROMPT, &[])
        ))?;
    }
    let pasted = read_pasted();
    if at_terminal {
        to_stderr("\n")?;
    }
    let (count, mut token) = match pasted {
        Ok(line) => match PairingToken::parse_pasted(&line) {
            Ok(token) => (line.chars().count(), token),
            Err(_) => {
                say(localizer, msg::CLI_PAIR_UNREADABLE_TOKEN, &[])?;
                return Ok(ExitCode::from(2));
            }
        },
        Err(_) => {
            say(localizer, msg::CLI_PAIR_UNREADABLE_TOKEN, &[])?;
            return Ok(ExitCode::from(2));
        }
    };
    if let Some(address) = ask.address {
        let Ok(parsed) = HubAddress::parse(&address) else {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_BAD_ADDRESS,
                &[("address", text(address))],
            )?;
            return Ok(ExitCode::from(2));
        };
        token = token.with_address(parsed);
    }
    let hostname = ask.hostname.unwrap_or_else(platform::hostname);
    let Ok(hostname) = Hostname::parse(&hostname) else {
        say(
            localizer,
            msg::CLI_PAIR_BAD_HOSTNAME,
            &[("name", text(hostname))],
        )?;
        return Ok(ExitCode::from(2));
    };
    // Each kind of address has its own whole sentence, not a phrase put into
    // one.
    let read = match token.tls() {
        TokenTls::Ca => msg::CLI_PAIR_READ_CA,
        TokenTls::Pinned(_) => msg::CLI_PAIR_READ_PINNED,
    };
    say(
        localizer,
        read,
        &[
            (
                "count",
                MsgArg::Count(u64::try_from(count).unwrap_or(u64::MAX)),
            ),
            ("address", text(token.address().as_str())),
            ("fingerprint", text(Fingerprint::of_key(token.hub_key()))),
        ],
    )?;
    if !confirmed(localizer, at_terminal, ask.yes)? {
        return Ok(ExitCode::FAILURE);
    }
    let runtime = match runtime(localizer)? {
        Ok(runtime) => runtime,
        Err(code) => return Ok(code),
    };
    runtime.block_on(on_the_agent(localizer, &data_dir, token, hostname))
}

/// Whether the person said `y`: at a terminal they are asked; piped, only
/// `--yes` goes ahead.
fn confirmed(localizer: &Localizer, at_terminal: bool, yes: bool) -> io::Result<bool> {
    if yes {
        return Ok(true);
    }
    if !at_terminal {
        say(localizer, msg::CLI_PAIR_NO_TERMINAL, &[])?;
        return Ok(false);
    }
    to_stderr(&format!(
        "{} ",
        localizer.message(msg::CLI_PAIR_CONFIRM, &[])
    ))?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let yes = matches!(answer.trim(), "y" | "Y");
    if !yes {
        say(localizer, msg::CLI_PAIR_NOT_CONFIRMED, &[])?;
    }
    Ok(yes)
}

async fn on_the_agent(
    localizer: &Localizer,
    data_dir: &Path,
    token: PairingToken,
    hostname: Hostname,
) -> io::Result<ExitCode> {
    let agent_id = match platform::agent_id::load_or_create(data_dir) {
        Ok(id) => id,
        Err(error) => return refused(localizer, &error.to_string()),
    };
    let connector = WebSocketHubConnector::new();
    let pairing = AgentPairing::new(
        Arc::new(FileIdentity::in_data_dir(data_dir)),
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
    );
    let address = token.address().as_str().to_owned();
    match PairWithHub::new(pairing, Arc::new(connector))
        .pair(token, agent_id, hostname)
        .await
    {
        Ok(paired) => {
            say(
                localizer,
                msg::CLI_PAIR_PENDING,
                &[("fingerprint", text(Fingerprint::of_key(&paired.agent_key)))],
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Err(failure) => {
            let (code, args, detail) = failed(&failure, &address);
            to_stderr(&format!(
                "ladderback: {}\n",
                localizer.error(code, &args, detail.as_deref())
            ))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// An error code, its arguments, and the detail the system gave.
type Said = (ErrorCode, Vec<(&'static str, MsgArg)>, Option<String>);

/// What `pair` says a failure with. Nothing names anything of the token but
/// its address (I4).
fn failed(failure: &PairFailure, address: &str) -> Said {
    let at = || vec![("address", text(address))];
    let lost = || (ErrorCode::HubConnectionLost, vec![], None);
    match failure {
        PairFailure::Route(RouteRefused::PlainOffLoopback) => {
            (ErrorCode::PlainAddress, vec![], None)
        }
        PairFailure::Route(RouteRefused::Malformed) => (ErrorCode::HubUnreachable, at(), None),
        PairFailure::Link(LinkError::Unreachable { detail }) => {
            (ErrorCode::HubUnreachable, at(), Some(detail.clone()))
        }
        PairFailure::Link(LinkError::PinMismatch) => (ErrorCode::HubPinMismatch, at(), None),
        PairFailure::Link(LinkError::Untrusted { detail }) => {
            (ErrorCode::HubUntrusted, at(), Some(detail.clone()))
        }
        PairFailure::Link(LinkError::TlsRefused { detail }) => {
            (ErrorCode::HubTlsRefused, at(), Some(detail.clone()))
        }
        PairFailure::Link(LinkError::Closed {
            reason: Some(reason),
        }) => {
            // Before its proof, a peer names only what it can't serve: any
            // other reason would show advice nothing backs.
            wire_code(reason)
                .filter(|code| *code == ErrorCode::BindingUnavailable)
                .map_or_else(lost, |code| (code, vec![], None))
        }
        PairFailure::Link(LinkError::Closed { reason: None } | LinkError::Unreadable)
        | PairFailure::OutOfTurn
        | PairFailure::Pairing(AgentPairingError::Unsigned) => lost(),
        PairFailure::Pairing(AgentPairingError::HubKeyMismatch) => {
            (ErrorCode::HubKeyMismatch, vec![], None)
        }
        PairFailure::Pairing(AgentPairingError::HubProofInvalid) => {
            (ErrorCode::HubProofInvalid, vec![], None)
        }
        PairFailure::Pairing(AgentPairingError::BindingUnavailable) => {
            (ErrorCode::BindingUnavailable, vec![], None)
        }
        PairFailure::Pairing(AgentPairingError::Refused(refusal)) => {
            let code = api::sync::refusal_code(refusal.code.as_str());
            let args = refusal.holder.as_ref().map_or_else(Vec::new, |holder| {
                vec![
                    ("hostname", text(holder.hostname.as_str())),
                    ("last_seen", text(when(holder.last_seen.seconds()))),
                ]
            });
            (code, args, None)
        }
        PairFailure::Pairing(other) => (ErrorCode::Unknown, vec![], Some(format!("{other:?}"))),
    }
}

/// The error code a hub's wire name names, if this build knows it.
fn wire_code(name: &str) -> Option<ErrorCode> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .ok()
        .filter(|code| *code != ErrorCode::Unknown)
}

/// A moment as `pair` shows it: the date and minute in UTC.
fn when(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|seconds| Timestamp::from_second(seconds).ok())
        .map_or_else(
            || seconds.to_string(),
            |moment| moment.strftime("%Y-%m-%d %H:%M UTC").to_string(),
        )
}

/// The token as pasted, read until it reads as a token, a blank line comes
/// or it grows past what a token takes, so a token wrapped across lines
/// reads whole and none of it is left for the shell (decision 121). Echo
/// stays off, and what the terminal queued stays queued, across the lines.
fn read_pasted() -> io::Result<String> {
    let mut input = terminal::SecretInput::stdin()?;
    let mut pasted = String::new();
    loop {
        let line = input.read_line(PASTE_MAX_BYTES)?;
        let line = line.trim();
        if line.is_empty() {
            return Ok(pasted);
        }
        pasted.push_str(line);
        if pasted.len() > PASTE_MAX_BYTES || PairingToken::parse_pasted(&pasted).is_ok() {
            return Ok(pasted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_shows_as_its_date_and_minute_in_utc() {
        assert_eq!(when(1_795_000_000), "2026-11-18 11:06 UTC");
    }

    #[test]
    fn a_peer_closing_before_its_proof_names_only_what_it_cannot_serve() {
        let closed = |reason: &str| {
            PairFailure::Link(LinkError::Closed {
                reason: Some(reason.to_owned()),
            })
        };
        let (code, _, _) = failed(&closed("binding-unavailable"), "https://hub.example");
        assert_eq!(code, ErrorCode::BindingUnavailable);
        for reason in ["token-used", "hub-key-mismatch", "nonsense"] {
            let (code, _, _) = failed(&closed(reason), "https://hub.example");
            assert_eq!(code, ErrorCode::HubConnectionLost, "{reason}");
        }
    }
}
