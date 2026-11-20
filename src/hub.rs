//! The hub's terminal commands (§16, decision 121, "From a terminal"):
//! `pairing-token` on the hub, and `pair` and `hub-status` on the agent.
//! Only a build with the `hub` feature has them.

use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::pairing::{
    AgentPairing, AgentPairingError, ChallengeRefused, ConnectFailure, ConnectToHub, HubPairing,
    HubPairingError, PairFailure, PairWithHub,
};
use app::ports::{AgentRegistry as _, Identity as _, IdentityError, LinkError, Signing as _};
use domain::fleet::{
    Approval, Fingerprint, Hostname, HubAddress, HubRoute, PairingToken, RouteRefused, TokenId,
    TokenTls, WelcomeRefused, is_loopback,
};
use domain::{SignedDuration, Timestamp};
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::data_dir_lock::{DataDirLock, LockError};
use platform::identity::FileIdentity;
use platform::random::OsRandom;
use platform::{SystemClock, dirs, terminal};
use signing_adapter::AwsLcSigning;
use store::SqliteStore;
use tls_adapter::WebSocketHubConnector;

use crate::output::{refusal_line, to_stderr, to_stdout};
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
        crate::build_info::SHOWN.as_str().to_owned(),
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
    if let HubPairingError::Identity(identity) = error
        && let Some(args) = exposed(identity)
    {
        say(localizer, msg::CLI_KEY_FILE_EXPOSED, &args)?;
        return Ok(ExitCode::FAILURE);
    }
    let id = match error {
        HubPairingError::TokenLife => msg::CLI_PAIRING_TOKEN_BAD_TTL,
        HubPairingError::TooManyUnusedTokens => msg::CLI_PAIRING_TOKEN_TOO_MANY,
        _ => return refused(localizer, &format!("{error:?}")),
    };
    say(localizer, id, &[])?;
    Ok(ExitCode::FAILURE)
}

/// The arguments of `cli-key-file-exposed` when `error` refused a key file
/// other users can read or change: its name and its mode in octal, as
/// `chmod` takes it (#1137).
fn exposed(error: &IdentityError) -> Option<Vec<(&'static str, MsgArg)>> {
    match error {
        IdentityError::Exposed { file, mode } => Some(vec![
            ("file", text(file)),
            ("mode", text(format!("{mode:04o}"))),
        ]),
        _ => None,
    }
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
            if let PairFailure::Pairing(AgentPairingError::Identity(identity)) = &failure
                && let Some(args) = exposed(identity)
            {
                say(localizer, msg::CLI_KEY_FILE_EXPOSED, &args)?;
                return Ok(ExitCode::FAILURE);
            }
            let (code, args, detail) = failed(&failure, &address);
            to_stderr(&refusal_line(localizer, code, &args, detail.as_deref()))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// `ladderback hub-status`: connects to the hub this machine pinned at
/// pairing, proves who it is, and says whether the hub holds it waiting for
/// approval or has approved it.
pub(crate) fn hub_status(hostname: Option<String>) -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, run_hub_status(&localizer, hostname))
}

fn run_hub_status(localizer: &Localizer, hostname: Option<String>) -> io::Result<ExitCode> {
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let hostname = hostname.unwrap_or_else(platform::hostname);
    let Ok(hostname) = Hostname::parse(&hostname) else {
        say(
            localizer,
            msg::CLI_PAIR_BAD_HOSTNAME,
            &[("name", text(hostname))],
        )?;
        return Ok(ExitCode::from(2));
    };
    let runtime = match runtime(localizer)? {
        Ok(runtime) => runtime,
        Err(code) => return Ok(code),
    };
    runtime.block_on(connect_to_hub(localizer, &data_dir, hostname))
}

