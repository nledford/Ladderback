//! Secrets written in the configuration itself, where a reference, a file, a
//! command or the keyring would keep them out of it (technical design §10.2).

use super::is_secret_option;
use super::template::{Part, Template};

/// Where in a repository's settings a secret is written as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineSecret {
    /// The location in `repository`: a URL with a password in it, or an
    /// rclone connection string with a credential in it.
    Location,
    /// The location in `repo-hot`, as [`InlineSecret::Location`].
    LocationHot,
    /// The repository's password.
    Password,
    /// An option both halves get, by its name.
    Option(String),
    /// An option of the hot half only.
    OptionHot(String),
    /// An option of the cold half only.
    OptionCold(String),
}

impl InlineSecret {
    /// Where the secret is, as the configuration names it: `repository`,
    /// `repo-hot`, `password`, `options.NAME`, `options-hot.NAME` or
    /// `options-cold.NAME`, with the name as one bounded line. Never the
    /// secret itself (I4).
    pub fn field(&self) -> String {
        match self {
            Self::Location => "repository".to_owned(),
            Self::LocationHot => "repo-hot".to_owned(),
            Self::Password => "password".to_owned(),
            Self::Option(name) => format!("options.{}", shown(name)),
            Self::OptionHot(name) => format!("options-hot.{}", shown(name)),
            Self::OptionCold(name) => format!("options-cold.{}", shown(name)),
        }
    }
}

/// Whether a repository location keeps a credential as text: a password in
/// the userinfo of the URL in it ([`url_password_written`]), or a credential
/// of an rclone connection string. One that is a `${…}` reference, or holds
/// one, is not.
pub fn location_secret_written(location: &str) -> bool {
    url_password_written(location) || rclone_credential_written(location)
}

/// Whether `location` is an rclone location (`rclone:…`). rustic hands it to
/// rclone as an argument, so a `${…}` reference can't keep a credential out
/// of it (§10.2): rclone's own configuration file does.
pub fn rclone_location(location: &str) -> bool {
    location.starts_with(RCLONE)
}

/// The prefix of an rclone location.
const RCLONE: &str = "rclone:";

/// Whether the URL in `text` has a password written as text in its userinfo:
/// what follows the first `:` of the userinfo, not empty and with no `${…}`
/// reference in it. A user name alone is not a password.
///
/// The URL starts after the first `://`. Its authority ends at the first `/`,
/// `\`, `?` or `#`, as the WHATWG URL standard reads a URL of a special
/// scheme, and its userinfo at the last `@` of the authority. A reference is
/// one opaque piece, so a `/`, `:` or `@` inside `${file:/path}` ends nothing.
/// Percent-encoding is not undone: `%70` is text in the file all the same.
/// One pass over the text, with no allocation beyond its pieces.
pub fn url_password_written(text: &str) -> bool {
    let pieces = pieces(text);
    let Some(start) = pieces
        .windows(3)
        .position(|three| three == [Piece::Char(':'), Piece::Char('/'), Piece::Char('/')])
    else {
        return false;
    };
    let rest = pieces.get(start.saturating_add(3)..).unwrap_or_default();
    let end = rest
        .iter()
        .position(|piece| matches!(piece, Piece::Char('/' | '\\' | '?' | '#')))
        .unwrap_or(rest.len());
    let authority = rest.get(..end).unwrap_or_default();
    let Some(at) = authority
        .iter()
        .rposition(|piece| *piece == Piece::Char('@'))
    else {
        return false;
    };
    let userinfo = authority.get(..at).unwrap_or_default();
    let Some(colon) = userinfo.iter().position(|piece| *piece == Piece::Char(':')) else {
        return false;
    };
    let password = userinfo.get(colon.saturating_add(1)..).unwrap_or_default();
    !password.is_empty() && !password.contains(&Piece::Reference)
}

