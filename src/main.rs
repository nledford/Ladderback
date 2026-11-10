//! Composition root: the only place that knows every adapter (technical
//! design §3.2). One binary, several roles.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use api::MsgArg;
use clap::{Parser, Subcommand};
use l10n::{Localizer, MessageId, PosixEnv, msg};

mod composition;
mod logging;
mod output;
mod profiles;

#[derive(Parser)]
#[command(
    name = "ladderback",
    version,
    about = "Self-hosted backup manager for rustic"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the agent and serve its web UI.
    Serve {
        /// Loopback address to listen on, until sign-in exists. Port 0 picks
        /// a free port; the address in use is printed on startup.
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
    },
    /// Write a plan as a rustic profile on stdout.
    ExportProfile {
        /// The ID of the plan to export.
        #[arg(long, value_name = "ID")]
        plan: String,
    },
    /// Run one operation, read from stdin, and report events on stdout.
    /// Started by the agent; not for direct use.
    #[command(hide = true)]
    Worker,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Serve {
            listen,
            allowed_hosts,
        } => serve(listen, allowed_hosts),
        Command::ImportProfile { file, repo_name } => profiles::import_profile(file, repo_name),
        Command::ExportProfile { plan } => profiles::export_profile(plan),
        Command::Worker => engine::worker::main(),
    }
}

fn serve(listen: SocketAddr, allowed_hosts: Vec<String>) -> ExitCode {
    logging::install();
    log::info!("ladderback {} starting", env!("CARGO_PKG_VERSION"));
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    // Checked here, not by the argument parser, so that the refusal is said
    // in the catalog's words and the person's language (§8.4). Exit status 2
    // is the parser's own for a value it rejects.
    let mut hosts = Vec::with_capacity(allowed_hosts.len());
    for name in allowed_hosts {
        match http_adapter::validate_host_name(&name) {
            Ok(host) => hosts.push(host),
            Err(refusal) => {
                say(
                    &localizer,
                    msg::CLI_INVALID_ALLOWED_HOST,
                    &[("name", text(refusal.0))],
                );
                return ExitCode::from(2);
            }
        }
    }
    // Sign-in doesn't exist yet and there is no trusted-proxy mode (§10.1).
    if let Err(refusal) = http_adapter::require_loopback(listen) {
        say(
            &localizer,
            msg::CLI_REFUSE_NON_LOOPBACK,
            &[("address", text(refusal.0))],
        );
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                &localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            );
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async {
        // Before the listener: a daemon that cannot serve must not print
        // that it is listening.
        let state = match composition::app_state().await {
            Ok(state) => state,
            Err(refused) => {
                say(&localizer, refused.message, &refused.args);
                return ExitCode::FAILURE;
            }
        };
        let listener = match tokio::net::TcpListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                say(
                    &localizer,
                    msg::CLI_CANNOT_LISTEN,
                    &[("address", text(listen)), ("detail", text(error))],
                );
                return ExitCode::FAILURE;
            }
        };
        match listener.local_addr() {
            Ok(addr) => say(
                &localizer,
                msg::CLI_LISTENING,
                &[("url", text(format!("http://{addr}")))],
            ),
            Err(error) => say(
                &localizer,
                msg::CLI_LISTENING_UNKNOWN,
                &[("detail", text(error))],
            ),
        }
        let app = http_adapter::router(
            Arc::new(http_adapter::EmbeddedUi),
            http_adapter::RequestChecks::new(hosts),
            state,
        );
        match axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                say(
                    &localizer,
                    msg::CLI_SERVER_ERROR,
                    &[("detail", text(error))],
                );
                ExitCode::FAILURE
            }
        }
    })
}

/// Prints one of Ladderback's own messages on stderr after the program's name.
fn say(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) {
    eprintln!("ladderback: {}", localizer.message(id, args));
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
