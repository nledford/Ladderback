//! Composition root: the only place that knows every adapter (technical
//! design §3.2). One binary, several roles.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    // A command prints through `output`: `print!` and `eprint!` panic when
    // nobody reads what they print.
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::ffi::OsString;
use std::future::IntoFuture as _;
use std::io::{self, Write as _};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use api::MsgArg;
use app::ports::Random;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::random::OsRandom;
use tokio::signal::unix::{Signal, SignalKind};

use composition::LOG_CLOSE_BOUND;
use shutdown::{REQUESTS_BOUND, Step, Stopping, WORKERS_BOUND};

mod build_info;
mod check_keyring;
mod composition;
mod confirm;
mod dumps;
#[cfg(feature = "hub")]
mod hub;
mod logging;
mod mcp_call;
mod output;
mod profiles;
mod service;
mod set_password;
mod shutdown;

#[derive(Parser)]
#[command(
    name = build_info::PROGRAM,
    version = build_info::SHOWN.as_str(),
    about = "Self-hosted backup manager for rustic"
)]
struct Cli {
    /// Print plain text: no colour, and nothing drawn over what is already
    /// printed.
    #[arg(long, global = true)]
    plain: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run Ladderback on this machine and serve its web UI.
    Serve {
        /// Address to listen on. Beyond loopback only with an
        /// `--allowed-host`; with sign-in off it then warns that anyone who
        /// can reach the address can use it. Port 0 picks a free port; the
        /// address in use is printed on startup.
        #[arg(long, default_value = "127.0.0.1:9898")]
        listen: SocketAddr,
        /// A host name or address, without a port, that the web UI answers to
        /// besides loopback addresses, such as the name a reverse proxy
        /// forwards with the browser's Host (repeatable).
        #[arg(long = "allowed-host", value_name = "NAME")]
        allowed_hosts: Vec<String>,
        /// Set by the service files `install-service` writes: a service
        /// manager runs Ladderback and starts it again when it exits with
        /// the restart status, so Settings offers a restart.
        #[arg(long)]
        service_managed: bool,
        /// Serve the hub's routes for paired machines under `/sync/` as well, so that
        /// other machines can pair with this one.
        #[cfg(feature = "hub")]
        #[arg(long)]
        hub: bool,
    },
    /// Add the repository and the plans of a rustic profile to the
    /// configuration.
    ImportProfile {
        /// The rustic profile, a TOML file.
        file: std::path::PathBuf,
        /// The name the repository gets in the configuration; the profile's
        /// file name, as a slug, when it is left out.
        #[arg(long = "repo-name", value_name = "NAME")]
        repo_name: Option<String>,
        /// Print one JSON document on standard output, for a script to read,
        /// instead of sentences: the repository and the plans that were
        /// added, or the code of the refusal.
        #[arg(long)]
        json: bool,
        /// Import the commands the profile runs (its hooks, the password's
        /// command, an option rustic runs as a command) without asking.
        /// Without it they are listed and confirmed at a terminal, and
        /// anywhere else nothing is imported.
        #[arg(long)]
        yes: bool,
    },
    /// Write a plan as a rustic profile on stdout.
    ExportProfile {
        /// The ID of the plan to export.
        #[arg(long, value_name = "ID")]
        plan: String,
    },
    /// Set the admin password, which turns sign-in on, and end every
    /// session. It is typed twice at a terminal, without being shown, or
    /// read as one line of standard input; never given as an argument.
    SetPassword,
    /// Make a one-time pairing token for a machine, list the unused ones, or
    /// revoke one. Run it while the daemon is stopped.
    #[cfg(feature = "hub")]
    PairingToken {
        /// The hub's address as machines reach it, such as
        /// `https://hub.example`.
        #[arg(long, required_unless_present_any = ["list", "revoke"])]
        address: Option<String>,
        /// How long the token lives, in minutes: 5 to 1440, 15 by default.
        #[arg(long, value_name = "MINUTES")]
        ttl: Option<i64>,
        /// List the unused tokens by token ID, with when each lapses.
        #[arg(long, conflicts_with_all = ["address", "ttl", "revoke"])]
        list: bool,
        /// Revoke the unused token with this token ID.
        #[arg(long, value_name = "TOKEN_ID", conflicts_with_all = ["address", "ttl"])]
        revoke: Option<String>,
    },
    /// Pair this machine with a hub. The token is pasted at the prompt,
    /// without being shown, or read as standard input; never given as an
    /// argument.
    #[cfg(feature = "hub")]
    Pair {
        /// The hub's address, when this machine reaches it another way than
        /// the token's.
        address: Option<String>,
        /// Pair under this hostname instead of the machine's.
        #[arg(long)]
        hostname: Option<String>,
        /// Pair without asking, when standard input is the token.
        #[arg(long)]
        yes: bool,
    },
    /// Check this machine's link to the hub it paired with: connect, prove
    /// who it is, and say whether the hub holds it waiting for approval or
    /// has approved it.
    #[cfg(feature = "hub")]
    HubStatus {
        /// Connect under this hostname instead of the machine's, as `pair`
        /// takes it.
        #[arg(long)]
        hostname: Option<String>,
    },
    // Their descriptions are the catalog's (`cli-help-*`), set in `parse`.
    InstallService,
    RemoveService,
    CheckKeyring,
    // Its description is the catalog's (`cli-help-mcp`), set in `parse`.
    #[command(
        group(clap::ArgGroup::new("mode").required(true).args(["connect", "headless", "scratch"])),
        subcommand_negates_reqs = true,
        args_conflicts_with_subcommands = true
    )]
    Mcp {
        /// Drive the running daemon at this address through its API;
        /// `http://127.0.0.1:9898` when no address follows.
        #[arg(long, value_name = "URL", num_args = 0..=1, default_missing_value = mcp::DEFAULT_DAEMON_URL)]
        connect: Option<String>,
        /// Send the API token this file holds, which only its owner may
        /// read; `mcp-token` in the configuration folder, if it is there,
        /// when no file is named. Only to an https address or this computer.
        #[arg(long, value_name = "PATH", requires = "connect")]
        token_file: Option<std::path::PathBuf>,
        /// Run a session of its own on Ladderback's folders, without a
        /// daemon: refused while a daemon or another session uses them.
        #[arg(long)]
        headless: bool,
        /// Run a session of its own in a new private folder, seeded with a
        /// plan over a few generated files, and delete the folder at the end.
        #[arg(long)]
        scratch: bool,
        #[command(subcommand)]
        call: Option<McpCommand>,
    },
    /// Run one operation, read from stdin, and report events on stdout.
    /// Started by the agent; not for direct use.
    #[command(hide = true)]
    Worker,
}

