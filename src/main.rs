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
mod panic_report;
mod profiles;
mod service;
mod set_password;
mod shutdown;

// Every description `--help` gives, of the program, a command or an
// argument, is the catalog's (`cli-help-*`), set by `described` when help is
// said: none is written here.
#[derive(Parser)]
#[command(name = build_info::PROGRAM, version = build_info::SHOWN.as_str())]
struct Cli {
    #[arg(long, global = true)]
    plain: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long = "allowed-host", value_name = "NAME")]
        allowed_hosts: Vec<String>,
        #[arg(long)]
        service_managed: bool,
        #[cfg(feature = "hub")]
        #[arg(
            long,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true",
            value_name = "ON"
        )]
        hub: Option<bool>,
    },
    ImportProfile {
        file: std::path::PathBuf,
        #[arg(long = "repo-name", value_name = "NAME")]
        repo_name: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        yes: bool,
    },
    ExportProfile {
        #[arg(long, value_name = "ID")]
        plan: String,
    },
    SetPassword,
    #[cfg(feature = "hub")]
    PairingToken {
        #[arg(long, required_unless_present_any = ["list", "revoke", "pending", "approve", "forget_computer"])]
        address: Option<String>,
        #[arg(long, value_name = "MINUTES")]
        ttl: Option<i64>,
        #[arg(long, requires = "address", conflicts_with_all = ["list", "revoke"])]
        certificate_authority: bool,
        #[arg(long, conflicts_with_all = ["address", "ttl", "revoke", "pending", "approve"])]
        list: bool,
        #[arg(long, value_name = "TOKEN_ID", conflicts_with_all = ["address", "ttl", "pending", "approve"])]
        revoke: Option<String>,
        #[arg(long, conflicts_with_all = ["address", "ttl", "approve"])]
        pending: bool,
        #[arg(
            long,
            value_name = "HOSTNAME",
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            conflicts_with_all = ["address", "ttl"]
        )]
        approve: Option<String>,
        #[arg(long, requires = "approve")]
        yes: bool,
        #[arg(
            long,
            value_name = "HOSTNAME",
            value_parser = clap::builder::NonEmptyStringValueParser::new(),
            conflicts_with_all = ["address", "ttl", "list", "revoke", "pending", "approve"]
        )]
        forget_computer: Option<String>,
    },
    #[cfg(feature = "hub")]
    Pair {
        address: Option<String>,
        #[arg(long)]
        hostname: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    #[cfg(feature = "hub")]
    HubStatus {
        #[arg(long)]
        hostname: Option<String>,
    },
    InstallService,
    RemoveService,
    CheckKeyring,
    #[command(
        group(clap::ArgGroup::new("mode").required(true).args(["connect", "headless", "scratch"])),
        subcommand_negates_reqs = true,
        args_conflicts_with_subcommands = true
    )]
    Mcp {
        #[arg(long, value_name = "URL", num_args = 0..=1, default_missing_value = mcp::DEFAULT_DAEMON_URL)]
        connect: Option<String>,
        #[arg(long, value_name = "PATH", requires = "connect")]
        token_file: Option<std::path::PathBuf>,
        #[arg(long)]
        headless: bool,
        #[arg(long)]
        scratch: bool,
        #[command(subcommand)]
        call: Option<McpCommand>,
    },
    // Runs one operation, read from stdin, and reports events on stdout.
    // Started by the daemon; not for direct use, so `--help` hides it and the
    // catalog doesn't describe it.
    #[command(hide = true)]
    Worker,
}

