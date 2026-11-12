//! Properties of [`Redactor`] (invariant I4): for any secrets and any text
//! that holds them, in any form the redactor masks, the masked text shows
//! none of them, nor does a second pass over it; masking a masked text
//! changes nothing when no secret holds the mask character; and a text that
//! holds no secret comes back as it was.
//!
//! How each part of a URL writes a secret comes from the `url` crate, as in
//! the redactor's own tests: the parser, not the redactor, decides how a
//! secret is printed.

use proptest::prelude::*;
use proptest::sample::{Index, select};
use url::Url;

use super::{MIN_LINE_BYTES, MIN_PIECE_BYTES, Redactor, SecretValue};

/// The characters of a secret: letters in both cases, digits, white space,
/// what a URL parser splits a URL at or encodes, and some that aren't ASCII.
/// Not `*`, the character of the mask, which [`secret`] adds where it may.
const SECRET_CHARS: &[char] = &[
    'a', 'b', 'z', 'A', 'B', 'Z', '0', '7', '-', '.', '_', '~', ' ', '\t', '\n', '\r', '@', '#',
    '?', '/', ':', '\\', '%', '"', '<', '>', '`', '{', '}', '^', '|', '[', ']', ';', '=', '&', '+',
    '\'', '!', '$', '(', ')', ',', 'é', 'ü', '日',
];

/// The characters at which a URL parser takes a secret apart.
const SPLITTING: [char; 5] = ['@', '#', '?', '/', ':'];

/// A secret: single characters, and runs of letters and digits long enough
/// to be a piece or a line of their own; with `*`, the mask's character,
/// among them when `masks` is set.
fn secret(masks: bool) -> impl Strategy<Value = String> {
    let chars: Vec<char> = SECRET_CHARS
        .iter()
        .copied()
        .chain(masks.then_some('*'))
        .collect();
    let chunk = prop_oneof![
        3 => select(chars).prop_map(String::from),
        1 => "[A-Za-z0-9]{8,10}",
    ];
    prop::collection::vec(chunk, 1..=8).prop_map(|chunks| chunks.concat())
}

/// What of a secret a URL parser writes.
#[derive(Clone, Copy, Debug)]
enum Source {
    Whole,
    /// Without tab and newlines, which a parser takes out of a URL.
    WithoutBreaks,
    /// Without them, and with `\` read as `/`, as in a special URL's path.
    Slashed,
    /// A piece of 8 bytes or more between the characters a parser splits a
    /// URL at, which a library that takes a URL apart may print alone.
    Piece(Index),
}

/// How a part of a URL writes a text.
#[derive(Clone, Copy, Debug)]
enum Encoding {
    AsIs,
    /// Every byte but the unreserved ones percent-encoded.
    Percent {
        upper: bool,
    },
    Userinfo,
    Path {
        special: bool,
    },
    Query {
        special: bool,
    },
    Fragment,
}

/// A form in which a secret shows in text.
#[derive(Clone, Copy, Debug)]
enum Form {
    /// As a URL parser writes it, and lower-cased, as a host is.
    Url {
        source: Source,
        encoding: Encoding,
        lower: bool,
    },
    /// With the white space around it taken off.
    Trimmed,
    /// One line of 8 bytes or more of a secret that spans lines.
    Line(Index),
}

fn form() -> impl Strategy<Value = Form> {
    let source = prop_oneof![
        Just(Source::Whole),
        Just(Source::WithoutBreaks),
        Just(Source::Slashed),
        any::<Index>().prop_map(Source::Piece),
    ];
    let encoding = prop_oneof![
        Just(Encoding::AsIs),
        any::<bool>().prop_map(|upper| Encoding::Percent { upper }),
        Just(Encoding::Userinfo),
        any::<bool>().prop_map(|special| Encoding::Path { special }),
        any::<bool>().prop_map(|special| Encoding::Query { special }),
        Just(Encoding::Fragment),
    ];
    prop_oneof![
        4 => (source, encoding, any::<bool>()).prop_map(|(source, encoding, lower)| Form::Url {
            source,
            encoding,
            lower,
        }),
        1 => Just(Form::Trimmed),
        1 => any::<Index>().prop_map(Form::Line),
    ]
}

