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

/// The build as people read it (`0.3.0+3f937cf`), which `--version` and the
/// startup log line say.
pub(crate) static SHOWN: LazyLock<String> = LazyLock::new(|| running().display());