// What `ladderback mcp` does instead of serving. Not a doc comment: `clap`
// would take it for the description of `mcp`.
#[derive(Subcommand)]
enum McpCommand {
    // Calls tools once, without a client.
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
            let localizer = Localizer::new(&PosixEnv::from_process().preferences());
            let error = described(command, &localizer)
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

/// `command` with the description of itself, each command below it and each
/// argument of every one of them, from the catalog in `localizer`'s
/// language.
fn described(command: clap::Command, localizer: &Localizer) -> clap::Command {
    described_at(command, "", localizer)
}

/// [`described`] for the command at `path`, its names below the program
/// (`mcp call`; empty for the program).
fn described_at(command: clap::Command, path: &str, localizer: &Localizer) -> clap::Command {
    let said = |id| localizer.message(id, &[]).into_text();
    let command = match description(path, None) {
        Some(id) => command.about(said(id)),
        None => command,
    };
    let command = command.mut_args(|argument| {
        // `clap` copies a global argument, as `--plain`, into each command
        // below the program when it builds the tree: it is the program's.
        let at = if argument.is_global_set() { "" } else { path };
        match description(at, Some(argument.get_id().as_str())) {
            Some(id) => argument.help(said(id)),
            None => argument,
        }
    });
    command.mut_subcommands(|subcommand| {
        let path = match path {
            "" => subcommand.get_name().to_owned(),
            path => format!("{path} {}", subcommand.get_name()),
        };
        described_at(subcommand, &path, localizer)
    })
}

/// The message that describes the command at `path` (as in
/// [`described_at`]), or its `argument`, by the argument's ID. None for one
/// `--help` hides.
fn description(path: &str, argument: Option<&str>) -> Option<MessageId> {
    Some(match (path, argument) {
        ("", None) => msg::CLI_HELP,
        ("", Some("plain")) => msg::CLI_HELP_PLAIN,
        ("serve", None) => msg::CLI_HELP_SERVE,
        ("serve", Some("listen")) => msg::CLI_HELP_SERVE_LISTEN,
        ("serve", Some("allowed_hosts")) => msg::CLI_HELP_SERVE_ALLOWED_HOST,
        ("serve", Some("service_managed")) => msg::CLI_HELP_SERVE_SERVICE_MANAGED,
        ("serve", Some("hub")) => msg::CLI_HELP_SERVE_HUB,
        ("import-profile", None) => msg::CLI_HELP_IMPORT_PROFILE,
        ("import-profile", Some("file")) => msg::CLI_HELP_IMPORT_PROFILE_FILE,
        ("import-profile", Some("repo_name")) => msg::CLI_HELP_IMPORT_PROFILE_REPO_NAME,
        ("import-profile", Some("json")) => msg::CLI_HELP_IMPORT_PROFILE_JSON,
        ("import-profile", Some("yes")) => msg::CLI_HELP_IMPORT_PROFILE_YES,
        ("export-profile", None) => msg::CLI_HELP_EXPORT_PROFILE,
        ("export-profile", Some("plan")) => msg::CLI_HELP_EXPORT_PROFILE_PLAN,
        ("set-password", None) => msg::CLI_HELP_SET_PASSWORD,
        ("pairing-token", None) => msg::CLI_HELP_PAIRING_TOKEN,
        ("pairing-token", Some("address")) => msg::CLI_HELP_PAIRING_TOKEN_ADDRESS,
        ("pairing-token", Some("certificate_authority")) => {
            msg::CLI_HELP_PAIRING_TOKEN_CERTIFICATE_AUTHORITY
        }
        ("pairing-token", Some("ttl")) => msg::CLI_HELP_PAIRING_TOKEN_TTL,
        ("pairing-token", Some("list")) => msg::CLI_HELP_PAIRING_TOKEN_LIST,
        ("pairing-token", Some("revoke")) => msg::CLI_HELP_PAIRING_TOKEN_REVOKE,
        ("pairing-token", Some("pending")) => msg::CLI_HELP_PAIRING_TOKEN_PENDING,
        ("pairing-token", Some("approve")) => msg::CLI_HELP_PAIRING_TOKEN_APPROVE,
        ("pairing-token", Some("yes")) => msg::CLI_HELP_PAIRING_TOKEN_YES,
        ("pairing-token", Some("forget_computer")) => msg::CLI_HELP_PAIRING_TOKEN_FORGET_COMPUTER,
        ("pair", None) => msg::CLI_HELP_PAIR,
        ("pair", Some("address")) => msg::CLI_HELP_PAIR_ADDRESS,
        ("pair", Some("hostname")) => msg::CLI_HELP_PAIR_HOSTNAME,
        ("pair", Some("yes")) => msg::CLI_HELP_PAIR_YES,
        ("hub-status", None) => msg::CLI_HELP_HUB_STATUS,
        ("hub-status", Some("hostname")) => msg::CLI_HELP_HUB_STATUS_HOSTNAME,
        ("install-service", None) => msg::CLI_HELP_INSTALL_SERVICE,
        ("remove-service", None) => msg::CLI_HELP_REMOVE_SERVICE,
        ("check-keyring", None) => msg::CLI_HELP_CHECK_KEYRING,
        ("mcp", None) => msg::CLI_HELP_MCP,
        ("mcp", Some("connect")) => msg::CLI_HELP_MCP_CONNECT,
        ("mcp", Some("token_file")) => msg::CLI_HELP_MCP_TOKEN_FILE,
        ("mcp", Some("headless")) => msg::CLI_HELP_MCP_HEADLESS,
        ("mcp", Some("scratch")) => msg::CLI_HELP_MCP_SCRATCH,
        ("mcp call", None) => msg::CLI_HELP_MCP_CALL,
        ("mcp call", Some("tool")) => msg::CLI_HELP_MCP_CALL_TOOL,
        ("mcp call", Some("words")) => msg::CLI_HELP_MCP_CALL_WORDS,
        ("mcp call", Some("arguments")) => msg::CLI_HELP_MCP_CALL_ARGS,
        ("mcp call", Some("script")) => msg::CLI_HELP_MCP_CALL_SCRIPT,
        ("mcp call", Some("keep_going")) => msg::CLI_HELP_MCP_CALL_KEEP_GOING,
        ("mcp call", Some("connect")) => msg::CLI_HELP_MCP_CALL_CONNECT,
        ("mcp call", Some("token_file")) => msg::CLI_HELP_MCP_CALL_TOKEN_FILE,
        ("mcp call", Some("headless")) => msg::CLI_HELP_MCP_CALL_HEADLESS,
        ("mcp call", Some("scratch")) => msg::CLI_HELP_MCP_CALL_SCRATCH,
        _ => return None,
    })
}

fn main() -> ExitCode {
    // First, for every command: a panic's message can quote a secret, the
    // daemon's in its service log, a prompt's on the terminal (#1384). A
    // worker replaces it with its own, which masks the message (§4.1).
    panic_report::install();
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
            certificate_authority,
            list,
            revoke,
            pending,
            approve,
            yes,
            forget_computer,
        } => hub::pairing_token(
            match (forget_computer, list, revoke, pending, approve, address) {
                (Some(hostname), ..) => hub::TokenAsk::ForgetComputer(hostname),
                (None, true, ..) => hub::TokenAsk::List,
                (None, false, Some(id), ..) => hub::TokenAsk::Revoke(id),
                (None, false, None, true, ..) => hub::TokenAsk::Pending,
                (None, false, None, false, Some(hostname), _) => {
                    hub::TokenAsk::Approve { hostname, yes }
                }
                (None, false, None, false, None, address) => hub::TokenAsk::Make {
                    address: address.unwrap_or_default(),
                    ttl_minutes: ttl,
                    certificate_authority,
                },
            },
        ),
        #[cfg(feature = "hub")]
        Command::Pair {
            address,
            hostname,
            yes,
        } => hub::pair(
            hub::PairAsk {
                address,
                hostname,
                yes,
            },
            plain,
        ),
        #[cfg(feature = "hub")]
        Command::HubStatus { hostname } => hub::hub_status(hostname),
        Command::ImportProfile {
            file,
            repo_name,
            json,
            yes,
        } => profiles::import_profile(file, repo_name, output::Report::asked(json), yes),
        Command::ExportProfile { plan } => profiles::export_profile(plan),
        Command::SetPassword => set_password::set_password(plain),
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
    let recent = recent_log();
    let localizer = start_protected(plain, Some(Arc::clone(&recent)));
    finish(&localizer, run_mcp(&localizer, mode, recent))
}

