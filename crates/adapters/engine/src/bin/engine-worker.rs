//! The worker as a binary of its own, for this crate's integration tests:
//! Cargo only sets `CARGO_BIN_EXE_<name>` for binaries of the package under
//! test, and the `ladderback` binary belongs to the root package. It does what
//! `ladderback worker` does.

use std::process::ExitCode;

fn main() -> ExitCode {
    engine::worker::main()
}
