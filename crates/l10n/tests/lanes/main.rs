//! The localization lanes (technical design §8.4, "Tests"): what the catalogs
//! keep, how codes render, how arguments are formatted, which messages nothing
//! uses, and what the pseudolocales do. One test crate, so the helpers in
//! `support` are built once.

mod catalog;
mod cli;
mod codes;
mod formatting;
mod icu;
mod language;
#[cfg(feature = "pseudo")]
mod pseudo;
#[cfg(not(feature = "pseudo"))]
mod release;
mod support;
mod unused_ids;
