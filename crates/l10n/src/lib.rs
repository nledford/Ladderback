//! Ladderback's localization (technical design §8.4): the Fluent catalogs, the
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
//! Numbers and instants are written as their locale writes them where a host
//! is on. The `icu` feature is the daemon's: ICU4X formats them, and `jiff`
//! gives the offset of an instant in its zone. The `browser-intl` feature is
//! the web UI's, on wasm32: the browser's `Intl` formats them, and no locale
//! data ships. A catalog asks for a style with `NUMBER` and `DATETIME`, whose
//! options are a closed vocabulary. Without a host they are written plainly:
//! Fluent's own number output and UTC. Either way it plugs in behind
//! [`Localizer`] and changes no caller.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod args;
#[cfg(all(feature = "browser-intl", target_arch = "wasm32"))]
mod browser;
pub mod catalog;
pub mod code;
#[cfg(test)]
mod constants;
mod format;
#[cfg(all(
    feature = "icu",
    not(all(feature = "browser-intl", target_arch = "wasm32"))
))]
mod icu;
mod id;
mod locale;
mod localizer;
pub mod msg;
mod pseudo;

/// The zone the viewer's browser reads its own clock in.
pub fn viewer_zone() -> Option<api::IanaZone> {
    todo!()
}

pub use id::MessageId;
pub use locale::{BrowserLanguages, Direction, PosixEnv};
pub use localizer::{Formatted, Localizer};
