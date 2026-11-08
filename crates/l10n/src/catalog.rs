//! The catalogs compiled into the binary. `build.rs` embeds every `.ftl`
//! file under `locales/<tag>/`; nothing is loaded at run time, and a locale
//! tag never reaches a file path.

/// One embedded `.ftl` file.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    /// The file's name, such as `common.ftl`.
    pub name: &'static str,
    pub text: &'static str,
}

/// All the files of one locale.
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    /// The locale's tag, the name of its directory: `en-US`.
    pub tag: &'static str,
    pub sources: &'static [Source],
}

/// The tag of the source locale: the one every message ID comes from and
/// every other locale falls back to.
pub const SOURCE_TAG: &str = "en-US";

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

/// Every embedded catalog, the source locale included.
pub fn catalogs() -> &'static [Catalog] {
    CATALOGS
}
