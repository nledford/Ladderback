//! The localizer: formats catalog messages in a chain of locales and renders
//! the API's codes (technical design §8.4).

use std::fmt;

use api::{ErrorCode, MsgArg, WarningCode};
use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

use crate::MessageId;
use crate::catalog::Catalog;

type Bundle = FluentBundle<FluentResource>;

/// A message as shown, and what went wrong while making it.
///
/// Rendering never fails: a message the catalogs lack is shown as its ID, an
/// argument nobody supplied as its name. Each such slip is one entry in
/// [`problems`](Formatted::problems), so a test, or a log line, can tell a
/// clean rendering from a degraded one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatted {
    text: String,
    problems: Vec<String>,
}

impl Formatted {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn into_text(self) -> String {
        self.text
    }
}

impl fmt::Display for Formatted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Formats messages in a chain of locales that always ends in `en-US`. It
/// holds parsed catalogs only, takes no lock and does no I/O, so one is
/// shared freely between threads.
pub struct Localizer {
    locales: Vec<LanguageIdentifier>,
    bundles: Vec<Bundle>,
    isolate: bool,
}

impl fmt::Debug for Localizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Localizer")
            .field("locales", &self.locales)
            .finish_non_exhaustive()
    }
}

impl Localizer {
    /// A localizer for the `preferences`, best first, over the embedded
    /// catalogs. Locales without a catalog are left out, and `en-US` is added
    /// last, so every message has an answer.
    pub fn new(preferences: &[LanguageIdentifier]) -> Self {
        Self::from_catalogs(preferences, crate::catalog::catalogs())
    }

    /// The same over `catalogs`, which tests use to bring a catalog of their
    /// own. Without an `en-US` catalog among them nothing ends the chain, and
    /// a message no locale has is shown as its ID.
    pub fn from_catalogs(preferences: &[LanguageIdentifier], catalogs: &[Catalog]) -> Self {
        let _ = (preferences, catalogs);
        todo!()
    }

    /// Whether a value put into a message is wrapped in Unicode isolation
    /// marks (U+2068, U+2069), as Fluent does by default. Off by default: a
    /// terminal, and a log line, would show the marks as stray characters.
    /// An interface that mixes right-to-left text into a message turns it on.
    pub fn with_isolation(mut self, isolate: bool) -> Self {
        self.isolate = isolate;
        self
    }

    /// The locales messages are looked up in, best first; ends with `en-US`.
    pub fn locales(&self) -> &[LanguageIdentifier] {
        &self.locales
    }

    /// The message `id`, with `args` for its variables.
    pub fn message(&self, id: MessageId, args: &[(&str, MsgArg)]) -> Formatted {
        let _ = (id, args);
        todo!()
    }

    /// The text of an error code. A `detail` is text the sender can't
    /// translate, such as an operating system's message: it goes where the
    /// message puts `{ $detail }`, and the message's `.with-detail` form is
    /// used when there is one.
    pub fn error(
        &self,
        code: ErrorCode,
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
    ) -> Formatted {
        let _ = (code, args, detail);
        todo!()
    }

    /// The text of a warning code; see [`Localizer::error`].
    pub fn warning(
        &self,
        code: WarningCode,
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
    ) -> Formatted {
        let _ = (code, args, detail);
        todo!()
    }
}
