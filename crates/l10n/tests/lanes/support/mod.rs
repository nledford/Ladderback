//! Helpers the lanes share.

pub mod catalog;
pub mod sources;

use std::path::{Path, PathBuf};

use api::{ArgKind, MsgArg};
use l10n::Localizer;
use unic_langid::LanguageIdentifier;

pub fn tag(raw: &str) -> LanguageIdentifier {
    raw.parse()
        .unwrap_or_else(|_| panic!("{raw} is a language tag"))
}

/// A localizer over the embedded catalogs that asks for no language: `en-US`.
pub fn english() -> Localizer {
    Localizer::new(&[])
}

/// The workspace root: two levels above this crate.
pub fn workspace_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(crate_dir)
        .to_path_buf()
}

/// Arguments in the form `Localizer` takes.
pub fn borrowed(args: &[(String, MsgArg)]) -> Vec<(&str, MsgArg)> {
    args.iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect()
}

/// A representative value for an argument of `kind`.
pub fn sample(kind: ArgKind) -> MsgArg {
    match kind {
        ArgKind::Count => MsgArg::Count(3),
        ArgKind::Bytes => MsgArg::Bytes(1536),
        ArgKind::DurationMs => MsgArg::DurationMs(90_000),
        ArgKind::Timestamp => MsgArg::Timestamp {
            utc_ms: 1_794_144_645_000,
            zone: None,
        },
        ArgKind::Text => MsgArg::Text("demo".to_owned()),
    }
}

/// The files of an embedded catalog, as `(name, text)` pairs.
pub fn files(catalog: &l10n::catalog::Catalog) -> Vec<(&'static str, &'static str)> {
    catalog
        .sources
        .iter()
        .map(|source| (source.name, source.text))
        .collect()
}

/// The embedded source catalog, as the rules read it.
pub fn source_locale() -> catalog::Locale {
    let source = l10n::catalog::catalogs()
        .iter()
        .find(|catalog| catalog.tag == l10n::catalog::SOURCE_TAG)
        .unwrap_or_else(|| panic!("the source catalog is embedded"));
    catalog::parse(&files(source)).unwrap_or_else(|problems| panic!("{problems:#?}"))
}

/// A locale from FTL text.
pub fn locale(text: &str) -> catalog::Locale {
    catalog::parse(&[("test.ftl", text)]).unwrap_or_else(|problems| panic!("{problems:#?}"))
}
