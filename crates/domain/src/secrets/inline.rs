//! Secrets written in the configuration itself, where a reference, a file, a
//! command or the keyring would keep them out of it (technical design §10.2).

use std::ops::Range;

use super::template::{Part, Template};
use super::{SecretValue, is_rclone_secret_option};

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
/// One pass over the text, with no allocation beyond its pieces and where
/// they are written.
pub fn url_password_written(text: &str) -> bool {
    let pieces = written_pieces(text).0;
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
/// ([`is_rclone_secret_option`]) and whose value, quotes aside, is not empty
/// and holds no `${…}` reference. What follows the settings is the path,
/// which holds none.
fn rclone_credential_written(location: &str) -> bool {
    let Some(remote) = rclone_remote(location) else {
        return false;
    };
    let pieces = written_pieces(remote).0;
    rclone_settings(&pieces)
        .into_iter()
        .filter_map(|setting| credential(&pieces, setting))
        .any(|value| {
            let value = pieces
                .get(unquoted(&pieces, value))
                .unwrap_or_default();
            !value.is_empty() && !value.contains(&Piece::Reference)
        })
}

/// The most credentials one rclone location may hold. Each is a secret a
/// redactor looks for in every text it masks, so a job refuses a location
/// with more ([`too_many_rclone_credentials`]), and a redactor masks one
/// whole.
pub const MAX_RCLONE_CREDENTIALS: usize = 16;

/// Whether an rclone location holds more than [`MAX_RCLONE_CREDENTIALS`]
/// credentials: settings whose key names one ([`is_rclone_secret_option`])
/// and whose value, quotes aside, is not empty. False for any other
/// location.
pub fn too_many_rclone_credentials(location: &str) -> bool {
    rclone_remote(location).is_some_and(|remote| {
        credential_values(&written_pieces(remote).0).len() > MAX_RCLONE_CREDENTIALS
    })
}

/// The value of each setting of an rclone remote, read as `pieces`, whose
/// key names a credential ([`is_rclone_secret_option`]) and that is not
/// empty, quotes aside.
fn credential_values(pieces: &[Piece]) -> Vec<Range<usize>> {
    rclone_settings(pieces)
        .into_iter()
        .filter_map(|setting| credential(pieces, setting))
        .filter(|value| !unquoted(pieces, value.clone()).is_empty())
        .collect()
}

/// What of an rclone location must not show wherever the location is echoed
/// (I4): the value of each of its settings whose key names a credential
/// ([`is_rclone_secret_option`]) and that is not empty, quotes aside. Each is
/// given as `location` writes it, quotes and `$${` included, and as rclone
/// reads it: with `$${` as `${`, without the quotes around it, and with a
/// quote written twice inside them as one. A value with a `${…}` reference
/// in it is given only as written. A location with more than
/// [`MAX_RCLONE_CREDENTIALS`] of them, which a job refuses, is one secret
/// whole. Empty for any other location.
pub(super) fn rclone_credentials(location: &str) -> Vec<SecretValue> {
    let Some(remote) = rclone_remote(location) else {
        return Vec::new();
    };
    let (pieces, starts) = written_pieces(remote);
    let values = credential_values(&pieces);
    if values.len() > MAX_RCLONE_CREDENTIALS {
        return vec![SecretValue::new(location)];
    }
    let written = |range: &Range<usize>| -> String {
        starts
            .get(range.start)
            .zip(starts.get(range.end))
            .and_then(|(&from, &to)| remote.get(from..to))
            .unwrap_or_default()
            .to_owned()
    };
    let text = |range: &Range<usize>| -> Option<String> {
        pieces
            .get(range.clone())
            .unwrap_or_default()
            .iter()
            .map(|piece| match piece {
                Piece::Char(c) => Some(*c),
                Piece::Reference => None,
            })
            .collect()
    };
    let mut found: Vec<String> = Vec::new();
    for value in values {
        let inner = unquoted(&pieces, value.clone());
        let inner_forms: Vec<String> = std::iter::once(written(&inner))
            .chain(text(&inner))
            .collect();
        let mut forms: Vec<String> = std::iter::once(written(&value))
            .chain(text(&value))
            .collect();
        if let Some(Piece::Char(quote)) = pieces.get(value.start).filter(|_| inner != value) {
            let twice: String = [*quote, *quote].iter().collect();
            let once = quote.to_string();
            forms.extend(inner_forms.iter().map(|form| form.replace(&twice, &once)));
        }
        forms.extend(inner_forms);
        for form in forms {
            if !found.contains(&form) {
                found.push(form);
            }
        }
    }
    found.into_iter().map(SecretValue::new).collect()
}

/// What follows `rclone:` in an rclone location. `rclone::sftp,…` names a
/// backend rather than a remote of rclone's configuration file; its settings
/// follow the backend's name all the same, so the second `:` goes too.
fn rclone_remote(location: &str) -> Option<&str> {
    let rest = location.strip_prefix(RCLONE)?;
    Some(rest.strip_prefix(':').unwrap_or(rest))
}

/// The value of `setting`, the pieces after its first `=`, when its key
/// names a credential in a connection string ([`is_rclone_secret_option`]).
/// A key that holds a reference names nothing in the file.
fn credential(pieces: &[Piece], setting: Range<usize>) -> Option<Range<usize>> {
    let equals = pieces
        .get(setting.clone())?
        .iter()
        .position(|piece| *piece == Piece::Char('='))?;
    let value_start = setting.start.saturating_add(equals);
    let name: String = pieces
        .get(setting.start..value_start)?
        .iter()
        .map(|piece| match piece {
            Piece::Char(c) => Some(*c),
            Piece::Reference => None,
        })
        .collect::<Option<String>>()?;
    is_rclone_secret_option(&name).then(|| value_start.saturating_add(1)..setting.end)
}

/// The settings of an rclone remote, as rclone reads them: the pieces between
/// commas up to the `:` that starts the path, the remote's name first, each
/// as the range of `pieces` it covers. A value may be quoted with `'` or `"`,
/// where a quote written twice stands for itself, so a `,` or `:` inside the
/// quotes ends nothing.
fn rclone_settings(pieces: &[Piece]) -> Vec<Range<usize>> {
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
                settings.push(start..at);
                start = next;
            }
            (None, Piece::Char(':')) => break,
            (None, _) => {}
        }
        at = at.saturating_add(1);
    }
    settings.push(start..at);
    settings
}

