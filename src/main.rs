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

use clap::{Parser, Subcommand};

mod logging;

#[derive(Parser)]
#[command(
    name = "steading",
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
        #[arg(
            long = "allowed-host",
            value_name = "NAME",
            value_parser = http_adapter::validate_host_name
        )]
        allowed_hosts: Vec<String>,
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
        Command::Worker => engine::worker::main(),
    }
}

fn serve(listen: SocketAddr, allowed_hosts: Vec<String>) -> ExitCode {
    logging::install();
    log::info!("steading {} starting", env!("CARGO_PKG_VERSION"));
    // Sign-in doesn't exist yet and there is no trusted-proxy mode (§10.1).
    if let Err(error) = http_adapter::require_loopback(listen) {
        eprintln!("steading: {error}");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("steading: cannot start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async {
        let listener = match tokio::net::TcpListener::bind(listen).await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("steading: cannot listen on {listen}: {error}");
                return ExitCode::FAILURE;
            }
        };
        match listener.local_addr() {
            Ok(addr) => eprintln!("steading: listening on http://{addr}"),
            Err(error) => eprintln!("steading: listening (address unknown: {error})"),
        }
        let app = http_adapter::router(
            Arc::new(http_adapter::EmbeddedUi),
            http_adapter::RequestChecks::new(allowed_hosts),
        );
        match axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("steading: server error: {error}");
                ExitCode::FAILURE
            }
        }
    })
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
