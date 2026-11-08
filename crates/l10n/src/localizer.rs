//! The localizer: formats catalog messages in a chain of locales and renders
//! the API's codes (technical design §8.4).

use std::borrow::Cow;
use std::fmt;

use api::{ErrorCode, MsgArg, WarningCode};
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::args::{MAX_ARGS, clamp_text, duration_parts, size_parts};
use crate::catalog::Catalog;
use crate::locale::negotiate;
use crate::{MessageId, code, format, msg};

type Bundle = FluentBundle<FluentResource>;

/// The attribute of a code's message that shows the sender's detail.
const WITH_DETAIL: &str = "with-detail";

/// What an optional argument the sender left out is bound to, so that a
/// message can choose its wording on it (`[none]`).
const ABSENT: &str = "none";

/// The largest count a Fluent number holds exactly: it is an `f64`, which
/// counts every integer up to 2^53.
const MAX_EXACT_COUNT: u64 = 1 << 53;

/// Unicode's first-strong and pop isolates, which Fluent puts around values.
const ISOLATION_MARKS: [char; 2] = ['\u{2068}', '\u{2069}'];

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
/// holds parsed catalogs only and does no I/O, so one is shared freely
/// between threads.
///
/// A message is looked up locale by locale, best first, so a translation may
/// leave messages out. The helper messages `size` and `duration`, which
/// arrange the parts of a [`MsgArg::Bytes`] or [`MsgArg::DurationMs`], are
/// looked up from the locale that formats the message onward.
pub struct Localizer {
    locales: Vec<LanguageIdentifier>,
    bundles: Vec<Bundle>,
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
        let known: Vec<(LanguageIdentifier, &Catalog)> = catalogs
            .iter()
            .filter_map(|catalog| Some((catalog.tag.parse().ok()?, catalog)))
            .collect();
        let tags: Vec<LanguageIdentifier> = known.iter().map(|(tag, _)| tag.clone()).collect();
        let mut locales = Vec::new();
        let mut bundles = Vec::new();
        for locale in negotiate(preferences, &tags) {
            if let Some((_, catalog)) = known.iter().find(|(tag, _)| *tag == locale) {
                bundles.push(new_bundle(&locale, catalog));
                locales.push(locale);
            }
        }
        Self { locales, bundles }
    }

    /// Whether a value put into a message is wrapped in Unicode isolation
    /// marks (U+2068, U+2069), as Fluent does by default. Off by default: a
    /// terminal, and a log line, would show the marks as stray characters.
    /// An interface that mixes right-to-left text into a message turns it on.
    pub fn with_isolation(mut self, isolate: bool) -> Self {
        for bundle in &mut self.bundles {
            bundle.set_use_isolating(isolate);
        }
        self
    }

    /// The locales messages are looked up in, best first; ends with `en-US`.
    pub fn locales(&self) -> &[LanguageIdentifier] {
        &self.locales
    }

    /// The message `id`, with `args` for its variables. At most 32 arguments
    /// are read, and a text argument is cut at 16 KiB.
    pub fn message(&self, id: MessageId, args: &[(&str, MsgArg)]) -> Formatted {
        self.render(id, &[], args, None)
    }

    /// The text of an error code. A `detail` is text the sender can't
    /// translate, such as an operating system's message: it goes where the
    /// message puts `{ $detail }`, and the message's `.with-detail` form is
    /// used when there is one. A detail wins over an argument named `detail`,
    /// and a blank detail is no detail. An argument the code declares
    /// optional that the sender left out is the text `none`.
    pub fn error(
        &self,
        code: ErrorCode,
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
    ) -> Formatted {
        self.render(code::error_message(code), code.args(), args, detail)
    }

    /// The text of a warning code; see [`Localizer::error`].
    pub fn warning(
        &self,
        code: WarningCode,
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
    ) -> Formatted {
        self.render(code::warning_message(code), code.args(), args, detail)
    }

    fn render(
        &self,
        id: MessageId,
        declared: &[api::ArgSpec],
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
    ) -> Formatted {
        let mut problems = Vec::new();
        // A blank detail is no detail: it would leave a dangling colon.
        let detail = detail.filter(|text| !text.trim().is_empty());
        let with_detail = detail
            .and_then(|_| self.locate(id.as_str(), Some(WITH_DETAIL)))
            .map(|index| (index, Some(WITH_DETAIL)));
        let found =
            with_detail.or_else(|| self.locate(id.as_str(), None).map(|index| (index, None)));
        let text = match found {
            Some((index, attribute)) => {
                // Only the first arguments are read. An optional argument the
                // sender left out is the text `none`, so a message can choose.
                let read = args.get(..MAX_ARGS).unwrap_or(args);
                let mut all = read.to_vec();
                all.extend(
                    declared
                        .iter()
                        .filter(|spec| {
                            spec.optional && !read.iter().any(|(name, _)| *name == spec.name)
                        })
                        .map(|spec| (spec.name, MsgArg::Text(ABSENT.to_owned()))),
                );
                self.format(index, id.as_str(), attribute, &all, detail, &mut problems)
            }
            None => {
                problems.push(format!("message not found: {}", id.as_str()));
                id.as_str().to_owned()
            }
        };
        Formatted { text, problems }
    }

    /// The first locale that has `id`: its attribute, or its value.
    fn locate(&self, id: &str, attribute: Option<&str>) -> Option<usize> {
        self.bundles.iter().position(|bundle| {
            bundle
                .get_message(id)
                .is_some_and(|message| match attribute {
                    Some(name) => message.get_attribute(name).is_some(),
                    None => message.value().is_some(),
                })
        })
    }

    fn format(
        &self,
        index: usize,
        id: &str,
        attribute: Option<&str>,
        args: &[(&str, MsgArg)],
        detail: Option<&str>,
        problems: &mut Vec<String>,
    ) -> String {
        let Some(bundle) = self.bundles.get(index) else {
            return id.to_owned();
        };
        let Some(message) = bundle.get_message(id) else {
            return id.to_owned();
        };
        let pattern = match attribute {
            Some(name) => message
                .get_attribute(name)
                .map(|attribute| attribute.value()),
            None => message.value(),
        };
        let Some(pattern) = pattern else {
            return id.to_owned();
        };
        let mut fluent_args = FluentArgs::new();
        for (name, arg) in args {
            fluent_args.set(*name, self.value(index, arg, problems));
        }
        if let Some(detail) = detail {
            fluent_args.set("detail", FluentValue::String(clamp_text(detail)));
        }
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, Some(&fluent_args), &mut errors)
            .into_owned();
        problems.extend(errors.iter().map(ToString::to_string));
        text
    }

    /// A message argument as a Fluent value. A number stays a number, so
    /// plural selectors work; sizes, durations and instants are written by
    /// the catalog's helper messages and the host's formatters, then passed
    /// on as text; text is never taken for a number. A count past
    /// [`MAX_EXACT_COUNT`] is text too, so its digits are exact, and it
    /// selects the `other` plural form.
    fn value<'a>(
        &self,
        from: usize,
        arg: &'a MsgArg,
        problems: &mut Vec<String>,
    ) -> FluentValue<'a> {
        match arg {
            MsgArg::Count(count) if *count <= MAX_EXACT_COUNT => FluentValue::from(*count),
            MsgArg::Count(count) => FluentValue::String(Cow::Owned(count.to_string())),
            MsgArg::Text(text) => FluentValue::String(clamp_text(text)),
            MsgArg::Bytes(bytes) => {
                let (value, unit) = size_parts(*bytes);
                let parts = [
                    ("value", FluentValue::from(value)),
                    ("unit", FluentValue::String(Cow::Borrowed(unit.key()))),
                ];
                FluentValue::String(Cow::Owned(self.helper(from, msg::SIZE, &parts, problems)))
            }
            MsgArg::DurationMs(milliseconds) => {
                let duration = duration_parts(*milliseconds);
                let mut parts = vec![("form", FluentValue::String(Cow::Borrowed(duration.form())))];
                parts.extend(
                    duration
                        .parts()
                        .into_iter()
                        .map(|(name, part)| (name, FluentValue::from(part))),
                );
                FluentValue::String(Cow::Owned(self.helper(
                    from,
                    msg::DURATION,
                    &parts,
                    problems,
                )))
            }
            MsgArg::Timestamp { utc_ms, .. } => {
                FluentValue::String(Cow::Owned(format::instant(*utc_ms)))
            }
        }
    }

    /// Formats the helper message `id` in the first locale from `from` on
    /// that has it. Its isolation marks are dropped: the placeable it is
    /// passed to isolates the whole text once.
    fn helper(
        &self,
        from: usize,
        id: MessageId,
        args: &[(&str, FluentValue<'_>)],
        problems: &mut Vec<String>,
    ) -> String {
        for bundle in self.bundles.iter().skip(from) {
            let Some(pattern) = bundle
                .get_message(id.as_str())
                .and_then(|message| message.value())
            else {
                continue;
            };
            let mut fluent_args = FluentArgs::with_capacity(args.len());
            for (name, value) in args {
                fluent_args.set(*name, value.clone());
            }
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
            problems.extend(errors.iter().map(ToString::to_string));
            return text.replace(ISOLATION_MARKS, "");
        }
        problems.push(format!("message not found: {}", id.as_str()));
        id.as_str().to_owned()
    }
}

fn new_bundle(locale: &LanguageIdentifier, catalog: &Catalog) -> Bundle {
    let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);
    bundle.set_use_isolating(false);
    // `NUMBER` keeps a count numeric, so plural selectors work. An error only
    // means it was already added.
    let _ = bundle.add_builtins();
    for source in catalog.sources {
        // The catalog tests keep every file clean. A bundle in the field
        // takes what parses, and the first of an ID defined twice.
        let resource = FluentResource::try_new(source.text.to_owned())
            .unwrap_or_else(|(resource, _)| resource);
        let _ = bundle.add_resource(resource);
    }
    bundle
}
