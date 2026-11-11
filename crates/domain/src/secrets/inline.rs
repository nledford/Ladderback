//! Secrets written in the configuration itself, where a reference, a file, a
//! command or the keyring would keep them out of it (technical design §10.2).

use super::is_rclone_secret_option;
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
/// ([`is_rclone_secret_option`]) and whose value, quotes aside, is not empty and
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
        name.is_some_and(|name| is_rclone_secret_option(&name))
            && !value.is_empty()
            && !value.contains(&Piece::Reference)
    })
}

/// The most credentials one rclone location may hold. Each is a secret a
/// redactor looks for in every text it masks, so a job refuses a location
/// with more ([`too_many_rclone_credentials`]), and a redactor masks one
/// whole.
pub const MAX_RCLONE_CREDENTIALS: usize = 16;

/// Whether an rclone location holds more than [`MAX_RCLONE_CREDENTIALS`]
/// credentials: settings whose key names one and whose value, quotes aside,
/// is not empty. False for any other location.
pub fn too_many_rclone_credentials(_location: &str) -> bool {
    todo!()
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

/// `text` as pieces, and where in `text` each piece is written: the byte
/// offset of each, then `text.len()`.
fn written_pieces(text: &str) -> (Vec<Piece>, Vec<usize>) {
    let _ = text;
    todo!()
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