/// What `ladderback mcp` does instead of serving.
#[derive(Subcommand)]
enum McpCommand {
    /// Call tools once, without a client, printing each call's result or
    /// refusal as one JSON line on standard output.
    Call(mcp_call::CallArgs),
}

/// The command line, parsed, or the status to end with when `clap` has said
/// its help, its version or a usage error. `clap` colours what it says while
/// it parses, before it has a value for `--plain`, so the raw arguments are
/// scanned for the flag first (see [`output::asked_for_plain`]).
fn parse(args: Vec<OsString>) -> Result<Cli, ExitCode> {
    let plain = output::asked_for_plain(&args);
    let mut command = Cli::command().color(output::clap_colour(plain));
    let mut matches = match command.try_get_matches_from_mut(&args) {
        Ok(matches) => matches,
        // Help is said again with the catalog's descriptions: the catalog is
        // read only to say help, never on the start of the daemon or a worker.
        Err(error) if says_help(&error) => {
            let error = with_catalog_help(command)
                .try_get_matches_from_mut(&args)
                .err()
                .unwrap_or(error);
            return Err(output::parser_status(&error));
        }
        Err(error) => return Err(output::parser_status(&error)),
    };
    Cli::from_arg_matches_mut(&mut matches)
        .map_err(|error| output::parser_status(&error.format(&mut command)))
}

