//! The build this program came from (decision 90), as `build.rs` found it.

use std::sync::LazyLock;

#[cfg(test)]
mod probe;

/// The build this program came from.
pub(crate) fn running() -> api::BuildDto {
    let commit = env!("LADDERBACK_BUILT_COMMIT");
    api::BuildDto {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: (!commit.is_empty()).then(|| commit.to_owned()),
        dirty: env!("LADDERBACK_BUILT_DIRTY") == "true",
        release: env!("LADDERBACK_BUILT_RELEASE") == "true",
    }
}

/// The program's name, which `--version`, the startup log line and every
/// snapshot a backup writes put before its build.
pub(crate) const PROGRAM: &str = "ladderback";

/// The build as people read it (`0.3.0+3f937cf`), which `--version` says.
pub(crate) static SHOWN: LazyLock<String> = LazyLock::new(|| running().display());

/// The program and its build as `--version` prints them
/// (`ladderback 0.3.0+3f937cf`), which the startup log line and every snapshot
/// a backup writes name.
pub(crate) static NAMED: LazyLock<String> = LazyLock::new(|| format!("{PROGRAM} {}", *SHOWN));
