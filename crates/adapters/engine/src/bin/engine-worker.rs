//! The worker as a binary of its own, for this crate's integration tests:
//! Cargo only sets `CARGO_BIN_EXE_<name>` for binaries of the package under
//! test, and the `ladderback` binary belongs to the root package. It does what
//! `ladderback worker` does, and its snapshots name it, not a build of
//! Ladderback: the build is found by the root package's build script.

use std::process::ExitCode;

fn main() -> ExitCode {
    engine::worker::main(concat!("engine-worker ", env!("CARGO_PKG_VERSION")))
}