/// The range of a setting's value, `value`, without the quotes around it.
fn unquoted(pieces: &[Piece], value: Range<usize>) -> Range<usize> {
    match pieces.get(value.clone()) {
        Some([Piece::Char(open @ ('\'' | '"')), .., last]) if *last == Piece::Char(*open) => {
            value.start.saturating_add(1)..value.end.saturating_sub(1)
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

/// `text` as pieces, and where in `text` each piece is written: the byte
/// offset of each, then `text.len()`. A text that is not a valid template
/// has no reference: a job refuses it, and until then it is text as written,
/// as [`has_reference`](super::has_reference) reads it. `$${` is the text
/// `${`: its `$` is written `$$`.
fn written_pieces(text: &str) -> (Vec<Piece>, Vec<usize>) {
    let Ok(template) = Template::parse(text) else {
        let (pieces, mut starts): (Vec<Piece>, Vec<usize>) = text
            .char_indices()
            .map(|(at, c)| (Piece::Char(c), at))
            .unzip();
        starts.push(text.len());
        return (pieces, starts);
    };
    let mut pieces = Vec::with_capacity(text.len());
    let mut starts = Vec::with_capacity(text.len().saturating_add(1));
    let mut at: usize = 0;
    for part in template.parts() {
        match part {
            Part::Text(literal) => {
                let mut chars = literal.chars().peekable();
                while let Some(c) = chars.next() {
                    pieces.push(Piece::Char(c));
                    starts.push(at);
                    // A `${` in the text is the escape `$${`.
                    let width = if c == '$' && chars.peek() == Some(&'{') {
                        2
                    } else {
                        c.len_utf8()
                    };
                    at = at.saturating_add(width);
                }
            }
            Part::Reference(reference) => {
                pieces.push(Piece::Reference);
                starts.push(at);
                at = at.saturating_add(reference.written_len());
            }
        }
    }
    starts.push(at);
    (pieces, starts)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_piece_is_where_the_text_writes_it() {
        for text in [
            "",
            "plain",
            "é€,x",
            "a$${b}c",
            "$$${A}",
            "$$$${A}",
            "x${VAR}y${file:/run/demo/key}z",
            "${A}$${B}",
            // Not a template: every character is text as written.
            "${",
            "a${}b",
        ] {
            let (pieces, starts) = written_pieces(text);
            assert_eq!(starts.len(), pieces.len() + 1, "{text:?}");
            assert_eq!(starts.last(), Some(&text.len()), "{text:?}");
            for (at, piece) in pieces.iter().enumerate() {
                let written = &text[starts[at]..starts[at + 1]];
                let expected = match piece {
                    Piece::Char('$') if written == "$$" => true,
                    Piece::Char(c) => written == c.to_string(),
                    Piece::Reference => written.starts_with("${") && written.ends_with('}'),
                };
                assert!(expected, "{text:?}: {piece:?} is written {written:?}");
            }
        }
    }

    #[test]
    fn an_rclone_location_with_more_than_16_credentials_holds_too_many() {
        let credentials = |count: usize| -> String {
            (0..count)
                .map(|n| {
                    if n % 2 == 0 {
                        format!(",pass=demo-{n}")
                    } else {
                        format!(",key=demo-{n}")
                    }
                })
                .collect()
        };
        // A quoted `,` ends no setting; another key, an empty value and the
        // path after the settings hold no credential.
        let others = ",pass='demo,pass=x',host=nas,pass=,pass='',pass=\"\"";
        let location = |count: usize| {
            format!(
                "rclone::sftp{}{others}:/pass=x,key=y",
                credentials(count)
            )
        };
        assert!(!too_many_rclone_credentials(&location(15)));
        assert!(too_many_rclone_credentials(&location(16)));
        assert!(too_many_rclone_credentials(&format!(
            "rclone:nas{}:/backups",
            credentials(17)
        )));
        for location in [
            format!("opendal:sftp{}", credentials(17)),
            format!("/srv/demo{}", credentials(17)),
            "rclone:".to_owned(),
        ] {
            assert!(!too_many_rclone_credentials(&location), "{location}");
        }
    }

    /// rclone opens a quote only where a value starts, so a quote further on
    /// hides no setting from the count.
    #[test]
    fn a_quote_inside_a_value_hides_no_credential_from_the_count() {
        let settings: String = (1..=20).map(|n| format!(",pass={n}")).collect();
        let location = format!("rclone::sftp,pass=a='b{settings},x=':/backups");
        assert!(too_many_rclone_credentials(&location));
    }

    /// A location `depth` deep: each level but the last is a crypt remote
    /// whose `remote` is the next level, quoted.
    fn nested(depth: usize) -> String {
        let innermost = ":s3,secret_access_key=demo-a,session_token=demo-b:b".to_owned();
        (1..depth).fold(innermost, |inner, _| {
            format!(":crypt,remote='{}':p", inner.replace('\'', "''"))
        })
    }

    /// The credentials of a location written in a setting count with the
    /// location's own, four levels deep; one deeper counts as one, whole.
    #[test]
    fn the_credentials_of_a_nested_location_count_toward_the_cap() {
        let location = |own: usize, depth: usize| {
            let settings: String = (0..own).map(|n| format!(",pass=demo-{n}")).collect();
            format!(
                "rclone::crypt{settings},remote='{}':p",
                nested(depth - 1).replace('\'', "''")
            )
        };
        // Two credentials at the fourth level.
        assert!(!too_many_rclone_credentials(&location(14, 4)));
        assert!(too_many_rclone_credentials(&location(15, 4)));
        // The fifth level is one credential.
        assert!(!too_many_rclone_credentials(&location(15, 5)));
        assert!(too_many_rclone_credentials(&location(16, 5)));
    }
}
