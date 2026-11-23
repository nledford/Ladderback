//! The hub's terminal commands (§16, decision 121, "From a terminal"):
//! `pairing-token` on the hub, and `pair` and `hub-status` on the agent.
//! Only a build with the `hub` feature has them.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::hub_tls::{HubTlsKey, TlsKeyRefusal};
use app::leases::MaintenanceLeases;
use app::ownership::RepositoryOwners;
use app::pairing::{
    AgentPairing, AgentPairingError, ChallengeRefused, ConnectFailure, ConnectToHub, Connected,
    HubPairing, HubPairingError, HubStart, PairFailure, PairWithHub,
};
use app::ports::{
    AgentRegistry as _, EventBus, Identity as _, IdentityError, LinkError, Signing as _,
    ThisMachine as _,
};
use app::sync::{RecordReports, SyncWithHub};
use domain::fleet::{
    AgentStatus, Approval, ApprovalRefused, Fingerprint, Hostname, HubAddress, HubRoute, HubScheme,
    MachineZone, PairingToken, RouteRefused, RouteSession, TokenId, TokenTls, WelcomeRefused,
    is_loopback,
};
use domain::{SignedDuration, Timestamp};
use futures::StreamExt as _;
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::data_dir_lock::{DataDirLock, LockError};
use platform::identity::FileIdentity;
use platform::random::OsRandom;
use platform::{SystemClock, dirs, terminal};
use signing_adapter::AwsLcSigning;
use store::SqliteStore;
use tls_adapter::{RcgenTlsKeys, WebSocketHubConnector};

use crate::confirm::{Question, confirmed};
use crate::output::{escaped, refusal_line, to_stderr, to_stdout};
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
        /// The `https://` address is a proxy's, whose certificate a
        /// certificate authority signed, rather than the hub's own.
        certificate_authority: bool,
    },
    List,
    Revoke(String),
    /// List the machines waiting for approval.
    Pending,
    /// Approve the machine with this hostname, once the person confirms or
    /// gave `--yes`.
    Approve {
        hostname: String,
        yes: bool,
    },
    /// Forget the computer the hub took under the machine with this
    /// hostname, so the next one to connect as it is taken (decision 150).
    ForgetComputer(String),
}

/// The hub's pairing over `store` and the key files in `data_dir`, taking
/// its approved agents' reports into `store` (§7.2).
pub(crate) fn hub_pairing(data_dir: &Path, store: Arc<SqliteStore>) -> HubPairing {
    HubPairing::new(
        store.clone(),
        Arc::new(FileIdentity::in_data_dir(data_dir)),
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
        Arc::new(SystemClock),
        crate::build_info::SHOWN.as_str().to_owned(),
    )
    .with_inbox(Arc::<SqliteStore>::clone(&store))
    .with_owners(Arc::new(RepositoryOwners::new(
        Arc::<SqliteStore>::clone(&store),
        store,
        Arc::new(SystemClock),
    )))
}

/// The hub's TLS key in `data_dir`, from which its built-in TLS serves a
/// certificate (§16, decision 120, "The hub key").
pub(crate) fn hub_tls_key(data_dir: &Path) -> HubTlsKey {
    HubTlsKey::new(
        Arc::new(FileIdentity::in_data_dir(data_dir)),
        Arc::new(RcgenTlsKeys),
    )
}

/// Why the hub's TLS key can't serve and how to fix it, for the daemon's
/// log, which is in English (decision 120, "The hub key"). It names the
/// key file and none of the key (I4).
pub(crate) fn tls_key_unserved(refusal: &TlsKeyRefusal) -> String {
    let file = domain::fleet::KeyFile::HubTlsKey;
    match refusal {
        TlsKeyRefusal::Missing => format!(
            "its key file `{file}` in the data folder is missing; restore it from your backup of the data folder, then restart the hub"
        ),
        TlsKeyRefusal::AnotherKey => format!(
            "its key file `{file}` in the data folder holds another key than the one this hub made; restore it from your backup of the data folder, then restart the hub"
        ),
        TlsKeyRefusal::NotAKey | TlsKeyRefusal::Identity(IdentityError::Damaged { .. }) => {
            format!(
                "its key file `{file}` in the data folder holds no key the hub can serve; restore it from your backup of the data folder, then restart the hub"
            )
        }
        TlsKeyRefusal::Identity(IdentityError::Exposed { file, mode }) => format!(
            "its key file `{file}` can be read or changed by other users (mode {mode:04o}); make it its owner's only, as `chmod 600` does, then restart the hub"
        ),
        other => format!("{other:?}; fix what this names, then restart the hub"),
    }
}

/// The hub's keys at its start: made on a first start and read after that,
/// so a key file the hub can't read stops the start here, not at the first
/// machine or token that asks (decision 120, "The key's life"). The hub
/// key's fingerprint is logged, the one every machine pins; a key file
/// gone while machines are registered is logged as the reason `/sync/v1`
/// refuses every machine, and the hub runs without it ("The hub key").
pub(crate) async fn keys_at_start(pairing: &HubPairing) -> Result<(), HubPairingError> {
    match pairing.start().await? {
        HubStart::Keys(keys) => log::info!("the hub's key {}", Fingerprint::of_key(keys.key())),
        HubStart::Unserved(file) => log::error!("{}", keys_unserved(file)),
    }
    Ok(())
}

