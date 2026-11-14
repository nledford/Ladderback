//! The build this program came from (decision NN), as `build.rs` found it.

#[cfg(test)]
mod probe;

/// The build this program came from.
pub(crate) fn running() -> api::BuildDto {
    api::BuildDto {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: None,
        dirty: false,
        release: false,
    }
}