async fn connect_to_hub(
    localizer: &Localizer,
    data_dir: &Path,
    hostname: Hostname,
) -> io::Result<ExitCode> {
    // A status check writes nothing: a machine with no agent ID never paired,
    // and gets none from asking.
    let agent_id = match platform::agent_id::load(data_dir) {
        Ok(Some(id)) => id,
        Ok(None) => {
            say(localizer, msg::CLI_HUB_STATUS_NOT_PAIRED, &[])?;
            return Ok(ExitCode::FAILURE);
        }
        Err(error) => return refused(localizer, &error.to_string()),
    };
    let identity = Arc::new(FileIdentity::in_data_dir(data_dir));
    // The address a failure names; the use case reads the pin it trusts.
    let address = match identity.pin().await {
        Ok(Some(pin)) => pin.address.as_str().to_owned(),
        Ok(None) | Err(_) => String::new(),
    };
    let connecting = ConnectToHub::new(
        identity,
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
        Arc::new(SystemClock),
        Arc::new(WebSocketHubConnector::new()),
        crate::build_info::SHOWN.as_str().to_owned(),
    );
    match connecting.connect(agent_id, hostname).await {
        Ok(connected) => {
            match connected.approval {
                Approval::Pending => say(
                    localizer,
                    msg::CLI_HUB_STATUS_PENDING,
                    &[(
                        "fingerprint",
                        text(Fingerprint::of_key(&connected.agent_key)),
                    )],
                )?,
                Approval::Approved => say(localizer, msg::CLI_HUB_STATUS_APPROVED, &[])?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(ConnectFailure::NotPaired) => {
            say(localizer, msg::CLI_HUB_STATUS_NOT_PAIRED, &[])?;
            Ok(ExitCode::FAILURE)
        }
        Err(failure) => {
            if let ConnectFailure::Identity(identity) = &failure
                && let Some(args) = exposed(identity)
            {
                say(localizer, msg::CLI_KEY_FILE_EXPOSED, &args)?;
                return Ok(ExitCode::FAILURE);
            }
            let (code, args, detail) = connect_failed(&failure, &address);
            to_stderr(&refusal_line(localizer, code, &args, detail.as_deref()))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// What `hub-status` says a failure with. Nothing names anything of the pin
/// but its address.
fn connect_failed(failure: &ConnectFailure, address: &str) -> Said {
    match failure {
        ConnectFailure::Route(route) => failed(&PairFailure::Route(*route), address),
        ConnectFailure::Link(LinkError::Closed {
            reason: Some(reason),
        }) if wire_code(reason) == Some(ErrorCode::BindingUnavailable) => {
            (ErrorCode::BindingUnavailable, vec![], None)
        }
        ConnectFailure::Link(LinkError::Closed { .. } | LinkError::Unreadable)
        | ConnectFailure::OutOfTurn
        | ConnectFailure::Unsigned
        | ConnectFailure::Welcome(WelcomeRefused::Expiry | WelcomeRefused::Protocols) => {
            (ErrorCode::HubLinkLost, vec![], None)
        }
        ConnectFailure::Link(link) => failed(&PairFailure::Link(link.clone()), address),
        ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch) => {
            (ErrorCode::HubKeyMismatch, vec![], None)
        }
        ConnectFailure::Challenge(ChallengeRefused::HubProofInvalid) => {
            (ErrorCode::HubProofInvalid, vec![], None)
        }
        ConnectFailure::Challenge(ChallengeRefused::BindingUnavailable) => {
            (ErrorCode::BindingUnavailable, vec![], None)
        }
        ConnectFailure::Challenge(ChallengeRefused::NoCommonHandshake)
        | ConnectFailure::Welcome(WelcomeRefused::NoCommonProtocol) => {
            (ErrorCode::ProtocolOutOfDate, vec![], None)
        }
        ConnectFailure::Welcome(WelcomeRefused::GenerationLower) => {
            (ErrorCode::GenerationLower, vec![], None)
        }
        ConnectFailure::Refused(refusal) => {
            (api::sync::refusal_code(refusal.code.as_str()), vec![], None)
        }
        ConnectFailure::NotPaired
        | ConnectFailure::Identity(_)
        | ConnectFailure::Signing(_)
        | ConnectFailure::Random(_) => (ErrorCode::Unknown, vec![], Some(format!("{failure:?}"))),
    }
}

/// The message `hub-status` says `failure` with in place of its code's, when
/// the code's advice is pairing's and can't be followed: a hub that didn't
/// prove the key this machine pinned is no reason to pair again.
fn connect_message(failure: &ConnectFailure) -> Option<MessageId> {
    let _ = failure;
    todo!()
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

    /// Each way `hub-status` can fail is said with the code that names it;
    /// a link that broke for no reason the agent can act on is lost.
    #[test]
    fn each_connect_failure_is_said_with_its_code() {
        let code = |failure: ConnectFailure| connect_failed(&failure, "https://hub.example").0;
        let closed = |reason: Option<&str>| {
            ConnectFailure::Link(LinkError::Closed {
                reason: reason.map(str::to_owned),
            })
        };
        let refused = |name: &str| {
            ConnectFailure::Refused(domain::fleet::Refused {
                code: domain::fleet::WireName::parse(name).unwrap(),
                holder: None,
                signature: domain::fleet::Signature::from_bytes([0; 64]).unwrap(),
            })
        };
        let lost = [
            ConnectFailure::Unsigned,
            ConnectFailure::OutOfTurn,
            closed(None),
            closed(Some("token-used")),
            ConnectFailure::Link(LinkError::Unreadable),
            ConnectFailure::Welcome(WelcomeRefused::Expiry),
            ConnectFailure::Welcome(WelcomeRefused::Protocols),
        ];
        for failure in lost {
            let said = format!("{failure:?}");
            assert_eq!(code(failure), ErrorCode::HubLinkLost, "{said}");
        }
        let cases = [
            (
                closed(Some("binding-unavailable")),
                ErrorCode::BindingUnavailable,
            ),
            (
                ConnectFailure::Challenge(ChallengeRefused::BindingUnavailable),
                ErrorCode::BindingUnavailable,
            ),
            (
                ConnectFailure::Challenge(ChallengeRefused::NoCommonHandshake),
                ErrorCode::ProtocolOutOfDate,
            ),
            (
                ConnectFailure::Welcome(WelcomeRefused::NoCommonProtocol),
                ErrorCode::ProtocolOutOfDate,
            ),
            (
                ConnectFailure::Welcome(WelcomeRefused::GenerationLower),
                ErrorCode::GenerationLower,
            ),
            (
                ConnectFailure::Link(LinkError::PinMismatch),
                ErrorCode::HubPinMismatch,
            ),
            (
                ConnectFailure::Route(RouteRefused::PlainOffLoopback),
                ErrorCode::PlainAddress,
            ),
            (refused("proof-refused"), ErrorCode::ProofRefused),
            (refused("generation-lower"), ErrorCode::HubLinkLost),
            (refused("hostname-taken"), ErrorCode::HubLinkLost),
            (refused("token-used"), ErrorCode::HubLinkLost),
        ];
        for (failure, expected) in cases {
            let said = format!("{failure:?}");
            assert_eq!(code(failure), expected, "{said}");
        }
        let (code, args, _) = connect_failed(
            &ConnectFailure::Link(LinkError::Unreachable {
                detail: "refused".to_owned(),
            }),
            "https://hub.example",
        );
        assert_eq!(code, ErrorCode::HubUnreachable);
        assert_eq!(
            args,
            vec![("address", MsgArg::Text("https://hub.example".to_owned()))]
        );
    }

    /// A hub that didn't prove the key this machine pinned gets hub-status's
    /// own message, never pairing's advice to pair again, which `pair` can't
    /// follow while the machine holds that pin; every other failure is said
    /// by its code.
    #[test]
    fn a_hub_that_proves_another_key_is_no_reason_to_pair_again() {
        for refused in [
            ChallengeRefused::HubKeyMismatch,
            ChallengeRefused::HubProofInvalid,
        ] {
            assert_eq!(
                connect_message(&ConnectFailure::Challenge(refused)),
                Some(msg::CLI_HUB_STATUS_HUB_UNPROVED),
                "{refused:?}"
            );
        }
        for other in [
            ConnectFailure::Challenge(ChallengeRefused::BindingUnavailable),
            ConnectFailure::Challenge(ChallengeRefused::NoCommonHandshake),
            ConnectFailure::Link(LinkError::PinMismatch),
            ConnectFailure::Unsigned,
        ] {
            let said = format!("{other:?}");
            assert_eq!(connect_message(&other), None, "{said}");
        }
    }

    /// A key file refused for its mode is named by its file and its mode as
    /// `chmod` takes it, never in decimal (#1137).
    #[test]
    fn an_exposed_key_file_is_said_by_its_name_and_octal_mode() {
        let said = exposed(&IdentityError::Exposed {
            file: domain::fleet::KeyFile::AgentKey,
            mode: 0o644,
        });
        assert_eq!(
            said,
            Some(vec![
                ("file", MsgArg::Text("agent-key".to_owned())),
                ("mode", MsgArg::Text("0644".to_owned())),
            ])
        );
        assert_eq!(
            exposed(&IdentityError::Damaged {
                file: domain::fleet::KeyFile::AgentKey
            }),
            None
        );
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