/// The log an MCP server keeps of its own, from its start, as the daemon
/// keeps its recent log (#618): what `read_log` reads in a session of its
/// own. Connect mode reads the daemon's instead.
pub(crate) fn recent_log() -> Arc<dyn app::ports::RecentLogStore> {
    Arc::new(app::recent_log::RecentLogRing::new())
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
fn run_mcp(
    localizer: &Localizer,
    mode: McpMode,
    recent: Arc<dyn app::ports::RecentLogStore>,
) -> io::Result<ExitCode> {
    let session = match Session::start(localizer, &mode, recent)? {
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
    /// gives for a value it rejects, and 1 otherwise. A session of its own
    /// keeps its log in `recent`.
    fn start(
        localizer: &Localizer,
        mode: &McpMode,
        recent: Arc<dyn app::ports::RecentLogStore>,
    ) -> io::Result<Result<Self, ExitCode>> {
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
                .block_on(composition::mcp_server::scratch(&OsRandom, recent))
                .map(|(server, held)| (server, Some(held))),
            (None, _) => runtime
                .block_on(composition::mcp_server::headless(recent))
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
    listen: Option<SocketAddr>,
    allowed_hosts: Vec<String>,
    service_managed: bool,
    plain: bool,
    #[cfg(feature = "hub")] hub: Option<bool>,
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
    listen: Option<SocketAddr>,
    allowed_hosts: Vec<String>,
    service_managed: bool,
    recent_log: Arc<dyn app::ports::RecentLogStore>,
    #[cfg(feature = "hub")] hub: Option<bool>,
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
    // Before the listener: a daemon that cannot serve must not print that it
    // is listening. By the time it is handed back it has locked the data
    // folder and ended what an earlier run left unfinished, so no request is
    // served before the log says what really happened (§4.2, §9). It reads
    // the configuration under that lock, `[serve]` with the rest.
    let composition::Daemon {
        mut state,
        log: operation_log,
        lock,
        background,
        workers,
        serve: configured,
    } = match runtime.block_on(composition::startup::daemon(
        sign_in,
        Arc::clone(&random),
        service_managed,
        recent_log,
    )) {
        Ok(daemon) => daemon,
        Err(refused) => {
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    // A start that stops from here on closes the log and lets go of the
    // lock, as every refusal after the lock does.
    let stop = |said: io::Result<()>| -> io::Result<ExitCode> {
        runtime.block_on(operation_log.close());
        said.map(|()| ExitCode::FAILURE)
    };
    // What the flags leave unsaid comes from the `[serve]` table of
    // config.toml, so the service `install-service` writes runs as it says
    // with no flag of its own (§16, decision 167). Its host names are
    // checked as the flag's are, and refused in words that name the file.
    let listen = listen
        .or(configured.listen)
        .unwrap_or(domain::planning::DEFAULT_LISTEN);
    if hosts.is_empty() {
        for name in configured.allowed_hosts {
            match http_adapter::validate_host_name(&name) {
                Ok(host) => hosts.push(host),
                Err(refusal) => {
                    return stop(say(
                        localizer,
                        msg::CLI_INVALID_ALLOWED_HOST_CONFIG,
                        &[("name", text(refusal.0))],
                    ));
                }
            }
        }
    }
    let exposure = match http_adapter::startup_rule(listen, &hosts) {
        Ok(exposure) => exposure,
        Err(http_adapter::StartupRefusal::NoAllowedHost(address)) => {
            return stop(say(
                localizer,
                msg::CLI_REFUSE_NO_ALLOWED_HOST,
                &[("address", text(address))],
            ));
        }
    };
    state.exposure = exposure;
    if exposure == http_adapter::Exposure::Loopback && !sign_in_on {
        log::warn!(
            "sign-in is off: anyone on this machine can use Ladderback; set an admin password with `ladderback set-password`"
        );
    }
    let proxied: Vec<String> = http_adapter::proxied_hosts(&hosts)
        .into_iter()
        .map(str::to_owned)
        .collect();
    #[cfg(not(feature = "hub"))]
    if configured.hub {
        log::warn!(
            "hub mode is on in config.toml, but this build has no hub: it serves as a machine"
        );
    }
    // Hub mode turned off, by config.toml or `--hub=false`, stays on while
    // machines are paired with this hub, so none of them loses its hub and
    // this machine's backups go on: the daemon says which machines keep it
    // on, to be removed first (§16, decision 167).
    #[cfg(feature = "hub")]
    let hub = hub.unwrap_or(configured.hub)
        || match hub::hub_mode_kept_on(
            localizer,
            &runtime,
            operation_log.store.as_ref(),
            platform::dirs::data_dir().as_deref(),
        ) {
            Ok(kept_on) => kept_on,
            Err(error) => return stop(Err(error)),
        };
    // `serve --hub` serves its built-in TLS on this listener, whose
    // sessions carry the pairing handshake at `/sync/v1` with the binding
    // to the session (decision 120), and the page's pairing tokens under
    // `/api/v1/hub`, confirmed by the admin password when sign-in is on
    // (decision 121). A hub takes every connection through its front door,
    // which serves plain HTTP only from loopback. A TLS key it can't serve
    // fails closed ("The hub key"): the door serves only this machine, on
    // loopback, `/sync/v1` refuses every machine, and the log says why and
    // how to fix it; the daemon, and this machine's own backups, go on.
    #[cfg(feature = "hub")]
    let mut front_door: Option<Option<tls_adapter::HubListener>> = None;
    // The hub's pairing, which its watch of its machines for offline
    // alerts reads once the daemon listens (decision 168).
    #[cfg(feature = "hub")]
    let mut watched: Option<Arc<app::pairing::HubPairing>> = None;
    #[cfg(feature = "hub")]
    if hub && let Some(data_dir) = platform::dirs::data_dir() {
        // It tells the page's browsers of its machines as they pair, are
        // approved and report (#569).
        let pairing = hub::hub_pairing(&data_dir, Arc::clone(&operation_log.store))
            .with_events(Arc::clone(&state.events));
        // The hub's keys are made now, on its first start, and read after
        // that, so a key file it can't read stops the start here rather than
        // the first machine or token that needs it (decision 120). A start
        // that stops closes the log and lets go of the lock, as a refusal to
        // listen does.
        if let Err(error) = runtime.block_on(hub::keys_at_start(&pairing)) {
            let said = hub::start_refused(localizer, &error);
            runtime.block_on(operation_log.close());
            drop(runtime);
            drop(lock);
            return said;
        }
        let pairing = match &state.sign_in {
            http_adapter::SignIn::On(access) => pairing.with_access(Arc::clone(access)),
            http_adapter::SignIn::Off { .. } => pairing,
        };
        let pairing = Arc::new(pairing);
        watched = Some(Arc::clone(&pairing));
        state.sync = http_adapter::SyncHub::serving(pairing);
        let names = certificate_names(listen, &hosts);
        let served = runtime
            .block_on(hub::hub_tls_key(&data_dir).start(operation_log.store.as_ref()))
            .map_err(|refusal| hub::tls_key_unserved(&refusal))
            .and_then(|served| {
                tls_adapter::HubListener::new(&served.key, &names, domain::Timestamp::now())
                    .map(|listener| (listener, served.fingerprint))
                    .map_err(|error| {
                        format!(
                            "its certificate couldn't be made ({error:?}); restart the hub, and report it if this is said again"
                        )
                    })
            });
        match served {
            Ok((listener, fingerprint)) => {
                log::info!("serving the hub's pairing at {}", api::sync::PATH);
                log::info!("serving the hub's built-in TLS, its certificate {fingerprint}");
                state.sync = state.sync.with_certificate(fingerprint);
                front_door = Some(Some(listener));
            }
            Err(fix) => {
                log::error!(
                    "the hub's built-in TLS isn't served: {fix}. Until then the hub serves only this machine, over plain HTTP on loopback, and refuses every machine at {}; this machine's own backups go on",
                    api::sync::PATH
                );
                state.sync = state.sync.without_tls();
                front_door = Some(None);
            }
        }
    }
    // Open on the network with sign-in off is said on stderr once the
    // address is bound (below), not logged: no log setting hides it.
    #[cfg(feature = "hub")]
    let (loopback_plain_only, built_in_tls) =
        (front_door.is_some(), matches!(front_door, Some(Some(_))));
    #[cfg(not(feature = "hub"))]
    let (loopback_plain_only, built_in_tls) = (false, false);
    if warns_plain_http(exposure, sign_in_on, loopback_plain_only) {
        log::warn!(
            "listening on {listen} beyond loopback over plain HTTP: the password crosses the network unencrypted unless a reverse proxy in front adds TLS"
        );
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
        // A hub takes each connection through its front door first.
        #[cfg(feature = "hub")]
        let listener = match (front_door, &bound) {
            (Some(tls), Ok(local)) => {
                Listening::Door(composition::front_door::open(listener, *local, tls))
            }
            (Some(_), Err(error)) => {
                say(
                    localizer,
                    msg::CLI_CANNOT_LISTEN,
                    &[("address", text(listen)), ("detail", text(error))],
                )?;
                return Ok((Ok(ExitCode::FAILURE), None));
            }
            (None, _) => Listening::Gated(listener),
        };
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
                &[("url", text(listening_url(addr, built_in_tls)))],
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
        #[cfg(feature = "hub")]
        let standing = background.standing();
        let restart = state.restart.clone();
        // A paired machine's reports subscribe before the scheduler starts,
        // so an operation it starts at once is reported (§7.2).
        #[cfg(feature = "hub")]
        let early = app::ports::EventBus::subscribe(state.events.as_ref()).await;
        // It reads which repositories this machine maintains, for their
        // leases (§7.3).
        #[cfg(feature = "hub")]
        let config = background.config();
        #[cfg(feature = "hub")]
        let plans = background.read_plans();
        // And what the daemon's listings showed of other agents' snapshots
        // in them, listing before each session each it maintains whose
        // listing doesn't stand (§7.3, decision 156).
        #[cfg(feature = "hub")]
        let listings = background.listings_due();
        // And the leases maintenance reads, which it takes the hub's grants
        // into, so both judge a lease by one floor (§7.3, I1).
        #[cfg(feature = "hub")]
        let leases = background.leases();
        // A hub watches its machines for offline alerts beside the
        // scheduler (§11, decision 168).
        #[cfg(feature = "hub")]
        let watching = watched.and_then(|pairing| background.watch_machines(pairing));
        let scheduled = tokio::spawn(background.run(random));
        // A paired machine's reports to its hub run beside the scheduler
        // and never hold it up (§7.2).
        #[cfg(feature = "hub")]
        let reporting = platform::dirs::data_dir().map(|data_dir| {
            tokio::spawn(hub::reporting(
                data_dir,
                Arc::clone(&state.events),
                early,
                Arc::clone(&operation_log.store),
                hub::Maintained {
                    config,
                    listings,
                    leases,
                },
                standing,
                Some(plans),
            ))
        });
        let app = http_adapter::router(
            Arc::new(http_adapter::EmbeddedUi),
            http_adapter::RequestChecks::new(hosts),
            state,
        );
        let (cause_tx, mut cause) = tokio::sync::oneshot::channel();
        let shutdown = async move {
            let _ = cause_tx.send(shutdown_signal(interrupt, terminate, restart).await);
        };
        #[cfg(feature = "hub")]
        let serving = match listener {
            Listening::Door(listener) => Box::pin(http_adapter::serve_connections(
                listener,
                http_adapter::handed(app),
                shutdown,
            )) as Serving,
            Listening::Gated(listener) => served_gated(listener, app, shutdown),
        };
        #[cfg(not(feature = "hub"))]
        let serving = served_gated(listener, app, shutdown);
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
        #[cfg(feature = "hub")]
        if let Some(reporting) = &reporting {
            reporting.abort();
        }
        #[cfg(feature = "hub")]
        if let Some(watching) = &watching {
            watching.abort();
        }
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

/// The server's run to its end, whichever listener it serves.
type Serving = std::pin::Pin<Box<dyn Future<Output = io::Result<()>>>>;

/// What the daemon serves on: its listener as it is, or, on a hub serving
/// its built-in TLS, the front door on it.
#[cfg(feature = "hub")]
enum Listening {
    Gated(http_adapter::GatedListener),
    Door(http_adapter::HandedListener),
}

/// `app` served on `listener` until `shutdown`.
fn served_gated(
    listener: http_adapter::GatedListener,
    app: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Serving {
    Box::pin(http_adapter::serve_connections(listener, app, shutdown))
}

/// The names the hub's certificate carries: loopback's, the address it
/// listens on, and every allowed host. A machine whose token pins the
/// certificate checks its fingerprint, not these; a browser checks them.
#[cfg(feature = "hub")]
fn certificate_names(listen: SocketAddr, hosts: &[String]) -> Vec<String> {
    let mut names = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    if !listen.ip().is_unspecified() {
        names.push(listen.ip().to_string());
    }
    // An allowed host is kept as `Host` carries it, an IPv6 address in
    // brackets; the certificate names the address itself.
    names.extend(hosts.iter().map(|host| {
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host)
            .to_owned()
    }));
    names.sort();
    names.dedup();
    names
}

/// Whether serving on `exposure` with sign-in on or off warns that the
/// password crosses the network in plain HTTP. A hub's front door serves
/// plain HTTP only from loopback, with its built-in TLS or without it, so
/// it never does.
fn warns_plain_http(
    exposure: http_adapter::Exposure,
    sign_in_on: bool,
    loopback_plain_only: bool,
) -> bool {
    exposure == http_adapter::Exposure::Network && sign_in_on && !loopback_plain_only
}

/// The address the daemon says it listens at, `bound`: over `https://`
/// when the hub's built-in TLS serves and the address is beyond loopback,
/// where the front door closes plain HTTP. An unspecified address is no
/// address another machine can use, and on this one it reaches loopback.
fn listening_url(bound: SocketAddr, built_in_tls: bool) -> String {
    let ip = bound.ip().to_canonical();
    if built_in_tls && !ip.is_loopback() && !ip.is_unspecified() {
        format!("https://{bound}")
    } else {
        format!("http://{bound}")
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

    /// The certificate names the hub's own machine and every allowed host,
    /// an IPv6 one as an address rather than its bracketed form.
    #[cfg(feature = "hub")]
    #[test]
    fn the_certificate_names_allowed_ipv6_hosts_as_addresses() {
        let listen = "[::]:9898".parse().unwrap();
        let hosts = ["[2001:db8::1]".to_owned(), "hub.example".to_owned()];
        assert_eq!(
            certificate_names(listen, &hosts),
            [
                "127.0.0.1",
                "2001:db8::1",
                "::1",
                "hub.example",
                "localhost"
            ]
        );
    }

    /// The password crossing the network in plain HTTP is warned of
    /// beyond loopback with sign-in on, unless the hub's built-in TLS
    /// serves, which serves plain HTTP only from loopback.
    #[test]
    fn plain_http_is_warned_of_beyond_loopback_unless_the_built_in_tls_serves() {
        use http_adapter::Exposure::{Loopback, Network};
        for (exposure, sign_in_on, built_in_tls, warned) in [
            (Network, true, false, true),
            (Network, true, true, false),
            (Network, false, false, false),
            (Loopback, true, false, false),
        ] {
            assert_eq!(
                warns_plain_http(exposure, sign_in_on, built_in_tls),
                warned,
                "{exposure:?}, sign-in on {sign_in_on}, built-in TLS {built_in_tls}"
            );
        }
    }

    /// A hub serving its built-in TLS beyond loopback says its address over
    /// `https://`, the only way another machine reaches it; on loopback,
    /// where plain HTTP is served too, and without the TLS, over `http://`.
    /// An unspecified address is no address another machine can use, and
    /// on this one it reaches loopback.
    #[test]
    fn a_hub_beyond_loopback_says_its_address_over_tls() {
        for (bound, built_in_tls, url) in [
            ("192.0.2.10:9898", true, "https://192.0.2.10:9898"),
            ("[2001:db8::1]:9898", true, "https://[2001:db8::1]:9898"),
            ("0.0.0.0:9898", true, "http://0.0.0.0:9898"),
            ("[::]:9898", true, "http://[::]:9898"),
            ("127.0.0.1:9898", true, "http://127.0.0.1:9898"),
            ("[::1]:9898", true, "http://[::1]:9898"),
            ("192.0.2.10:9898", false, "http://192.0.2.10:9898"),
        ] {
            let bound: SocketAddr = bound.parse().unwrap();
            assert_eq!(
                listening_url(bound, built_in_tls),
                url,
                "{bound}, {built_in_tls}"
            );
        }
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

    /// `pairing-token --forget-computer` takes a machine's hostname and no
    /// address, and nothing else that asks the hub's terminal for another
    /// thing.
    #[cfg(feature = "hub")]
    #[test]
    fn pairing_token_forgets_a_computer_by_its_machines_hostname_alone() {
        let forget = ["pairing-token", "--forget-computer", "e2e-laptop"];
        assert!(matches!(
            parsed(&forget),
            Ok(Cli {
                command: Command::PairingToken {
                    forget_computer: Some(hostname),
                    ..
                },
                ..
            }) if hostname == "e2e-laptop"
        ));
        for other in [
            ["--list"].as_slice(),
            &["--revoke", "0190f3a0-0000-7000-8000-000000000001"],
            &["--pending"],
            &["--approve", "e2e-desktop"],
            &["--address", "http://127.0.0.1:9"],
            &["--ttl", "10"],
        ] {
            let args: Vec<&str> = forget.iter().chain(other).copied().collect();
            assert_eq!(
                parsed(&args).err(),
                Some(clap::error::ErrorKind::ArgumentConflict),
                "{args:?}"
            );
        }
        assert_eq!(
            parsed(&["pairing-token", "--forget-computer", ""]).err(),
            Some(clap::error::ErrorKind::InvalidValue)
        );
    }
}
