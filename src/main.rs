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
use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use api::MsgArg;
use app::ports::Random;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::random::OsRandom;

mod build_info;
mod composition;
mod logging;
mod output;
mod profiles;
mod service;
mod set_password;

#[derive(Parser)]
#[command(
    name = "ladderback",
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
    /// Run the agent and serve its web UI.
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
    // Their descriptions are the catalog's (`cli-help-*`), set in `parse`.
    InstallService,
    RemoveService,
    /// Run one operation, read from stdin, and report events on stdout.
    /// Started by the agent; not for direct use.
    #[command(hide = true)]
    Worker,
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
/// reader's language: those of the service commands (#299 moves the rest).
fn with_catalog_help(command: clap::Command) -> clap::Command {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    command.mut_subcommands(|subcommand| {
        let id = match subcommand.get_name() {
            "install-service" => msg::CLI_HELP_INSTALL_SERVICE,
            "remove-service" => msg::CLI_HELP_REMOVE_SERVICE,
            _ => return subcommand,
        };
        let about = localizer.message(id, &[]).text().to_owned();
        subcommand.about(about)
    })
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
        } => serve(listen, allowed_hosts, plain),
        Command::ImportProfile {
            file,
            repo_name,
            json,
        } => profiles::import_profile(file, repo_name, output::Report::asked(json)),
        Command::ExportProfile { plan } => profiles::export_profile(plan),
        Command::SetPassword => set_password::set_password(),
        Command::InstallService => service::install_service(),
        Command::RemoveService => service::remove_service(),
        Command::Worker => engine::worker::main(),
    }
}

fn serve(listen: SocketAddr, allowed_hosts: Vec<String>, plain: bool) -> ExitCode {
    logging::install(plain);
    log::info!("ladderback {} starting", *build_info::SHOWN);
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, run_daemon(&localizer, listen, allowed_hosts))
}

/// Starts the daemon and runs it until it is told to stop, and says how it
/// ended. What it says on stderr before it serves is written through `say`:
/// a startup line that nobody reads stops it, as anything that prints does.
/// Once it serves it only logs, and the logger drops what it cannot write.
fn run_daemon(
    localizer: &Localizer,
    listen: SocketAddr,
    allowed_hosts: Vec<String>,
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
    // The one source of randomness, for sign-in's tokens and the scheduler's
    // jitter alike (§3.2).
    let random: Arc<dyn Random> = Arc::new(OsRandom);
    let sign_in = match composition::sign_in(Arc::clone(&random)) {
        Ok(sign_in) => sign_in,
        Err(refused) => {
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let sign_in_on = matches!(sign_in, http_adapter::SignIn::On(_));
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
    } = match runtime.block_on(composition::daemon(sign_in, exposure)) {
        Ok(daemon) => daemon,
        Err(refused) => {
            say(localizer, refused.message, &refused.args)?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let code: io::Result<ExitCode> = runtime.block_on(async {
        let listener = match http_adapter::GatedListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                say(
                    localizer,
                    msg::CLI_CANNOT_LISTEN,
                    &[("address", text(listen)), ("detail", text(error))],
                )?;
                return Ok(ExitCode::FAILURE);
            }
        };
        let bound = listener.local_addr();
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
        let scheduled = tokio::spawn(background.run(random));
        let app = http_adapter::router(
            Arc::new(http_adapter::EmbeddedUi),
            http_adapter::RequestChecks::new(hosts),
            state,
        );
        let served = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<http_adapter::PeerAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await;
        // No run starts once the daemon stops serving; one already started
        // ends as the runtime drops it, and the next start recovers it.
        scheduled.abort();
        match served {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(error) => {
                say(localizer, msg::CLI_SERVER_ERROR, &[("detail", text(error))])?;
                Ok(ExitCode::FAILURE)
            }
        }
    });
    // The operation log closes once the server has stopped, inside the
    // runtime, so a write in flight ends as it would have rather than being
    // cut off, and nothing of the log outlives the daemon. Whatever came of
    // serving, a refusal to listen included (§9).
    runtime.block_on(operation_log.close());
    // The lock is let go last: until the runtime is gone, a job it was running
    // may still hold the operation log, and another daemon must not recover
    // that log from under it (§9).
    drop(runtime);
    drop(lock);
    code
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

/// Resolves on Ctrl-C or SIGTERM (what systemd and launchd send).
async fn shutdown_signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        () = terminate => {}
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
}