fn without_breaks(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

impl Form {
    /// How `secret` shows in this form, or `None` when it has no such form:
    /// no piece or line long enough, or a path a parser would rewrite.
    fn of(self, secret: &str) -> Option<String> {
        let shown = match self {
            Self::Trimmed => secret.trim().to_owned(),
            Self::Line(index) => {
                let lines: Vec<&str> = secret
                    .trim()
                    .lines()
                    .map(str::trim)
                    .filter(|line| line.len() >= MIN_LINE_BYTES)
                    .collect();
                if lines.is_empty() {
                    return None;
                }
                (*index.get(&lines)).to_owned()
            }
            Self::Url {
                source,
                encoding,
                lower,
            } => {
                let written = encoding.write(&source.of(secret)?)?;
                if lower {
                    written.to_ascii_lowercase()
                } else {
                    written
                }
            }
        };
        Some(shown).filter(|shown| !shown.is_empty())
    }
}

impl Source {
    fn of(self, secret: &str) -> Option<String> {
        let slashed = without_breaks(secret).replace('\\', "/");
        match self {
            Self::Whole => Some(secret.to_owned()),
            Self::WithoutBreaks => Some(without_breaks(secret)),
            Self::Slashed => Some(slashed),
            Self::Piece(index) => {
                let pieces: Vec<&str> = slashed
                    .split(SPLITTING)
                    .filter(|piece| piece.len() >= MIN_PIECE_BYTES)
                    .collect();
                if pieces.is_empty() {
                    return None;
                }
                Some((*index.get(&pieces)).to_owned())
            }
        }
    }
}

impl Encoding {
    fn write(self, text: &str) -> Option<String> {
        let url = |special: bool| {
            Url::parse(if special {
                "http://demo@nas.example/"
            } else {
                "demo://demo@nas.example/"
            })
            .expect("a URL")
        };
        match self {
            Self::AsIs => Some(text.to_owned()),
            Self::Percent { upper } => Some(fully_encoded(text, upper)),
            Self::Userinfo => {
                let mut url = url(true);
                url.set_password(Some(text)).expect("a URL with a host");
                url.password().map(str::to_owned)
            }
            // A parser resolves the dot segments of a path, which the
            // redactor doesn't follow (see `url_needles`).
            Self::Path { special } if has_dot_segment(text, special) => None,
            Self::Path { special } => {
                let mut url = url(special);
                url.set_path(&format!("/{text}"));
                url.path().strip_prefix('/').map(str::to_owned)
            }
            Self::Query { special } => {
                let mut url = url(special);
                url.set_query(Some(text));
                url.query().map(str::to_owned)
            }
            Self::Fragment => {
                let mut url = url(true);
                url.set_fragment(Some(text));
                url.fragment().map(str::to_owned)
            }
        }
    }
}

/// `text` with every byte but the unreserved ones (RFC 3986) written as
/// `%` and two hex digits.
fn fully_encoded(text: &str, upper: bool) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else if upper {
                format!("%{byte:02X}")
            } else {
                format!("%{byte:02x}")
            }
        })
        .collect()
}

fn has_dot_segment(text: &str, special: bool) -> bool {
    without_breaks(text)
        .to_ascii_lowercase()
        .replace("%2e", ".")
        .split(|c| c == '/' || (special && c == '\\'))
        .any(|segment| segment == "." || segment == "..")
}

/// A stretch of a text: ordinary characters, the mask's among them, or one
/// of the secrets in one of its forms.
#[derive(Clone, Debug)]
enum Stretch {
    Plain(String),
    Secret(Index, Form),
}

fn stretch() -> impl Strategy<Value = Stretch> {
    let plain = prop::collection::vec(prop_oneof![select(SECRET_CHARS), Just('*')], 0..=6)
        .prop_map(String::from_iter);
    prop_oneof![
        plain.prop_map(Stretch::Plain),
        (any::<Index>(), form()).prop_map(|(secret, form)| Stretch::Secret(secret, form)),
    ]
}

/// Secrets, and a text that holds them in some of their forms.
#[derive(Debug)]
struct Case {
    secrets: Vec<String>,
    text: String,
    /// Each secret as the text holds it, in the form it was put in.
    shown: Vec<String>,
}