/// Whether `clap` stopped to say help.
fn says_help(error: &clap::Error) -> bool {
    matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

/// `command` with the descriptions the catalog has (`cli-help-*`), in the
/// reader's language: those of the service commands, `check-keyring` and `mcp`
/// (#299 moves the rest).
fn with_catalog_help(command: clap::Command) -> clap::Command {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    command.mut_subcommands(|subcommand| {
        let id = match subcommand.get_name() {
            "install-service" => msg::CLI_HELP_INSTALL_SERVICE,
            "remove-service" => msg::CLI_HELP_REMOVE_SERVICE,
            "check-keyring" => msg::CLI_HELP_CHECK_KEYRING,
            "mcp" => msg::CLI_HELP_MCP,
            _ => return subcommand,
        };
        let about = localizer.message(id, &[]).text().to_owned();
        subcommand.about(about)
    })
}

/// `command` with the description of itself, each command below it and each
/// argument of every one of them, from the catalog in `localizer`'s
/// language.
fn described(command: clap::Command, localizer: &Localizer) -> clap::Command {
    let _ = (command, localizer);
    todo!()
}

fn main() -> ExitCode {
    let Cli { plain, command } = match parse(std::env::args_os().collect()) {
        Ok(cli) => cli,
        Err(status) => return status,
    };
    match command {
        Command::Serve {
            listen,
            allowed_hosts,
            service_managed,
            #[cfg(feature = "hub")]
            hub,
        } => serve(
            listen,
            allowed_hosts,
            service_managed,
            plain,
            #[cfg(feature = "hub")]
            hub,
        ),
        #[cfg(feature = "hub")]
        Command::PairingToken {
            address,
            ttl,
            list,
            revoke,
        } => hub::pairing_token(match (list, revoke, address) {
            (true, _, _) => hub::TokenAsk::List,
            (false, Some(id), _) => hub::TokenAsk::Revoke(id),
            (false, None, address) => hub::TokenAsk::Make {
                address: address.unwrap_or_default(),
                ttl_minutes: ttl,
            },
        }),
        #[cfg(feature = "hub")]
        Command::Pair {
            address,
            hostname,
            yes,
        } => hub::pair(hub::PairAsk {
            address,
            hostname,
            yes,
        }),
        #[cfg(feature = "hub")]
        Command::HubStatus { hostname } => hub::hub_status(hostname),
        Command::ImportProfile {
            file,
            repo_name,
            json,
            yes,
        } => profiles::import_profile(file, repo_name, output::Report::asked(json), yes),
        Command::ExportProfile { plan } => profiles::export_profile(plan),
        Command::SetPassword => set_password::set_password(),
        Command::InstallService => service::install_service(),
        Command::RemoveService => service::remove_service(),
        Command::CheckKeyring => check_keyring::check_keyring(plain),
        Command::Mcp {
            call: Some(McpCommand::Call(args)),
            ..
        } => mcp_call::call(args, plain),
        Command::Mcp {
            connect,
            token_file,
            scratch,
            call: None,
            ..
        } => serve_mcp(McpMode::of(connect, token_file, scratch), plain),
        Command::Worker => {
            // Before it reads its job, which carries the secrets (decision
            // 98). The kernel set the dumpable flag again when it started.
            for unprotected in platform::dumps::refuse() {
                let _ = writeln!(io::stderr(), "{}", dumps::worker_line(&unprotected));
            }
            engine::worker::main(build_info::NAMED.as_str())
        }
    }
}

/// What `ladderback mcp` drives: a running daemon at an address, with the
/// token file it was named, a session of its own on Ladderback's folders,
/// or one in a scratch folder (§4.6).
enum McpMode {
    Connect(String, Option<std::path::PathBuf>),
    Headless,
    Scratch,
}

impl McpMode {
    /// The mode the flags name; the parser lets exactly one through, and a
    /// token file only with `--connect`.
    fn of(connect: Option<String>, token_file: Option<std::path::PathBuf>, scratch: bool) -> Self {
        match (connect, scratch) {
            (Some(address), _) => Self::Connect(address, token_file),
            (None, true) => Self::Scratch,
            (None, false) => Self::Headless,
        }
    }
}

fn serve_mcp(mode: McpMode, plain: bool) -> ExitCode {
    let localizer = start_protected(plain, None);
    finish(&localizer, run_mcp(&localizer, mode))
}

/// The start of a process that will hold secrets, the daemon, the MCP
/// server or a one-shot command: first, before any secret is resolved or a
/// token read, its core dumps and debugger attach turned off (decision 98),
/// then its log and the reader's language, and a warning for each
/// protection it couldn't set. The daemon's log keeps its lines in `recent`
/// too, when given (#618).
pub(crate) fn start_protected(
    plain: bool,
    recent: Option<Arc<dyn app::ports::RecentLogStore>>,
) -> Localizer {
    let unprotected = platform::dumps::refuse();
    logging::install(plain, recent);
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    for unprotected in &unprotected {
        log::warn!("{}", dumps::daemon_line(&localizer, unprotected));
    }
    localizer
}

/// Serves MCP on stdin and stdout until the client closes stdin (§4.6).
/// Standard output carries protocol messages only: a refusal is said on
/// stderr, and the session logs there.
fn run_mcp(localizer: &Localizer, mode: McpMode) -> io::Result<ExitCode> {
    let session = match Session::start(localizer, &mode)? {
        Ok(session) => session,
        Err(status) => return Ok(status),
    };
    match session.serve() {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            log::error!("the MCP session ended: {error}");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// An MCP server in the mode named, with its runtime and, in a session of
/// its own, what it holds: served to a client, or called once (§4.6).
struct Session {
    runtime: tokio::runtime::Runtime,
    server: mcp::Server,
    held: Option<composition::mcp_server::Held>,
}

impl Session {
    /// Starts the server `mode` names, or says why it can't and gives the
    /// status to end with: 2 for an address that isn't one, as the parser
    /// gives for a value it rejects, and 1 otherwise.
    fn start(localizer: &Localizer, mode: &McpMode) -> io::Result<Result<Self, ExitCode>> {
        // Checked here, in the catalog's words, like `--allowed-host`; exit
        // status 2 is the parser's own for a value it rejects. The address
        // isn't repeated: it may hold a password.
        let daemon = match mode {
            McpMode::Connect(address, token_file) => match mcp::DaemonUrl::parse(address) {
                Ok(daemon) => Some((daemon, token_file.as_deref())),
                Err(_) => {
                    say(localizer, msg::CLI_MCP_CONNECT_INVALID, &[])?;
                    return Ok(Err(ExitCode::from(2)));
                }
            },
            McpMode::Headless | McpMode::Scratch => None,
        };
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                say(
                    localizer,
                    msg::CLI_CANNOT_START_RUNTIME,
                    &[("detail", text(error))],
                )?;
                return Ok(Err(ExitCode::FAILURE));
            }
        };
        // A session of its own holds the data folder from here, as `serve`
        // does; connect mode has its address parsed above, and reads its
        // token file.
        let started = match (daemon, mode) {
            (Some((daemon, token_file)), _) => composition::mcp_server::connect(
                daemon,
                token_file,
                platform::dirs::config_dir().as_deref(),
            )
            .map(|server| (server, None)),
            (None, McpMode::Scratch) => runtime
                .block_on(composition::mcp_server::scratch(&OsRandom))
                .map(|(server, held)| (server, Some(held))),
            (None, _) => runtime
                .block_on(composition::mcp_server::headless())
                .map(|(server, held)| (server, Some(held))),
        };
        match started {
            Ok((server, held)) => Ok(Ok(Self {
                runtime,
                server,
                held,
            })),
            Err(refused) => {
                say(localizer, refused.message, &refused.args)?;
                Ok(Err(ExitCode::FAILURE))
            }
        }
    }

    /// Calls `call`'s tool once.
    fn call(&self, call: mcp::Call) -> mcp::Answer {
        self.runtime.block_on(self.server.call(call))
    }

    /// Serves the session on stdin and stdout until the client closes
    /// stdin, then ends it.
    fn serve(self) -> Result<(), mcp::SessionFailed> {
        let Self {
            runtime,
            server,
            held,
        } = self;
        let ended = runtime.block_on(server.serve(tokio::io::stdin(), tokio::io::stdout()));
        Self::stop(runtime, held);
        ended
    }

    /// Ends the session.
    fn end(self) {
        Self::stop(self.runtime, self.held);
    }

    /// A session of its own stops as `serve` does, inside the runtime: its
    /// workers stopped, its runs frozen, what still runs ended as
    /// `daemon-shut-down` and the log closed (decision 92).
    fn stop(runtime: tokio::runtime::Runtime, held: Option<composition::mcp_server::Held>) {
        if let Some(held) = &held {
            runtime.block_on(held.stop());
        }
        // A read of stdin can't be cancelled: the runtime isn't waited for.
        runtime.shutdown_background();
        // The data folder is let go once nothing of the session can touch it.
        if let Some(held) = held {
            held.release();
        }
    }
}

fn serve(
    listen: SocketAddr,
    allowed_hosts: Vec<String>,
    service_managed: bool,
    plain: bool,
    #[cfg(feature = "hub")] hub: bool,
) -> ExitCode {
    // Before the log, so that it holds the start and every line after it.
    let recent_log: Arc<dyn app::ports::RecentLogStore> =
        Arc::new(app::recent_log::RecentLogRing::new());
    let localizer = start_protected(plain, Some(Arc::clone(&recent_log)));
    log::info!("{} starting", *build_info::NAMED);
    finish(
        &localizer,
        run_daemon(
            &localizer,
            listen,
            allowed_hosts,
            service_managed,
            recent_log,
            #[cfg(feature = "hub")]
            hub,
        ),
    )
}

/// Starts the daemon and runs it until it is told to stop, and says how it
/// ended. What it says on stderr before it serves is written through `say`:
/// a startup line that nobody reads stops it, as anything that prints does.
/// Once it serves it only logs, and the logger drops what it cannot write.
/// A stop asked for as a restart ends with the status the service manager
/// starts it again on (decision 94).
fn run_daemon(
    localizer: &Localizer,
    listen: SocketAddr,
    allowed_hosts: Vec<String>,
    service_managed: bool,
    recent_log: Arc<dyn app::ports::RecentLogStore>,
    #[cfg(feature = "hub")] hub: bool,
) -> io::Result<ExitCode> {
    // Checked here, not by the argument parser, so that the refusal is said
    // in the catalog's words and the person's language (§8.4). Exit status 2
    // is the parser's own for a value it rejects.
    let mut hosts = Vec::with_capacity(allowed_hosts.len());
    for name in allowed_hosts {
        match http_adapter::validate_host_name(&name) {
            Ok(host) => hosts.push(host),
            Err(refusal) => {
                say(
                    localizer,
                    msg::CLI_INVALID_ALLOWED_HOST,
                    &[("name", text(refusal.0))],
                )?;
                return Ok(ExitCode::from(2));
            }
        }
    }
    // Sign-in is decided once, at start, by whether an admin password is
    // kept (§10.1, decisions 70 and 87); beyond loopback the daemon needs a
    // host name, with sign-in on or off.
    // The one source of randomness, for sign-in's and API tokens' secrets
    // and the scheduler's jitter alike (§3.2).
    let random: Arc<dyn Random> = Arc::new(OsRandom);
    let sign_in = match composition::sign_in() {
        Ok(sign_in) => sign_in,
        Err(refused) => {
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let sign_in_on = sign_in.is_on();
    let exposure = match http_adapter::startup_rule(listen, &hosts) {
        Ok(exposure) => exposure,
        Err(http_adapter::StartupRefusal::NoAllowedHost(address)) => {
            say(
                localizer,
                msg::CLI_REFUSE_NO_ALLOWED_HOST,
                &[("address", text(address))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    // Open on the network with sign-in off is said on stderr once the
    // address is bound (below), not logged: no log setting hides it.
    match (exposure, sign_in_on) {
        (http_adapter::Exposure::Network, true) => log::warn!(
            "listening on {listen} beyond loopback over plain HTTP: the password crosses the network unencrypted unless a reverse proxy in front adds TLS"
        ),
        (http_adapter::Exposure::Loopback, false) => log::warn!(
            "sign-in is off: anyone on this machine can use Ladderback; set an admin password with `ladderback set-password`"
        ),
        (http_adapter::Exposure::Network, false) | (http_adapter::Exposure::Loopback, true) => {}
    }
    let proxied: Vec<String> = http_adapter::proxied_hosts(&hosts)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    // Before the listener: a daemon that cannot serve must not print that it
    // is listening. By the time it is handed back it has locked the data
    // folder and ended what an earlier run left unfinished, so no request is
    // served before the log says what really happened (§4.2, §9).
    let composition::Daemon {
        state,
        log: operation_log,
        lock,
        background,
        workers,
    } = match runtime.block_on(composition::startup::daemon(
        sign_in,
        Arc::clone(&random),
        exposure,
        service_managed,
        recent_log,
    )) {
        Ok(daemon) => daemon,
        Err(refused) => {
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    // `serve --hub` carries the pairing handshake at `/sync/v1` on this
    // listener, whose sessions have no TLS of the hub's own (decision 120).
    #[cfg(feature = "hub")]
    let mut state = state;
    #[cfg(feature = "hub")]
    if hub && let Some(data_dir) = platform::dirs::data_dir() {
        let pairing = hub::hub_pairing(&data_dir, Arc::clone(&operation_log.store));
        state.sync = http_adapter::SyncHub::serving(Arc::new(pairing));
        log::info!("serving the hub's pairing at {}", api::sync::PATH);
    }
    // What came of serving, and the stop once it began; what is said on
    // stderr before then can fail the whole of it.
    let ran: io::Result<(io::Result<ExitCode>, Option<Stopping>)> = runtime.block_on(async {
        // Before the listener: a stop signal is listened for from here on,
        // so one that comes once the daemon says it listens takes the
        // orderly stop, not the default action that kills it (#628). One
        // that can't be listened for is `None`, and never comes.
        let interrupt = tokio::signal::unix::signal(SignalKind::interrupt()).ok();
        let terminate = tokio::signal::unix::signal(SignalKind::terminate()).ok();
        let listener = match http_adapter::GatedListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                say(
                    localizer,
                    msg::CLI_CANNOT_LISTEN,
                    &[("address", text(listen)), ("detail", text(error))],
                )?;
                return Ok((Ok(ExitCode::FAILURE), None));
            }
        };
        let bound = listener.local_addr();
        // With sign-in off on loopback, a host name beyond it is one a
        // reverse proxy brings people in by: each is said, as the network
        // is (§10.1, decision 87).
        if exposure == http_adapter::Exposure::Loopback && !sign_in_on {
            for host in &proxied {
                say(
                    localizer,
                    msg::CLI_WARN_SIGN_IN_OFF_BEHIND_PROXY,
                    &[("host", text(host))],
                )?;
            }
        }
        if exposure == http_adapter::Exposure::Network && !sign_in_on {
            let address = bound.as_ref().map_or(listen, |addr| *addr);
            say(
                localizer,
                msg::CLI_WARN_NETWORK_SIGN_IN_OFF,
                &[("address", text(address))],
            )?;
        }
        match bound {
            Ok(addr) => say(
                localizer,
                msg::CLI_LISTENING,
                &[("url", text(format!("http://{addr}")))],
            )?,
            Err(error) => say(
                localizer,
                msg::CLI_LISTENING_UNKNOWN,
                &[("detail", text(error))],
            )?,
        }
        // The queue's leftovers and the scheduler start once the daemon
        // listens, and stop with it: they run on what the log still holds
        // open (§5, decision 73).
        let runs = background.runs();
        let restart = state.restart.clone();
        let scheduled = tokio::spawn(background.run(random));
        let app = http_adapter::router(
            Arc::new(http_adapter::EmbeddedUi),
            http_adapter::RequestChecks::new(hosts),
            state,
        );
        let (cause_tx, mut cause) = tokio::sync::oneshot::channel();
        let serving = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<http_adapter::PeerAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = cause_tx.send(shutdown_signal(interrupt, terminate, restart).await);
        })
        .into_future();
        tokio::pin!(serving);
        // Told to stop, the server stops taking connections and waits for
        // the requests still open; the stop begins then, not once they end.
        // With no request open, the server ends in the same poll that sees
        // the signal, so its cause is looked for then too.
        let (cause, served) = tokio::select! {
            served = &mut serving => (cause.try_recv().unwrap_or(Cause::ServerEnded), Some(served)),
            Ok(cause) = &mut cause => (cause, None),
        };
        // At the signal, before anything else: no scheduled run starts, the
        // workers are asked to stop, as a cancel stops one, so a backup that
        // archives winds down and saves the index of the packs it wrote
        // (decision 50), and every run is frozen where it stands. A run
        // frozen starts no worker, runs no hook, records no end and
        // announces none, whatever its worker, its timers or its turn in the
        // queue do from now on; a worker it lets go of still gets its SIGTERM
        // and its grace, since the stop was asked for first (decision 92).
        scheduled.abort();
        let workers_stopped = workers.stop();
        runs.freeze();
        log::info!("stopping ({})", cause.said());
        let mut stop = Stopping::new(shutdown::LIMIT);
        let served = match served {
            Some(served) => {
                stop.waited(Step::OpenRequests, Duration::ZERO, true);
                Some(served)
            }
            None => stop.wait(Step::OpenRequests, REQUESTS_BOUND, serving).await,
        };
        stop.wait(Step::Workers, WORKERS_BOUND, workers_stopped)
            .await;
        // Then the operation log ends what is still running as
        // `daemon-shut-down` and closes, inside the runtime, so a write in
        // flight ends as it would have rather than being cut off, and nothing
        // of the log outlives the daemon. A stop that never gets here, or
        // gives this step up, leaves them to the next start, which ends and
        // announces them as `daemon-stopped` (§9, decision 92).
        stop.wait(
            Step::OperationLog,
            LOG_CLOSE_BOUND,
            operation_log.close_at_orderly_stop(),
        )
        .await;
        let code = match served {
            Some(Err(error)) => say(localizer, msg::CLI_SERVER_ERROR, &[("detail", text(error))])
                .map(|()| ExitCode::FAILURE),
            Some(Ok(())) | None => Ok(cause.status()),
        };
        Ok((code, Some(stop)))
    });
    match ran {
        Ok((code, Some(mut stop))) => {
            // What the runtime's tasks were still doing gets what is left of
            // the limit; a blocking task still running then is left behind.
            let left = stop.left();
            let began = Instant::now();
            runtime.shutdown_timeout(left);
            let took = began.elapsed();
            stop.waited(Step::Tasks, took, took < left);
            // The lock is let go last: until then a task may still hold the
            // operation log, and another daemon must not recover that log
            // from under it (§9).
            drop(lock);
            stop.finish();
            code
        }
        // The daemon never began to serve: the log closes as a refusal's
        // does, and the lock is let go once the runtime is gone.
        ran => {
            runtime.block_on(operation_log.close());
            drop(runtime);
            drop(lock);
            ran.and_then(|(code, _)| code)
        }
    }
}

/// The line `say` prints: one of Ladderback's own messages after the
/// program's name.
fn said(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    format!("ladderback: {}\n", localizer.message(id, args))
}

/// Prints one of Ladderback's own messages on stderr after the program's name.
fn say(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> io::Result<()> {
    output::to_stderr(&said(localizer, id, args))
}

/// The status a command ends with, given what writing its output came to
/// (see [`output::status_after`]); a write that failed for another reason
/// than a closed pipe is said as `cannot write the result`.
fn finish(localizer: &Localizer, written: io::Result<ExitCode>) -> ExitCode {
    output::status_after(written, |error| {
        // If this cannot be said either, there is nothing left to say it on.
        let _ = say(
            localizer,
            msg::CLI_CANNOT_WRITE_OUTPUT,
            &[("detail", text(error))],
        );
    })
}

/// A value shown as the text it prints as.
fn text(value: impl std::fmt::Display) -> MsgArg {
    MsgArg::Text(value.to_string())
}

/// Why the daemon stops: a signal, a restart asked for, or the server's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    /// Ctrl-C (`SIGINT`) or `SIGTERM`: what systemd and launchd send, and
    /// what any other program running as the user may.
    Signal(&'static str),
    /// A restart asked for in Settings, by whom (decision 94).
    Restart(http_adapter::RestartAsker),
    /// The server ended of itself.
    ServerEnded,
}

impl Cause {
    /// The cause as the log's `stopping (…)` line says it.
    fn said(self) -> &'static str {
        match self {
            Self::Signal(name) => name,
            Self::Restart(asker) => http_adapter::asked_by(asker),
            Self::ServerEnded => "the server ended",
        }
    }

    /// The status an orderly stop for this cause ends with. The service
    /// manager starts the daemon again after any stop it didn't ask for,
    /// whatever the status (#1035), so a signal's is 0, a clean end for a
    /// `serve` in a terminal; a restart's is the one systemd doesn't mark
    /// the unit failed for (decision 94).
    fn status(self) -> ExitCode {
        match self {
            Self::Restart(_) => ExitCode::from(platform::service::RESTART_STATUS),
            Self::Signal(_) | Self::ServerEnded => ExitCode::SUCCESS,
        }
    }
}

/// Resolves on Ctrl-C (`interrupt`), SIGTERM (`terminate`), or once a
/// restart asked for is due, with its cause.
async fn shutdown_signal(
    interrupt: Option<Signal>,
    terminate: Option<Signal>,
    restart: http_adapter::Restart,
) -> Cause {
    tokio::select! {
        () = received(interrupt) => Cause::Signal("SIGINT"),
        () = received(terminate) => Cause::Signal("SIGTERM"),
        asker = restart.due() => Cause::Restart(asker),
    }
}

/// Resolves when `signal` comes, and never when there is none.
async fn received(signal: Option<Signal>) {
    match signal {
        Some(mut signal) => {
            signal.recv().await;
        }
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_the_program_says_is_its_name_and_a_sentence() {
        let line = said(
            &Localizer::new(&[]),
            msg::CLI_CANNOT_WRITE_OUTPUT,
            &[("detail", text("No space left on device"))],
        );
        assert_eq!(
            line,
            "ladderback: cannot write the result: No space left on device\n"
        );
    }

    #[test]
    fn only_a_restart_ends_with_the_status_the_service_manager_restarts_on() {
        let restart = Cause::Restart(http_adapter::RestartAsker::SignInOff);
        assert_eq!(restart.status(), ExitCode::from(75));
        assert_eq!(Cause::Signal("SIGTERM").status(), ExitCode::SUCCESS);
        assert_eq!(Cause::ServerEnded.status(), ExitCode::SUCCESS);
    }

    fn parsed(args: &[&str]) -> Result<Cli, clap::error::ErrorKind> {
        Cli::try_parse_from(std::iter::once("ladderback").chain(args.iter().copied()))
            .map_err(|error| error.kind())
    }

    /// What `--help` could say a command or an argument does, named by its
    /// path from the program: its short and long description, and whether
    /// `--help` hides it.
    struct Description {
        name: String,
        hidden: bool,
        short: Option<String>,
        long: Option<String>,
    }

    /// Adds to `found` each command and argument at or below `command`,
    /// named `path`, but for `clap`'s own: its `--help` and `--version`
    /// options and its `help` command, whose words stay English (§8.4).
    fn descriptions(command: &clap::Command, path: &str, found: &mut Vec<Description>) {
        let text = |styled: Option<&clap::builder::StyledStr>| styled.map(ToString::to_string);
        found.push(Description {
            name: path.to_owned(),
            hidden: command.is_hide_set(),
            short: text(command.get_about()),
            long: text(command.get_long_about()),
        });
        let arguments = command.get_arguments().filter(|argument| {
            !matches!(
                argument.get_action(),
                clap::ArgAction::Help
                    | clap::ArgAction::HelpShort
                    | clap::ArgAction::HelpLong
                    | clap::ArgAction::Version
            )
        });
        for argument in arguments {
            found.push(Description {
                name: format!("{path} {}", argument.get_id()),
                hidden: argument.is_hide_set(),
                short: text(argument.get_help()),
                long: text(argument.get_long_help()),
            });
        }
        let subcommands = command
            .get_subcommands()
            .filter(|subcommand| subcommand.get_name() != "help");
        for subcommand in subcommands {
            let path = format!("{path} {}", subcommand.get_name());
            descriptions(subcommand, &path, found);
        }
    }

    /// The command tree built, as `clap` builds it to parse and so as
    /// `parse` describes it: a global argument, as `--plain`, is in every
    /// command.
    fn built() -> clap::Command {
        let mut command = Cli::command();
        command.build();
        command
    }

    /// No description is written in the code, not even as a doc comment
    /// `clap` would take for one (technical design §8.4): each is the
    /// catalog's, set only when help is said.
    #[test]
    fn no_description_is_written_in_the_code() {
        let mut found = Vec::new();
        descriptions(&built(), "ladderback", &mut found);
        let written: Vec<String> = found
            .into_iter()
            .filter_map(|description| {
                let shown = if description.hidden {
                    "hidden"
                } else {
                    "shown"
                };
                let text = description.short.or(description.long)?;
                Some(format!("{} ({shown}): {text}", description.name))
            })
            .collect();
        assert_eq!(written, Vec::<String>::new());
    }

    /// Every command and argument `--help` shows is described from the
    /// catalog (technical design §8.4): under `en-XA` none is left without a
    /// description or in English.
    #[cfg(feature = "pseudo")]
    #[test]
    fn every_command_and_argument_is_described_from_the_catalog() {
        let en_xa = PosixEnv {
            language: Some("en-XA".to_owned()),
            ..PosixEnv::default()
        };
        let localizer = Localizer::new(&en_xa.preferences());
        let mut found = Vec::new();
        descriptions(&described(built(), &localizer), "ladderback", &mut found);
        let transformed = |text: &String| !text.is_ascii();
        let english: Vec<String> = found
            .into_iter()
            .filter(|description| {
                !description.hidden
                    && !(description.short.as_ref().is_some_and(transformed)
                        && description.long.as_ref().is_none_or(transformed))
            })
            .map(|description| format!("{}: {:?}", description.name, description.short))
            .collect();
        assert_eq!(english, Vec::<String>::new());
    }

    #[test]
    fn mcp_still_needs_a_mode_to_serve_but_not_to_call() {
        assert_eq!(
            parsed(&["mcp"]).err(),
            Some(clap::error::ErrorKind::MissingRequiredArgument)
        );
        assert!(matches!(
            parsed(&["mcp", "--headless"]),
            Ok(Cli {
                command: Command::Mcp { call: None, .. },
                ..
            })
        ));
        assert!(matches!(
            parsed(&["mcp", "call", "status"]),
            Ok(Cli {
                command: Command::Mcp {
                    call: Some(McpCommand::Call(_)),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn a_mode_of_mcp_is_refused_before_call_which_takes_its_own() {
        for args in [
            ["mcp", "--headless", "call", "status"].as_slice(),
            &["mcp", "--scratch", "call", "status"],
            &["mcp", "--connect", "http://127.0.0.1:1/", "call", "status"],
        ] {
            assert_eq!(
                parsed(args).err(),
                Some(clap::error::ErrorKind::ArgumentConflict),
                "{args:?}"
            );
        }
    }
}
