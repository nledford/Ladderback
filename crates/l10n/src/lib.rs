//! Steading's localization (technical design §8.4): the Fluent catalogs, the
//! generated message IDs, locale selection and the rendering of the API's
//! error codes. A presentation library: it depends on [`api`] for the codes
//! and arguments, and `domain` and `app` never use it.
//!
//! - [`msg`] has one constant per message of the `en-US` catalog in
//!   `locales/en-US/`. Every call site names its message through one, so a
//!   typo doesn't compile.
//! - [`PosixEnv`] reads the locales a command-line program is asked to speak,
//!   and [`BrowserLanguages`] the ones a web page is asked for.
//! - [`Localizer`] formats messages, and renders the codes of the API
//!   ([`Localizer::error`], [`Localizer::warning`]), in a chain of locales that
//!   always ends in `en-US`. It also tells a page which `lang` and which
//!   [`Direction`] to declare.
//!
//! A development build, with the `pseudo` feature, can also speak the
//! pseudolocales `en-XA` and `ar-XB`: the `en-US` catalog, transformed, to show
//! what is written in the code instead. A release build leaves them out.
//!
//! Numbers and dates are written plainly for now: Fluent's own number output
//! and UTC. Locale-aware formatting plugs in behind [`Localizer`] and changes
//! no caller.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod args;
pub mod catalog;
pub mod code;
#[cfg(test)]
mod constants;
mod format;
mod id;
mod locale;
mod localizer;
pub mod msg;
mod pseudo;

pub use id::MessageId;
pub use locale::{BrowserLanguages, Direction, PosixEnv};
pub use localizer::{Formatted, Localizer};