/// Whether an rclone connection string keeps a credential as text: one of
/// its settings (`rclone::s3,secret_access_key=…:bucket`,
/// `rclone:remote,pass=…:path`) whose key names a credential
/// ([`is_secret_option`]) and whose value, quotes aside, is not empty and
/// holds no `${…}` reference. What follows the settings is the path, which
/// holds none.
fn rclone_credential_written(location: &str) -> bool {
    let Some(rest) = location.strip_prefix(RCLONE) else {
        return false;
    };
    // `rclone::sftp,…` names a backend rather than a remote of rclone's
    // configuration file; its settings follow the backend's name all the same.
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let pieces = pieces(rest);
    rclone_settings(&pieces).into_iter().any(|setting| {
        let Some(equals) = setting.iter().position(|piece| *piece == Piece::Char('=')) else {
            return false;
        };
        let key = setting.get(..equals).unwrap_or_default();
        let value = unquoted(setting.get(equals.saturating_add(1)..).unwrap_or_default());
        let name: Option<String> = key
            .iter()
            .map(|piece| match piece {
                Piece::Char(c) => Some(*c),
                Piece::Reference => None,
            })
            .collect();
        name.is_some_and(|name| is_secret_option(&name))
            && !value.is_empty()
            && !value.contains(&Piece::Reference)
    })
}

/// The settings of an rclone remote, as rclone reads them: the pieces between
/// commas up to the `:` that starts the path, the remote's name first. A value
/// may be quoted with `'` or `"`, where a quote written twice stands for
/// itself, so a `,` or `:` inside the quotes ends nothing.
fn rclone_settings(pieces: &[Piece]) -> Vec<&[Piece]> {
    let mut settings = Vec::new();
    let mut start = 0;
    let mut at = 0;
    let mut quote = None;
    while let Some(piece) = pieces.get(at) {
        let next = at.saturating_add(1);
        match (quote, *piece) {
            (Some(open), Piece::Char(c)) if c == open => {
                if pieces.get(next) == Some(&Piece::Char(open)) {
                    at = next;
                } else {
                    quote = None;
                }
            }
            (Some(_), _) => {}
            (None, Piece::Char(c @ ('\'' | '"')))
                if at.checked_sub(1).and_then(|before| pieces.get(before))
                    == Some(&Piece::Char('=')) =>
            {
                quote = Some(c);
            }
            (None, Piece::Char(',')) => {
                settings.push(pieces.get(start..at).unwrap_or_default());
                start = next;
            }
            (None, Piece::Char(':')) => break,
            (None, _) => {}
        }
        at = at.saturating_add(1);
    }
    settings.push(pieces.get(start..at).unwrap_or_default());
    settings
}

/// A setting's value without the quotes around it.
fn unquoted(value: &[Piece]) -> &[Piece] {
    match value {
        [Piece::Char(open @ ('\'' | '"')), inner @ .., last] if *last == Piece::Char(*open) => {
            inner
        }
        _ => value,
    }
}

/// A character of a field as the configuration writes it, or a whole `${…}`
/// reference, which stands for text that is not in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    Char(char),
    Reference,
}

/// `text` as pieces. A text that is not a valid template has no reference:
/// a job refuses it, and until then it is text as written, as
/// [`has_reference`](super::has_reference) reads it. `$${` is the text `${`.
fn pieces(text: &str) -> Vec<Piece> {
    let Ok(template) = Template::parse(text) else {
        return text.chars().map(Piece::Char).collect();
    };
    let mut pieces = Vec::with_capacity(text.len());
    for part in template.parts() {
        match part {
            Part::Text(text) => pieces.extend(text.chars().map(Piece::Char)),
            Part::Reference(_) => pieces.push(Piece::Reference),
        }
    }
    pieces
}

/// The most characters of an option name a warning shows.
const MAX_NAME_CHARS: usize = 80;

/// An option name as one bounded line: a name is what a person typed, so a
/// control character (a line break that would forge a log line, an escape
/// sequence) is `?`, and no more than [`MAX_NAME_CHARS`] characters stay.
fn shown(name: &str) -> String {
    name.chars()
        .take(MAX_NAME_CHARS)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}
