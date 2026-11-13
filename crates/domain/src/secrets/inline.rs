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
/// its credentials ([`Walk::of`]: `rclone::s3,secret_access_key=…:bucket`,
/// `rclone:remote,pass=…:path`, or one of a location written in a setting)
/// that holds no `${…}` reference.
fn rclone_credential_written(location: &str) -> bool {
    let Some(remote) = rclone_remote(location) else {
        return false;
    };
    let walk = Walk::of(written_pieces(remote).0);
    walk.found.iter().any(|found| {
        walk.levels.get(found.level).is_some_and(|level| {
            !level
                .pieces
                .get(unquoted(&level.pieces, found.value.clone()))
                .unwrap_or_default()
                .contains(&Piece::Reference)
        })
    })
}

/// The most credentials one rclone location may hold. Each is a secret a
/// redactor looks for in every text it masks, so a job refuses a location
/// with more ([`too_many_rclone_credentials`]), and a redactor masks one
/// whole.
pub const MAX_RCLONE_CREDENTIALS: usize = 16;

/// Whether an rclone location holds more than [`MAX_RCLONE_CREDENTIALS`]
/// credentials, as [`Walk::of`] finds them, at every level. False for any
/// other location.
pub fn too_many_rclone_credentials(location: &str) -> bool {
    rclone_remote(location).is_some_and(|remote| {
        Walk::of(written_pieces(remote).0).found.len() > MAX_RCLONE_CREDENTIALS
    })
}

/// What of an rclone location must not show wherever the location is echoed
/// (I4): each of its credentials ([`Walk::of`]) in every form a text may
/// show it in ([`Walk::forms`]). A location with more than
/// [`MAX_RCLONE_CREDENTIALS`] of them, which a job refuses, is one secret
/// whole. Empty for any other location.
pub(super) fn rclone_credentials(location: &str) -> Vec<SecretValue> {
    let Some(remote) = rclone_remote(location) else {
        return Vec::new();
    };
    let (pieces, starts) = written_pieces(remote);
    let walk = Walk::of(pieces);
    if walk.found.len() > MAX_RCLONE_CREDENTIALS {
        return vec![SecretValue::new(location)];
    }
    let mut found: Vec<String> = Vec::new();
    for credential in &walk.found {
        for form in walk.forms(credential, remote, &starts) {
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
    Some(rest.trim_prefix(':'))
}

/// How many levels deep an rclone location is read for credentials, and for
/// what it names on this machine (`planning::local_paths::rclone`): the
/// location itself is the first, and a location written in the value of
/// one of its settings (crypt's `remote=':s3,…:bucket'`) the next.
pub(crate) const MAX_RCLONE_DEPTH: usize = 4;

/// One location of an rclone location: the location itself, or one written
/// in the value of a setting of another.
struct Level {
    /// The location as rclone reads it, its remote's or backend's name first.
    pieces: Vec<Piece>,
    /// For each of `pieces`, the first and the last piece of the location
    /// itself it is read from (a quote written twice is read as one);
    /// `None` for the location itself.
    at: Option<Vec<(usize, usize)>>,
    /// The level in a setting of which this one is written.
    parent: Option<usize>,
}

impl Level {
    /// The first and the last piece of the location itself that piece
    /// `index` is read from.
    fn span(&self, index: usize) -> Option<(usize, usize)> {
        match &self.at {
            None => Some((index, index)),
            Some(at) => at.get(index).copied(),
        }
    }

    /// The pieces read from pieces `first..=last` of the location itself.
    fn within(&self, first: usize, last: usize) -> &[Piece] {
        let range = match &self.at {
            None => first..last.saturating_add(1),
            Some(at) => {
                at.partition_point(|&(from, _)| from < first)
                    ..at.partition_point(|&(_, to)| to <= last)
            }
        };
        self.pieces.get(range).unwrap_or_default()
    }
}

/// A credential: its value, quotes included, as a range of the pieces of
/// the level it is in.
struct Found {
    level: usize,
    value: Range<usize>,
}

/// The credentials of an rclone location, and the levels they are in.
struct Walk {
    levels: Vec<Level>,
    found: Vec<Found>,
}

impl Walk {
    /// The credentials of an rclone location, `pieces` what follows
    /// `rclone:` ([`rclone_remote`]): the value of each setting whose key
    /// names a credential ([`is_rclone_secret_option`]) and that is not
    /// empty, quotes aside. The value of any other setting that is a location
    /// of its own ([`nested_location`]) is read the same way, to
    /// [`MAX_RCLONE_DEPTH`] levels; one deeper is a credential whole.
    fn of(pieces: Vec<Piece>) -> Self {
        let mut walk = Self {
            levels: vec![Level {
                pieces,
                at: None,
                parent: None,
            }],
            found: Vec::new(),
        };
        walk.read(0, 1);
        walk
    }

    /// Reads level `index`, `depth` levels deep.
    fn read(&mut self, index: usize, depth: usize) {
        let Some(level) = self.levels.get(index) else {
            return;
        };
        let mut nested = Vec::new();
        for setting in rclone_settings(&level.pieces).0 {
            let Some((secret, value)) = setting_value(&level.pieces, setting) else {
                continue;
            };
            if unquoted(&level.pieces, value.clone()).is_empty() {
                continue;
            }
            if secret {
                self.found.push(Found {
                    level: index,
                    value,
                });
            } else if let Some(location) = nested_location(level, index, value.clone()) {
                if depth < MAX_RCLONE_DEPTH {
                    nested.push(location);
                } else {
                    self.found.push(Found {
                        level: index,
                        value,
                    });
                }
            }
        }
        for location in nested {
            self.levels.push(location);
            self.read(self.levels.len().saturating_sub(1), depth.saturating_add(1));
        }
    }

    /// Each form of `found` a text may show: its value, and its value without
    /// the quotes around it, as `remote` writes them (`starts`: where each
    /// piece is written, [`written_pieces`]) and as each level reads them,
    /// from the location itself to the level it is in; and without its
    /// quotes with a quote written twice as one, as rclone reads it last. A
    /// form with a `${…}` reference in it is given only as written.
    fn forms(&self, found: &Found, remote: &str, starts: &[usize]) -> Vec<String> {
        let Some(level) = self.levels.get(found.level) else {
            return Vec::new();
        };
        let inner = unquoted(&level.pieces, found.value.clone());
        let quote = match level.pieces.get(found.value.start) {
            Some(Piece::Char(quote)) if inner != found.value => Some(*quote),
            _ => None,
        };
        let mut forms = Vec::new();
        for (range, quote) in [(found.value.clone(), None), (inner, quote)] {
            let Some((first, last)) = level
                .span(range.start)
                .zip(range.end.checked_sub(1).and_then(|last| level.span(last)))
            else {
                continue;
            };
            let (first, last) = (first.0, last.1);
            let mut region: Vec<String> = starts
                .get(first)
                .zip(starts.get(last.saturating_add(1)))
                .and_then(|(&from, &to)| remote.get(from..to))
                .map(str::to_owned)
                .into_iter()
                .collect();
            let mut at = Some(found.level);
            while let Some(reader) = at.and_then(|index| self.levels.get(index)) {
                region.extend(text(reader.within(first, last)));
                at = reader.parent;
            }
            if let Some(quote) = quote {
                let twice: String = [quote, quote].iter().collect();
                let once = quote.to_string();
                let read: Vec<String> = region
                    .iter()
                    .map(|form| form.replace(&twice, &once))
                    .collect();
                forms.extend(read);
            }
            forms.extend(region);
        }
        forms
    }
}

/// `pieces` as text, `None` when a `${…}` reference is among them.
fn text(pieces: &[Piece]) -> Option<String> {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Char(c) => Some(*c),
            Piece::Reference => None,
        })
        .collect()
}