/// Why the hub serves no machine with its key file gone, and how to fix it,
/// for the daemon's log, which is in English. It names the key file and
/// none of the key (I4).
fn keys_unserved(file: domain::fleet::KeyFile) -> String {
    format!(
        "the hub's key file `{file}` in the data folder is missing while machines are registered, so the hub made no new key and refuses every machine at {} until the file is restored; restore it from your backup of the data folder, then restart the hub. This machine's own backups go on",
        api::sync::PATH
    )
}

/// Says why the hub's keys stopped its start, and the exit status of that.
pub(crate) fn start_refused(
    localizer: &Localizer,
    error: &HubPairingError,
) -> io::Result<ExitCode> {
    let (id, args) = start_refusal(error);
    say(localizer, id, &args)?;
    Ok(ExitCode::FAILURE)
}

/// The message that says why `error` stopped the hub's start, and its
/// arguments. A text that came from the system is [`escaped`], and none
/// carries a key (I4).
fn start_refusal(error: &HubPairingError) -> (MessageId, Vec<(&'static str, MsgArg)>) {
    match error {
        HubPairingError::Identity(identity @ IdentityError::Exposed { .. }) => (
            msg::CLI_KEY_FILE_EXPOSED,
            exposed(identity).unwrap_or_default(),
        ),
        HubPairingError::Identity(IdentityError::Damaged { file }) => {
            (msg::CLI_HUB_KEY_FILE_DAMAGED, vec![("file", text(file))])
        }
        HubPairingError::Identity(IdentityError::Unavailable { file, detail }) => (
            msg::CLI_HUB_KEY_FILE_UNAVAILABLE,
            vec![("file", text(file)), ("detail", text(escaped(detail)))],
        ),
        other => (
            msg::CLI_HUB_KEYS_FAILED,
            vec![("detail", text(format!("{other:?}")))],
        ),
    }
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
                daemon_running(&ask),
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
    // The zone this computer is in, which the person reads times in.
    let zone = domain::TimeZone::system();
    let said = match ask {
        TokenAsk::Make {
            address,
            ttl_minutes,
            certificate_authority,
        } => {
            let tls = (!certificate_authority).then(|| hub_tls_key(data_dir));
            make(
                localizer,
                &pairing,
                &store,
                &address,
                ttl_minutes,
                tls,
                &zone,
            )
            .await
        }
        TokenAsk::List => list(localizer, &store, &zone).await,
        TokenAsk::Revoke(id) => revoke(localizer, &pairing, &id).await,
        TokenAsk::Pending => pending(localizer, &pairing, &zone).await,
        TokenAsk::Approve { hostname, yes } => approve(localizer, &pairing, &hostname, yes).await,
        TokenAsk::ForgetComputer(hostname) => forget_computer(localizer, &pairing, &hostname).await,
    };
    store.close().await;
    said
}

/// The message `ask` refuses with while the daemon holds the data folder:
/// what this terminal would have done, and that the daemon must stop first.
fn daemon_running(ask: &TokenAsk) -> MessageId {
    match ask {
        TokenAsk::Make { .. } => msg::CLI_PAIRING_TOKEN_MAKE_DAEMON_RUNNING,
        TokenAsk::List => msg::CLI_PAIRING_TOKEN_LIST_DAEMON_RUNNING,
        TokenAsk::Revoke(_) => msg::CLI_PAIRING_TOKEN_REVOKE_DAEMON_RUNNING,
        TokenAsk::Pending => msg::CLI_PAIRING_TOKEN_PENDING_DAEMON_RUNNING,
        TokenAsk::Approve { .. } => msg::CLI_PAIRING_TOKEN_APPROVE_DAEMON_RUNNING,
        TokenAsk::ForgetComputer(_) => msg::CLI_PAIRING_TOKEN_FORGET_DAEMON_RUNNING,
    }
}

/// Makes a token for `address`. An `https://` address is the hub's own,
/// whose certificate the token pins, when `tls` is its TLS key, and a
/// proxy's, checked against a certificate authority, when it is `None`.
async fn make(
    localizer: &Localizer,
    pairing: &HubPairing,
    store: &SqliteStore,
    address: &str,
    ttl_minutes: Option<i64>,
    tls: Option<HubTlsKey>,
    zone: &domain::TimeZone,
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
    // The hub's built-in TLS serves an `https://` address unless it is a
    // proxy's: its key is taken (made at the hub's first start, or here when
    // the hub never started) and its certificate pinned, so no certificate
    // authority is needed.
    let certificate = match (&route, tls) {
        (Some(route), Some(tls)) if route.session == RouteSession::Ca => {
            match tls.start(store).await {
                Ok(served) => TokenTls::Pinned(served.fingerprint),
                Err(refusal) => return tls_refused(localizer, &refusal),
            }
        }
        _ => TokenTls::Ca,
    };
    let token = match pairing
        .make_token(&keys, address.clone(), certificate, life)
        .await
    {
        Ok(token) => token,
        Err(error) => return hub_refused(localizer, &error),
    };
    if let TokenTls::Pinned(fingerprint) = certificate {
        say(
            localizer,
            msg::CLI_PAIRING_TOKEN_PINNED,
            &[("fingerprint", text(fingerprint))],
        )?;
    }
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
            .map(|record| record.expires_at),
        Err(error) => return refused(localizer, &format!("{error:?}")),
    };
    let Some(expires) = expires else {
        return refused(localizer, "the token just made isn't among the unused ones");
    };
    let (expires, offset) = written_at(expires, zone);
    say(
        localizer,
        msg::CLI_PAIRING_TOKEN_MADE,
        &[("id", text(id)), ("expires", expires), ("offset", offset)],
    )?;
    Ok(ExitCode::SUCCESS)
}