fn case(masks: bool) -> impl Strategy<Value = Case> {
    (
        prop::collection::vec(secret(masks), 1..=3),
        prop::collection::vec(stretch(), 1..=6),
    )
        .prop_map(|(secrets, stretches)| {
            let mut text = String::new();
            let mut shown = Vec::new();
            for stretch in stretches {
                match stretch {
                    Stretch::Plain(plain) => text.push_str(&plain),
                    Stretch::Secret(index, form) => {
                        let secret = index.get(&secrets);
                        // A form the secret doesn't have: it shows as it is.
                        let form = form.of(secret).unwrap_or_else(|| secret.clone());
                        text.push_str(&form);
                        shown.push(form);
                    }
                }
            }
            Case {
                secrets,
                text,
                shown,
            }
        })
}

/// Whether `secret` shows in `masked` other than through a mask: an
/// occurrence that has a `*` of a run of three or more, which a mask is part
/// of, could be that mask and the text beside it, so it doesn't count.
fn shows(masked: &str, secret: &str) -> bool {
    let bytes = masked.as_bytes();
    let mut in_mask = vec![false; bytes.len()];
    let mut start = 0;
    for at in 0..=bytes.len() {
        if bytes.get(at) != Some(&b'*') {
            if at.saturating_sub(start) >= 3
                && let Some(run) = in_mask.get_mut(start..at)
            {
                run.fill(true);
            }
            start = at.saturating_add(1);
        }
    }
    (0..bytes.len()).any(|at| {
        masked
            .get(at..)
            .is_some_and(|rest| rest.starts_with(secret))
            && in_mask
                .get(at..at.saturating_add(secret.len()))
                .is_some_and(|span| !span.contains(&true))
    })
}

fn redactor(secrets: &[String]) -> Redactor {
    let secrets: Vec<SecretValue> = secrets.iter().cloned().map(SecretValue::new).collect();
    Redactor::new(&secrets)
}

/// Secrets made of some of the characters, and a text made of others: none
/// a secret has, in either case, nor what an encoding of one or `\` read as
/// `/` writes (`%`, `/`), so no form of a secret can be in the text.
fn secrets_and_a_text_apart() -> impl Strategy<Value = (Vec<String>, String)> {
    prop::collection::vec(any::<bool>(), SECRET_CHARS.len())
        .prop_filter("a secret needs a character", |ours| ours.contains(&true))
        .prop_flat_map(|ours| {
            let secret_chars: Vec<char> = SECRET_CHARS
                .iter()
                .zip(&ours)
                .filter(|&(_, &ours)| ours)
                .map(|(&c, _)| c)
                .collect();
            let text_chars: Vec<char> = SECRET_CHARS
                .iter()
                .copied()
                .chain(['*'])
                .filter(|&c| {
                    c != '%'
                        && c != '/'
                        && !secret_chars
                            .iter()
                            .any(|s| *s == c || s.to_ascii_lowercase() == c)
                })
                .collect();
            let secret =
                prop::collection::vec(select(secret_chars), 1..=12).prop_map(String::from_iter);
            (
                prop::collection::vec(secret, 1..=3),
                prop::collection::vec(select(text_chars), 0..=40).prop_map(String::from_iter),
            )
        })
}

proptest! {
    #![proptest_config(crate::properties::config())]

    #[test]
    fn no_secret_shows_in_a_masked_text_in_any_form_the_redactor_masks(case in case(true)) {
        let redactor = redactor(&case.secrets);
        let once = redactor.redact(&case.text);
        // A second pass may mask more when a secret holds `*` (#287), but
        // never reveals one.
        let twice = redactor.redact(&once);
        for masked in [&once, &twice] {
            for secret in case.shown.iter().chain(&case.secrets) {
                prop_assert!(
                    !shows(masked, secret),
                    "{secret:?} shows in {masked:?}, masked from {:?}",
                    case.text
                );
            }
        }
    }

    /// Only for secrets without `*`: one with it can make up a mask and the
    /// text beside it, which a second pass masks too (#287).
    #[test]
    fn masking_a_masked_text_changes_nothing(case in case(false)) {
        let redactor = redactor(&case.secrets);
        let once = redactor.redact(&case.text);
        prop_assert_eq!(redactor.redact(&once), once);
    }

    #[test]
    fn a_text_that_holds_no_secret_comes_back_as_it_was(
        (secrets, text) in secrets_and_a_text_apart()
    ) {
        prop_assert_eq!(redactor(&secrets).redact(&text), text);
    }
}