/// The location written in `value`, a setting's value at `level` (whose
/// index is `index`), as rclone reads it: without the quotes around the
/// value, with a quote written twice as one, and without the `:` that starts
/// a backend's name. `None` when the value is not quoted, which leaves no
/// room for a `,` or a `:`, or when it is no location to rclone: one that
/// names no backend and has no `:` outside quotes is a local path.
fn nested_location(level: &Level, index: usize, value: Range<usize>) -> Option<Level> {
    let inner = unquoted(&level.pieces, value.clone());
    let &quote = level.pieces.get(value.start).filter(|_| inner != value)?;
    let backend = level.pieces.get(inner.start) == Some(&Piece::Char(':'));
    let mut pieces = Vec::with_capacity(inner.len());
    let mut at = Vec::with_capacity(inner.len());
    let mut next = inner.start.saturating_add(usize::from(backend));
    while next < inner.end {
        let piece = *level.pieces.get(next)?;
        pieces.push(piece);
        let twice = piece == quote && level.pieces.get(next.saturating_add(1)) == Some(&quote);
        let last = next.saturating_add(usize::from(twice));
        at.push((level.span(next)?.0, level.span(last)?.1));
        next = last.saturating_add(1);
    }
    let location = backend || rclone_settings(&pieces).1;
    location.then_some(Level {
        pieces,
        at: Some(at),
        parent: Some(index),
    })
}

/// The value of `setting`, the pieces after its first `=`, and whether its
/// key names a credential in a connection string
/// ([`is_rclone_secret_option`]). A key that holds a reference names nothing
/// in the file. `None` for a setting with no `=`.
fn setting_value(pieces: &[Piece], setting: Range<usize>) -> Option<(bool, Range<usize>)> {
    let equals = pieces
        .get(setting.clone())?
        .iter()
        .position(|piece| *piece == Piece::Char('='))?;
    let value_start = setting.start.saturating_add(equals);
    let secret = text(pieces.get(setting.start..value_start)?)
        .is_some_and(|name| is_rclone_secret_option(&name));
    Some((secret, value_start.saturating_add(1)..setting.end))
}

/// The settings of an rclone remote, as rclone reads them: the pieces between
/// commas up to the `:` that starts the path, the remote's name first, each
/// as the range of `pieces` it covers; and whether such a `:` ends them. A
/// value that starts with `'` or `"` is quoted, where a quote written twice
/// stands for itself, so a `,` or `:` inside the quotes ends nothing; a
/// quote anywhere else is text.
fn rclone_settings(pieces: &[Piece]) -> (Vec<Range<usize>>, bool) {
    let mut settings = Vec::new();
    let mut start = 0;
    let mut at = 0;
    // Where the value of the setting read starts, once its first `=` is.
    let mut value_start = None;
    let mut quote = None;
    let mut path = false;
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
            (None, Piece::Char(c @ ('\'' | '"'))) if value_start == Some(at) => {
                quote = Some(c);
            }
            (None, Piece::Char('=')) if value_start.is_none() => value_start = Some(next),
            (None, Piece::Char(',')) => {
                settings.push(start..at);
                start = next;
                value_start = None;
            }
            (None, Piece::Char(':')) => {
                path = true;
                break;
            }
            (None, _) => {}
        }
        at = at.saturating_add(1);
    }
    settings.push(start..at);
    (settings, path)
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
        let location =
            |count: usize| format!("rclone::sftp{}{others}:/pass=x,key=y", credentials(count));
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