async fn list(
    localizer: &Localizer,
    store: &SqliteStore,
    zone: &domain::TimeZone,
) -> io::Result<ExitCode> {
    let tokens = match store.unused_tokens(Timestamp::now()).await {
        Ok(tokens) => tokens,
        Err(error) => return refused(localizer, &format!("{error:?}")),
    };
    if tokens.is_empty() {
        say(localizer, msg::CLI_PAIRING_TOKEN_NONE, &[])?;
    }
    for token in tokens {
        let (expires, offset) = written_at(token.expires_at, zone);
        let args = [
            ("id", text(token.id())),
            ("expires", expires),
            ("offset", offset),
        ];
        to_stdout(&format!(
            "{}\n",
            localizer.message(msg::CLI_PAIRING_TOKEN_LISTED, &args)
        ))?;
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

/// `pairing-token --pending`: one line per machine waiting for approval,
/// its agent ID, key fingerprint, since when it waits in the person's
/// `zone` and, last, since it alone may hold spaces, its hostname. The
/// columns are separated by two spaces.
async fn pending(
    localizer: &Localizer,
    pairing: &HubPairing,
    zone: &domain::TimeZone,
) -> io::Result<ExitCode> {
    let agents = match pairing.pending().await {
        Ok(agents) => agents,
        Err(error) => return hub_refused(localizer, &error),
    };
    if agents.is_empty() {
        say(localizer, msg::CLI_PAIRING_TOKEN_NONE_WAITING, &[])?;
    }
    for agent in agents {
        let since = match agent.status {
            AgentStatus::Pending { since } => since,
            AgentStatus::Approved { at } => at,
        };
        let (since, offset) = written_at(since, zone);
        let args = [
            ("agent_id", text(agent.agent_id.as_uuid())),
            ("fingerprint", text(Fingerprint::of_key(&agent.key))),
            ("since", since),
            ("offset", offset),
            ("hostname", text(agent.hostname)),
        ];
        to_stdout(&format!(
            "{}\n",
            localizer.message(msg::CLI_PAIRING_TOKEN_WAITING, &args)
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// What `pairing-token --approve` asks before it approves.
const APPROVE: Question = Question {
    ask: msg::CLI_PAIRING_TOKEN_APPROVE_CONFIRM,
    declined: msg::CLI_PAIRING_TOKEN_APPROVE_DECLINED,
    no_terminal: msg::CLI_PAIRING_TOKEN_APPROVE_NO_TERMINAL,
};

/// `pairing-token --approve`: names the machine with its agent ID and key
/// fingerprint, for the person to compare with that machine's screen, and
/// once they confirm approves it as named (decision 120, "Pending
/// approval").
async fn approve(
    localizer: &Localizer,
    pairing: &HubPairing,
    hostname: &str,
    yes: bool,
) -> io::Result<ExitCode> {
    let found = match Hostname::parse(hostname) {
        Ok(name) => match pairing.agent_named(&name).await {
            Ok(found) => found,
            Err(error) => return hub_refused(localizer, &error),
        },
        Err(_) => None,
    };
    let Some(agent) = found else {
        return approval_refused(localizer, ApprovalRefused::Unknown, hostname);
    };
    let name = agent.hostname.to_string();
    if !agent.is_pending() {
        return approval_refused(localizer, ApprovalRefused::AlreadyApproved, &name);
    }
    say(
        localizer,
        msg::CLI_PAIRING_TOKEN_APPROVE_WAITING,
        &[
            ("hostname", text(&name)),
            ("agent_id", text(agent.agent_id.as_uuid())),
            ("fingerprint", text(Fingerprint::of_key(&agent.key))),
        ],
    )?;
    if !confirmed(localizer, &APPROVE, terminal::stdin_is_terminal(), yes)? {
        return Ok(ExitCode::FAILURE);
    }
    match pairing.approve(&agent).await {
        Ok(Ok(_)) => {
            say(
                localizer,
                msg::CLI_PAIRING_TOKEN_APPROVED,
                &[("hostname", text(&name))],
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Ok(Err(refused)) => approval_refused(localizer, refused, &name),
        Err(error) => hub_refused(localizer, &error),
    }
}

/// `pairing-token --forget-computer`: forgets the computer the hub took
/// under the machine named `hostname`, so the next one to connect as it is
/// taken in its place: the way back for a machine refused as a second
/// computer (decision 150).
async fn forget_computer(
    localizer: &Localizer,
    pairing: &HubPairing,
    hostname: &str,
) -> io::Result<ExitCode> {
    let found = match Hostname::parse(hostname) {
        Ok(name) => match pairing.agent_named(&name).await {
            Ok(found) => found,
            Err(error) => return hub_refused(localizer, &error),
        },
        Err(_) => None,
    };
    let Some(agent) = found else {
        say(
            localizer,
            msg::CLI_PAIRING_TOKEN_FORGET_UNKNOWN,
            &[("hostname", text(hostname))],
        )?;
        return Ok(ExitCode::FAILURE);
    };
    if let Err(error) = pairing.forget_machine(&agent).await {
        return hub_refused(localizer, &error);
    }
    say(
        localizer,
        msg::CLI_PAIRING_TOKEN_FORGOT_COMPUTER,
        &[("hostname", text(agent.hostname.as_str()))],
    )?;
    Ok(ExitCode::SUCCESS)
}

fn approval_refused(
    localizer: &Localizer,
    refused: ApprovalRefused,
    hostname: &str,
) -> io::Result<ExitCode> {
    let message = match refused {
        ApprovalRefused::Unknown => msg::CLI_PAIRING_TOKEN_APPROVE_UNKNOWN,
        ApprovalRefused::AlreadyApproved => msg::CLI_PAIRING_TOKEN_APPROVE_ALREADY,
    };
    say(localizer, message, &[("hostname", text(hostname))])?;
    Ok(ExitCode::FAILURE)
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

fn tls_refused(localizer: &Localizer, refusal: &TlsKeyRefusal) -> io::Result<ExitCode> {
    if let TlsKeyRefusal::Identity(identity) = refusal
        && let Some(args) = exposed(identity)
    {
        say(localizer, msg::CLI_KEY_FILE_EXPOSED, &args)?;
        return Ok(ExitCode::FAILURE);
    }
    match refusal {
        TlsKeyRefusal::Missing
        | TlsKeyRefusal::AnotherKey
        | TlsKeyRefusal::NotAKey
        | TlsKeyRefusal::Identity(IdentityError::Damaged { .. }) => {
            say(localizer, msg::CLI_PAIRING_TOKEN_NO_TLS_KEY, &[])?;
            Ok(ExitCode::FAILURE)
        }
        _ => refused(localizer, &format!("{refusal:?}")),
    }
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
pub(crate) fn pair(ask: PairAsk, plain: bool) -> ExitCode {
    // Core dumps and debugger attach are off before the token is read
    // (decision 98): a Ctrl-\ at the prompt dumps nothing.
    let localizer = crate::start_protected(plain, None);
    finish(&localizer, run_pair(&localizer, ask))
}

fn run_pair(localizer: &Localizer, ask: PairAsk) -> io::Result<ExitCode> {
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    if let Some(address) = ask.address.as_deref()
        && let Some(code) = follow(localizer, &data_dir, address, ask.hostname.clone())?
    {
        return Ok(code);
    }
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
    let Some(hostname) = hostname_or_said(localizer, ask.hostname)? else {
        return Ok(ExitCode::from(2));
    };
    // Each kind of address has its own whole sentence, not a phrase put into
    // one. A plain address shows no certificate, so its sentence names none.
    let read = match (token.address().scheme(), token.tls()) {
        (HubScheme::Http, _) => msg::CLI_PAIR_READ_PLAIN,
        (HubScheme::Https, TokenTls::Ca) => msg::CLI_PAIR_READ_CA,
        (HubScheme::Https, TokenTls::Pinned(_)) => msg::CLI_PAIR_READ_PINNED,
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
    if !confirmed(localizer, &PAIR, at_terminal, ask.yes)? {
        return Ok(ExitCode::FAILURE);
    }
    let runtime = match runtime(localizer)? {
        Ok(runtime) => runtime,
        Err(code) => return Ok(code),
    };
    runtime.block_on(on_the_agent(localizer, &data_dir, token, hostname))
}

/// What `pair` asks before it sends anything.
const PAIR: Question = Question {
    ask: msg::CLI_PAIR_CONFIRM,
    declined: msg::CLI_PAIR_NOT_CONFIRMED,
    no_terminal: msg::CLI_PAIR_NO_TERMINAL,
};

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
    // The leases and epoch floors a new hub replaces are in the database.
    let store = match SqliteStore::open(&data_dir.join(DATABASE)).await {
        Ok(store) => Arc::new(store),
        Err(error) => return refused(localizer, &error.to_string()),
    };
    let connector = WebSocketHubConnector::new();
    let pairing = AgentPairing::new(
        Arc::new(FileIdentity::in_data_dir(data_dir)),
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
        Arc::<SqliteStore>::clone(&store),
    );
    let address = token.address().as_str().to_owned();
    let asked = hostname.clone();
    let outcome = PairWithHub::new(pairing, Arc::new(connector))
        .pair(token, agent_id, hostname, machine_zone())
        .await;
    store.close().await;
    match outcome {
        Ok(paired) => {
            let fingerprint = ("fingerprint", text(Fingerprint::of_key(&paired.agent_key)));
            let (one_name, held_name) = if paired.approved {
                (msg::CLI_PAIR_APPROVED, msg::CLI_PAIR_APPROVED_HELD)
            } else {
                (msg::CLI_PAIR_PENDING, msg::CLI_PAIR_PENDING_HELD)
            };
            match &paired.held_as {
                Some(held) => say(
                    localizer,
                    held_name,
                    &[
                        ("held", text(escaped(held.as_str()))),
                        ("asked", text(escaped(asked.as_str()))),
                        fingerprint,
                    ],
                )?,
                None => say(localizer, one_name, &[fingerprint])?,
            }
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

/// `ladderback pair <address>` on a machine that paired: the hub's new
/// address, which needs no token (decision 120, "Addresses and the
/// transport statement"). `None` when the machine holds no pin, so the
/// address goes with the token it pairs with.
fn follow(
    localizer: &Localizer,
    data_dir: &Path,
    address: &str,
    hostname: Option<String>,
) -> io::Result<Option<ExitCode>> {
    let Ok(Some(agent_id)) = platform::agent_id::load(data_dir) else {
        return Ok(None);
    };
    let runtime = match runtime(localizer)? {
        Ok(runtime) => runtime,
        Err(code) => return Ok(Some(code)),
    };
    let identity = Arc::new(FileIdentity::in_data_dir(data_dir));
    // A pin that can't be read is still a pin: connecting says why.
    if matches!(runtime.block_on(identity.pin()), Ok(None)) {
        return Ok(None);
    }
    let Ok(parsed) = HubAddress::parse(address) else {
        say(
            localizer,
            msg::CLI_PAIRING_TOKEN_BAD_ADDRESS,
            &[("address", text(address))],
        )?;
        return Ok(Some(ExitCode::from(2)));
    };
    let Some(hostname) = hostname_or_said(localizer, hostname)? else {
        return Ok(Some(ExitCode::from(2)));
    };
    let connecting = connecting(identity, data_dir);
    let outcome =
        runtime.block_on(connecting.connect_at(agent_id, hostname, machine_zone(), parsed));
    let connected = match outcome {
        Ok(connected) => connected,
        // The hub there proved the pinned key but no longer knows this
        // machine, or the machine lost its key: it pairs again, with a
        // token, at that address.
        Err(failure) if pairs_again(&failure) => {
            // The pairing that follows gives the exit code.
            let _ = say_unconnected(localizer, &failure, address)?;
            return Ok(None);
        }
        Err(failure) => {
            let code = say_unconnected(localizer, &failure, address)?;
            say(localizer, msg::CLI_PAIR_ADDRESS_NOT_KEPT, &[])?;
            return Ok(Some(code));
        }
    };
    say(
        localizer,
        msg::CLI_PAIR_ADDRESS_KEPT,
        &[("address", text(address))],
    )?;
    say_connected(localizer, &connected)?;
    Ok(Some(ExitCode::SUCCESS))
}

/// Whether a failure to follow the hub leaves pairing again with a token
/// as the way on: the hub refused the proof, so it no longer knows this
/// machine, or the machine holds a pin but no key.
fn pairs_again(failure: &ConnectFailure) -> bool {
    match failure {
        ConnectFailure::NotPaired => true,
        ConnectFailure::Refused(refusal) => {
            api::sync::refusal_code(refusal.code.as_str()) == ErrorCode::ProofRefused
        }
        _ => false,
    }
}

/// `hostname`, or the machine's, when it is one; `None` once it said why
/// it isn't.
fn hostname_or_said(
    localizer: &Localizer,
    hostname: Option<String>,
) -> io::Result<Option<Hostname>> {
    let hostname = hostname.unwrap_or_else(platform::hostname);
    let Ok(parsed) = Hostname::parse(&hostname) else {
        say(
            localizer,
            msg::CLI_PAIR_BAD_HOSTNAME,
            &[("name", text(hostname))],
        )?;
        return Ok(None);
    };
    Ok(Some(parsed))
}

/// The machine zone of the time zone this machine runs in, which it tells
/// its hub; none when the zone has no IANA name.
fn machine_zone() -> Option<MachineZone> {
    MachineZone::of(&domain::TimeZone::system())
}

/// Connecting this machine to its hub, on the pin and keys in `identity`,
/// telling the fingerprint of the computer it runs on, none when its
/// identifier can't be read (decision 150).
fn connecting(identity: Arc<FileIdentity>, data_dir: &Path) -> ConnectToHub {
    let machine = platform::this_machine::SystemMachine::in_data_dir(data_dir)
        .identifier()
        .ok();
    ConnectToHub::new(
        identity,
        Arc::new(AwsLcSigning),
        Arc::new(OsRandom),
        Arc::new(SystemClock),
        Arc::new(WebSocketHubConnector::new()),
        crate::build_info::SHOWN.as_str().to_owned(),
        machine,
    )
}

/// How often a daemon on a machine that hasn't paired looks for its pairing.
const PAIRING_POLL: std::time::Duration = std::time::Duration::from_secs(30);

/// This machine's reports to its hub (§7.2): once it paired, a recorder
/// keeps each operation's start and end in the outbox in `store`, and a
/// session replays them to the hub whenever it holds one the hub hasn't
/// acknowledged. Neither touches the machine's operations, so a machine
/// that can't reach its hub backs up on schedule all the same. A machine
/// paired when this starts records from `early`, the events since its
/// caller subscribed before starting the scheduler. One that hasn't paired
/// drops them, looks for its pairing every [`PAIRING_POLL`], and keeps no
/// report until then. Each session tells `standing` what the hub made of
/// this machine: refused, signed, as a second computer under its machine
/// ID, or taken (decision 150). Runs until aborted.
pub(crate) async fn reporting(
    data_dir: PathBuf,
    events: Arc<dyn EventBus>,
    early: futures::stream::BoxStream<'static, api::UiEvent>,
    store: Arc<SqliteStore>,
    config: Arc<dyn app::ports::ConfigStore>,
    standing: Arc<app::machine::MachineStanding>,
) {
    let identity = Arc::new(FileIdentity::in_data_dir(&data_dir));
    let mut early = Some(early);
    let agent_id = loop {
        if let Ok(Some(agent_id)) = platform::agent_id::load(&data_dir)
            && matches!(identity.pin().await, Ok(Some(_)))
        {
            break agent_id;
        }
        early = None;
        tokio::time::sleep(PAIRING_POLL).await;
    };
    let Ok(hostname) = Hostname::parse(&platform::hostname()) else {
        log::warn!(
            "this machine's name isn't one a hub takes, so its reports aren't sent to its hub"
        );
        return;
    };
    let recorder = RecordReports::new(store.clone());
    let mut happened = match early {
        Some(early) => early,
        None => events.subscribe().await,
    };
    let recording = async {
        while let Some(event) = happened.next().await {
            if let Err(error) = recorder.record(&event).await {
                log::warn!("a report for the hub couldn't be kept in the outbox: {error:?}");
            }
        }
    };
    // Each session also asks for the leases on the repositories this
    // machine maintains, and keeps the hub's grants beside the outbox,
    // where its maintenance reads them (§7.3, I1).
    let leases = Arc::new(MaintenanceLeases::new(
        Arc::<FileIdentity>::clone(&identity),
        Arc::<SqliteStore>::clone(&store),
        Arc::new(SystemClock),
    ));
    let syncing = SyncWithHub::new(
        connecting(identity, &data_dir),
        Arc::<SqliteStore>::clone(&store),
        Arc::new(SystemClock),
        agent_id,
        hostname,
        machine_zone(),
    )
    // A maintenance running here keeps its repository's lease from a
    // release, as its operation log says.
    .with_leases(config, leases, store)
    .with_standing(standing);
    tokio::join!(recording, syncing.run());
}

/// Says where the hub holds this machine.
fn say_connected(localizer: &Localizer, connected: &Connected) -> io::Result<()> {
    match connected.approval {
        Approval::Pending => say(
            localizer,
            msg::CLI_HUB_STATUS_PENDING,
            &[(
                "fingerprint",
                text(Fingerprint::of_key(&connected.agent_key)),
            )],
        ),
        Approval::Approved => say(localizer, msg::CLI_HUB_STATUS_APPROVED, &[]),
    }
}

/// Says why this machine didn't connect to its hub at `address`, the
/// first it tried.
fn say_unconnected(
    localizer: &Localizer,
    failure: &ConnectFailure,
    address: &str,
) -> io::Result<ExitCode> {
    if let ConnectFailure::NotPaired = failure {
        say(localizer, msg::CLI_HUB_STATUS_NOT_PAIRED, &[])?;
        return Ok(ExitCode::FAILURE);
    }
    if let ConnectFailure::Identity(identity) = failure
        && let Some(args) = exposed(identity)
    {
        say(localizer, msg::CLI_KEY_FILE_EXPOSED, &args)?;
        return Ok(ExitCode::FAILURE);
    }
    if let Some(message) = connect_message(failure) {
        say(localizer, message, &[("address", text(address))])?;
        return Ok(ExitCode::FAILURE);
    }
    let (code, args, detail) = connect_failed(failure, address);
    to_stderr(&refusal_line(localizer, code, &args, detail.as_deref()))?;
    Ok(ExitCode::FAILURE)
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
    let Some(hostname) = hostname_or_said(localizer, hostname)? else {
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
    // The address a failure names, the first tried; the use case reads the
    // pin it trusts.
    let address = match identity.pin().await {
        Ok(Some(pin)) => pin
            .addresses()
            .next()
            .map_or_else(String::new, |address| address.as_str().to_owned()),
        Ok(None) | Err(_) => String::new(),
    };
    match connecting(identity, data_dir)
        .connect(agent_id, hostname, machine_zone())
        .await
    {
        Ok(connected) => {
            say_connected(localizer, &connected)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(failure) => say_unconnected(localizer, &failure, &address),
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
        // A hub refuses a proof only as `proof-refused`, and a second
        // computer under the ID as `identity-conflict`; any other code,
        // pairing's among them, would show advice for something else.
        ConnectFailure::Refused(refusal) => match api::sync::refusal_code(refusal.code.as_str()) {
            code @ (ErrorCode::ProofRefused | ErrorCode::IdentityConflict) => (code, vec![], None),
            _ => (
                ErrorCode::HubLinkLost,
                vec![],
                Some(refusal.code.as_str().to_owned()),
            ),
        },
        ConnectFailure::NotPaired
        | ConnectFailure::PinFull
        | ConnectFailure::Identity(_)
        | ConnectFailure::Signing(_)
        | ConnectFailure::Random(_) => (ErrorCode::Unknown, vec![], Some(format!("{failure:?}"))),
    }
}

/// The message `hub-status` says `failure` with in place of its code's, when
/// the code's advice is pairing's and can't be followed: a hub that didn't
/// prove the key this machine pinned is no reason to pair again.
fn connect_message(failure: &ConnectFailure) -> Option<MessageId> {
    match failure {
        ConnectFailure::Challenge(
            ChallengeRefused::HubKeyMismatch | ChallengeRefused::HubProofInvalid,
        ) => Some(msg::CLI_HUB_STATUS_HUB_UNPROVED),
        _ => None,
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

/// A time as the person at this terminal reads it: the instant, which the
/// catalog writes in `zone`, and that zone's offset from UTC at the
/// instant. A zone with no IANA name is read as UTC, with UTC's offset, so
/// the time and the offset beside it never disagree.
fn written_at(at: Timestamp, zone: &domain::TimeZone) -> (MsgArg, MsgArg) {
    let named = MachineZone::of(zone).and_then(|name| api::IanaZone::new(name.as_str()).ok());
    let offset = if named.is_some() {
        zone.to_offset(at)
    } else {
        domain::Offset::UTC
    };
    (
        MsgArg::Timestamp {
            utc_ms: at.as_millisecond(),
            zone: named,
        },
        text(api::ClockOffset::new(offset)),
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

    /// A paired machine's reporting keeps the start of an operation the
    /// scheduler began before the reporting task first ran, on the events
    /// it was subscribed to before the scheduler started (§7.2).
    #[tokio::test]
    async fn a_start_published_before_reporting_runs_is_kept_for_the_hub() {
        use app::ports::Outbox as _;
        use domain::fleet::{Fingerprint, HubPin, TokenTls};

        const KEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let data = tempfile::tempdir().unwrap();
        platform::agent_id::load_or_create(data.path()).unwrap();
        let key = domain::fleet::PublicKey::from_hex(KEY).unwrap();
        FileIdentity::in_data_dir(data.path())
            .write_pin(&HubPin::new(
                key,
                TokenTls::Pinned(Fingerprint::from_digest([0x5a; 32])),
                key,
                HubAddress::parse("https://127.0.0.1:1").unwrap(),
            ))
            .await
            .unwrap();
        let store = Arc::new(
            SqliteStore::open(&data.path().join("store.db"))
                .await
                .unwrap(),
        );
        let events: Arc<dyn EventBus> = Arc::new(app::event_bus::BroadcastEventBus::new());
        let early = events.subscribe().await;
        let task = tokio::spawn(reporting(
            data.path().to_owned(),
            Arc::clone(&events),
            early,
            Arc::clone(&store),
            Arc::new(app::fakes::InMemoryConfigStore::default()),
            Arc::default(),
        ));
        events
            .publish(api::UiEvent::OperationStarted {
                operation: "0190f3a0-0000-7000-8000-000000000001".to_owned(),
                repository: None,
                started_at: "2027-01-15T08:00:00Z".to_owned(),
            })
            .await;
        let kept = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !store.unacknowledged(None, 1).await.unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        task.abort();
        assert!(kept.is_ok(), "the start wasn't kept in the outbox");
    }

    #[test]
    fn a_moment_shows_as_its_date_and_minute_in_utc() {
        assert_eq!(when(1_795_000_000), "2026-11-18 11:06 UTC");
    }

    /// A line of the three that carry a time, rendered in English with the
    /// narrow no-break space before `AM` folded to a plain one.
    fn said_in_english(id: MessageId, args: &[(&str, MsgArg)]) -> String {
        let localizer = Localizer::new(&["en-US".parse().expect("a locale")]);
        localizer
            .message(id, args)
            .into_text()
            .replace(['\u{202f}', '\u{a0}'], " ")
    }

    fn at(text: &str) -> Timestamp {
        text.parse().expect("an instant")
    }

    /// `--list` writes a token's lapse time in the person's zone, in
    /// words, with the zone's offset beside it (#1309).
    #[test]
    fn a_token_listed_lapses_at_the_time_in_the_persons_zone_with_its_offset() {
        let zone = domain::TimeZone::get("America/New_York").expect("a zone");
        let (expires, offset) = written_at(at("2026-11-22T11:59:26.17Z"), &zone);
        let args = [
            ("id", text("01a20320")),
            ("expires", expires),
            ("offset", offset),
        ];
        assert_eq!(
            said_in_english(msg::CLI_PAIRING_TOKEN_LISTED, &args),
            "01a20320  usable until November 22, 2026 at 6:59 AM (-05:00)"
        );
    }

    /// The offset beside a time is the zone's at that instant, so one zone
    /// reads -05:00 in winter and -04:00 in summer, and a half-hour zone
    /// keeps its half hour.
    #[test]
    fn the_offset_beside_a_time_is_the_zones_at_that_instant() {
        let read = |zone: &str, instant: &str| {
            let zone = domain::TimeZone::get(zone).expect("a zone");
            let (since, offset) = written_at(at(instant), &zone);
            let args = [
                ("agent_id", text("0190")),
                ("fingerprint", text("FP2F-TBBW-YJDQ-3NF8-A15V")),
                ("since", since),
                ("offset", offset),
                ("hostname", text("e2e laptop")),
            ];
            said_in_english(msg::CLI_PAIRING_TOKEN_WAITING, &args)
        };
        assert_eq!(
            read("America/New_York", "2026-07-04T16:00:00Z"),
            "0190  FP2F-TBBW-YJDQ-3NF8-A15V  waiting since July 4, 2026 at 12:00 PM (-04:00)  e2e laptop"
        );
        assert_eq!(
            read("Asia/Kolkata", "2026-11-22T11:59:26Z"),
            "0190  FP2F-TBBW-YJDQ-3NF8-A15V  waiting since November 22, 2026 at 5:29 PM (+05:30)  e2e laptop"
        );
    }

    /// A zone with no IANA name can't be passed to the formatter, which
    /// then writes UTC's time, so the offset beside it is UTC's too: the
    /// time and its label never disagree.
    #[test]
    fn a_zone_with_no_name_is_read_in_utc_with_utcs_offset() {
        let zone = domain::TimeZone::fixed(domain::Offset::constant(2));
        let (expires, offset) = written_at(at("2026-11-22T11:59:26Z"), &zone);
        let args = [
            ("id", text("01a20320")),
            ("expires", expires),
            ("offset", offset),
        ];
        assert_eq!(
            said_in_english(msg::CLI_PAIRING_TOKEN_LISTED, &args),
            "01a20320  usable until November 22, 2026 at 11:59 AM (+00:00)"
        );
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
            (refused("identity-conflict"), ErrorCode::IdentityConflict),
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
        // A hub's signed refusal other than `proof-refused` is a lost link
        // whose text still names the hub's code, so the person can report it.
        for name in ["hostname-taken", "generation-lower", "token-used"] {
            let (code, args, detail) = connect_failed(&refused(name), "https://hub.example");
            let text = l10n::Localizer::new(&[]).error(code, &args, detail.as_deref());
            assert!(text.text().contains(name), "{name}: {text}");
        }
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

    /// A machine the hub no longer knows, or one that lost its key, pairs
    /// again at the address it was given; any other failure keeps the
    /// addresses it had and ends there.
    #[test]
    fn only_a_refused_proof_or_a_lost_key_pairs_again() {
        let refused = |code: &str| {
            ConnectFailure::Refused(domain::fleet::Refused {
                code: domain::fleet::WireName::parse(code).unwrap(),
                holder: None,
                signature: domain::fleet::Signature::from_bytes([0; 64]).unwrap(),
            })
        };
        assert!(pairs_again(&ConnectFailure::NotPaired));
        assert!(pairs_again(&refused("proof-refused")));
        for other in [
            refused("hub-unavailable"),
            ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch),
            ConnectFailure::Link(LinkError::PinMismatch),
            ConnectFailure::Unsigned,
        ] {
            let said = format!("{other:?}");
            assert!(!pairs_again(&other), "{said}");
        }
    }

    /// While the daemon holds the data folder, `--forget-computer` is
    /// refused in words of its own, saying what it would have done.
    #[test]
    fn a_running_hub_refuses_to_forget_a_computer_in_words_of_its_own() {
        let forget = TokenAsk::ForgetComputer("e2e-laptop".to_owned());
        assert_eq!(
            daemon_running(&forget),
            msg::CLI_PAIRING_TOKEN_FORGET_DAEMON_RUNNING
        );
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

    /// Each way a key file stops the hub's start is said by the message
    /// that names it, and a system's text is escaped.
    #[test]
    fn each_way_the_keys_stop_the_start_has_its_own_message() {
        use domain::fleet::KeyFile;
        let file = KeyFile::HubKey;
        let (id, args) = start_refusal(&HubPairingError::Identity(IdentityError::Damaged { file }));
        assert_eq!(id, msg::CLI_HUB_KEY_FILE_DAMAGED);
        assert_eq!(args, vec![("file", MsgArg::Text("hub-key".to_owned()))]);
        let (id, _) = start_refusal(&HubPairingError::Identity(IdentityError::Exposed {
            file,
            mode: 0o644,
        }));
        assert_eq!(id, msg::CLI_KEY_FILE_EXPOSED);
        let (id, args) = start_refusal(&HubPairingError::Identity(IdentityError::Unavailable {
            file,
            detail: "denied\u{1b}[31m".to_owned(),
        }));
        assert_eq!(id, msg::CLI_HUB_KEY_FILE_UNAVAILABLE);
        assert_eq!(
            args,
            vec![
                ("file", MsgArg::Text("hub-key".to_owned())),
                ("detail", MsgArg::Text("denied\\u{1b}[31m".to_owned())),
            ]
        );
        let (id, _) = start_refusal(&HubPairingError::TokenLife);
        assert_eq!(id, msg::CLI_HUB_KEYS_FAILED);
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
