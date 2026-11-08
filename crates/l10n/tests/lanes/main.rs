//! The localization lanes (technical design §8.4, "Tests"): what the catalogs
//! keep, how codes render, how arguments are formatted, and which messages
//! nothing uses. One test crate, so the helpers in `support` are built once.

mod catalog;
mod cli;
mod codes;
mod formatting;
mod support;
mod unused_ids;
