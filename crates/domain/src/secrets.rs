//! Secrets context: resolved secret values and the redactor that keeps them
//! out of logs, warnings and errors (invariant I4, technical design §10.2).

use std::collections::BTreeMap;

mod command;
mod options;
mod outcome;
mod source;
mod template;

pub use command::{CommandLine, CommandLineError};
pub use options::is_secret_option;
pub use outcome::{MAX_DETAIL_BYTES, Problem, SecretError, SecretWarning, decode, file_warning};
pub use source::{FileRead, KeyringEntry, NotAKeyringEntry, SecretRef, SourceName, first_line};
pub use template::{Part, Reference, Template, TemplateError};

/// The most bytes a secret may have, and a field that may hold references:
/// what is read from a file or from a command's output, what a reference
/// expands to, and what a template is made of. A file or an output of any
/// size could otherwise size an allocation (technical design §13.5).
pub const MAX_SECRET_BYTES: usize = 64 * 1024;

/// The most `${…}` references one field may hold.
pub const MAX_REFERENCES: usize = 16;

/// A resolved secret, such as a repository password. Its `Debug` form never
/// shows the value, so it can't leak through a log or a test failure.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for the one place that must use it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretValue(***)")
    }
}

/// What a secret is replaced with.
pub const REDACTED: &str = "***";

/// Replaces every known secret value, raw or percent-encoded as it appears
/// in URLs (fully encoded with either hex case, or as the url crate encodes
/// a userinfo), with [`REDACTED`].
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    /// Longest first, so a secret containing another is replaced whole.
    needles: Vec<String>,
}

impl Redactor {
    pub fn new<'a>(secrets: impl IntoIterator<Item = &'a SecretValue>) -> Self {
        let mut needles: Vec<String> = secrets
            .into_iter()
            .map(SecretValue::expose)
            .filter(|value| !value.is_empty())
            .flat_map(|value| {
                [
                    value.to_owned(),
                    percent_encode(value, is_unreserved, Hex::Upper),
                    percent_encode(value, is_unreserved, Hex::Lower),
                    percent_encode(value, is_userinfo_safe, Hex::Upper),
                ]
            })
            .collect();
        needles.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        needles.dedup();
        Self { needles }
    }

    /// A redactor for a repository's password and for the password in the
    /// userinfo of the URL inside its repository string (`rest:https://…`),
    /// which rustic's errors quote.
    pub fn for_repository(
        repository: &str,
        password: &SecretValue,
        _options: &BTreeMap<String, String>,
        _resolved: &[SecretValue],
    ) -> Self {
        let url_passwords = url_passwords(repository);
        Self::new(std::iter::once(password).chain(&url_passwords))
    }

    pub fn redact(&self, text: &str) -> String {
        self.needles.iter().fold(text.to_owned(), |text, needle| {
            text.replace(needle, REDACTED)
        })
    }

    pub fn longest_match(&self) -> usize {
        todo!("the length in bytes of the longest needle")
    }
}

/// Every password the userinfo of the URL inside a repository string may
/// hold, as written and decoded; empty when the string has no URL or the
/// URL has no password.
///
/// The userinfo usually ends at the last `@` before the first `/`, but a raw
/// `/` in the password ends the authority early, so the last `@` of the
/// whole string is tried too. Masking part of a path by mistake is the safe
/// direction.
fn url_passwords(repository: &str) -> Vec<SecretValue> {
    let Some((_, rest)) = repository.split_once("://") else {
        return Vec::new();
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    let mut found: Vec<String> = Vec::new();
    for candidate in [authority, rest] {
        let Some((userinfo, _)) = candidate.rsplit_once('@') else {
            continue;
        };
        let Some((user, password)) = userinfo.split_once(':') else {
            continue;
        };
        // A `/` before the `:` puts the `@` in the path, not in a userinfo.
        if user.contains('/') {
            continue;
        }
        for form in [Some(password.to_owned()), percent_decode(password)]
            .into_iter()
            .flatten()
        {
            if !found.contains(&form) {
                found.push(form);
            }
        }
    }
    found.into_iter().map(SecretValue::new).collect()
}

/// `None` when `text` isn't valid percent-encoding.
fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let (hex, after) = tail.split_at_checked(2)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = after;
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

#[derive(Clone, Copy)]
enum Hex {
    Upper,
    Lower,
}

/// RFC 3986 unreserved characters.
fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~".contains(&b)
}

/// What the url crate (the WHATWG URL standard) leaves unencoded in a
/// userinfo: printable ASCII outside its userinfo percent-encode set.
fn is_userinfo_safe(b: u8) -> bool {
    b.is_ascii_graphic() && !b"\"#<>?`{}/:;=@[\\]^|".contains(&b)
}

/// Percent-encodes every byte that `keep` rejects, with `hex` digits.
fn percent_encode(value: &str, keep: fn(u8) -> bool, hex: Hex) -> String {
    use std::fmt::Write;
    value.bytes().fold(String::new(), |mut out, b| {
        if keep(b) {
            out.push(char::from(b));
        } else {
            let _ = match hex {
                Hex::Upper => write!(out, "%{b:02X}"),
                Hex::Lower => write!(out, "%{b:02x}"),
            };
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor(secrets: &[&str]) -> Redactor {
        let secrets: Vec<SecretValue> = secrets.iter().map(|&s| SecretValue::new(s)).collect();
        Redactor::new(&secrets)
    }

    #[test]
    fn a_secret_never_shows_in_its_debug_form() {
        let secret = SecretValue::new("hunter2");
        let debug = format!("{secret:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(
            debug.contains(REDACTED),
            "a masked value still shows: {debug}"
        );
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn a_redactor_never_shows_what_it_hides_in_its_debug_form() {
        let debug = format!("{:?}", redactor(&["hunter2", "tok-ABC", "a b"]));
        for secret in ["hunter2", "tok-ABC", "tok-abc", "a%20b"] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        assert!(debug.contains("Redactor"), "{debug}");
    }

    #[test]
    fn a_secret_that_overlaps_itself_is_replaced_whole() {
        for (secret, text) in [
            ("abab", "ababab"),
            ("abcabc", "abcabcabc"),
            ("aaaa", "aaaaa"),
        ] {
            assert_eq!(
                redactor(&[secret]).redact(text),
                "***",
                "{secret} in {text}"
            );
        }
        // At character boundaries, with text around it.
        assert_eq!(redactor(&["éé"]).redact("xéééy"), "x***y");
    }

    #[test]
    fn every_occurrence_of_every_secret_is_replaced() {
        let redactor = redactor(&["hunter2", "s3cr3t"]);
        assert_eq!(
            redactor.redact("hunter2 then s3cr3t then hunter2"),
            "*** then *** then ***"
        );
    }

    #[test]
    fn a_secret_inside_a_url_is_replaced_in_its_encoded_form() {
        let redactor = redactor(&["p@ss word/1"]);
        assert_eq!(
            redactor.redact("rest:https://demo:p%40ss%20word%2F1@nas/ and p%40ss%20word%2f1"),
            "rest:https://demo:***@nas/ and ***"
        );
    }

    /// The url crate leaves `!`, `$`, `&` and the like unencoded in a
    /// userinfo, and rustic quotes URLs as the url crate writes them.
    #[test]
    fn a_secret_is_replaced_as_the_url_crate_encodes_a_userinfo() {
        let redactor = redactor(&["P@ssw0rd!"]);
        assert_eq!(
            redactor.redact("URL `rest:https://demo:P%40ssw0rd!@nas:80x/b` parsing failed"),
            "URL `rest:https://demo:***@nas:80x/b` parsing failed"
        );
    }

    #[test]
    fn lowercase_percent_encoding_keeps_the_letters_case() {
        let redactor = redactor(&["Ab/C"]);
        assert_eq!(redactor.redact("x Ab%2fC x"), "x *** x");
    }

    #[test]
    fn a_secret_containing_another_is_replaced_whole() {
        let redactor = redactor(&["abc", "abcdef"]);
        assert_eq!(redactor.redact("xabcdefx abc"), "x***x ***");
    }

    #[test]
    fn secrets_that_overlap_are_replaced_as_one_span() {
        let redactor = redactor(&["abcdefghij", "defghijklm"]);
        assert_eq!(redactor.redact("abcdefghijklm"), "***");
        assert_eq!(redactor.redact("x abcdefghijklm y"), "x *** y");
    }

    #[test]
    fn secrets_that_touch_are_replaced_as_one_span() {
        let redactor = redactor(&["abc", "def"]);
        assert_eq!(redactor.redact("abcdef ghi abc"), "*** ghi ***");
    }

    #[test]
    fn text_that_is_not_ascii_is_cut_only_at_character_boundaries() {
        let redactor = redactor(&["päss", "ssé"]);
        assert_eq!(redactor.redact("é pässéé ü"), "é ***é ü");
    }

    /// Where a secret may land in a URL, and what a URL parser makes of it:
    /// the parser, not the redactor, decides how it is printed, so the
    /// `url` crate is the oracle.
    #[derive(Clone, Copy, Debug)]
    enum Place {
        Userinfo,
        Host,
        Path,
        Query,
        Fragment,
    }

    fn url_with(place: Place, secret: &str) -> String {
        match place {
            Place::Userinfo => format!("http://demo:{secret}@nas.example/b"),
            Place::Host => format!("http://{secret}.example/b"),
            Place::Path => format!("http://nas.example/{secret}/b"),
            Place::Query => format!("http://nas.example/b?k={secret}&z=1"),
            Place::Fragment => format!("http://nas.example/b#{secret}"),
        }
    }

    #[test]
    fn a_secret_shows_nowhere_as_the_url_parser_prints_it() {
        const PIECES: [&str; 14] = [
            "a b:c",
            "q=1&x y",
            "\"q\\b",
            "tab\there",
            "nl\nine",
            "it's",
            "Host",
            "ab+/=",
            "100%",
            "päss",
            "a;b=c|d^e",
            "x[y]z",
            "{x} <y>",
            "back`tick",
        ];
        let mut checked = 0;
        for piece in PIECES {
            let secret = format!("CANARY{piece}ZZEND");
            for place in [
                Place::Userinfo,
                Place::Host,
                Place::Path,
                Place::Query,
                Place::Fragment,
            ] {
                // What the parser refuses, or what changes the URL's shape
                // (a `/` ends a host), is not a case of this kind.
                // A host that isn't ASCII is written in punycode, which needs
                // an IDNA library that the domain doesn't link: not covered.
                if matches!(place, Place::Host) && !secret.is_ascii() {
                    continue;
                }
                let Ok(url) = url::Url::parse(&url_with(place, &secret)) else {
                    continue;
                };
                let kept_shape = match place {
                    Place::Host => url.host_str().is_some_and(|h| h.ends_with(".example")),
                    Place::Userinfo => {
                        url.password().is_some() && url.host_str() == Some("nas.example")
                    }
                    _ => url.host_str() == Some("nas.example"),
                };
                if !kept_shape {
                    continue;
                }
                checked += 1;
                let printed = url.as_str();
                let redacted = redactor(&[&secret]).redact(printed).to_lowercase();
                for marker in ["canary", "zzend"] {
                    assert!(
                        !redacted.contains(marker),
                        "{piece:?} in {place:?} shows as {printed:?}, redacted to {redacted:?}"
                    );
                }
            }
        }
        assert!(checked >= 50, "only {checked} cases ran");
    }

    #[test]
    fn an_empty_secret_redacts_nothing() {
        assert_eq!(redactor(&[""]).redact("text"), "text");
        assert_eq!(redactor(&[]).redact("text"), "text");
    }

    fn passwords(repository: &str) -> Vec<String> {
        url_passwords(repository)
            .iter()
            .map(|secret| secret.expose().to_owned())
            .collect()
    }

    fn for_repository(repository: &str, password: &str) -> Redactor {
        Redactor::for_repository(
            repository,
            &SecretValue::new(password),
            &BTreeMap::new(),
            &[],
        )
    }

    #[test]
    fn the_password_of_a_url_in_a_repository_string_is_found() {
        for repository in [
            "rest:https://demo:s3cr3t@nas:8000/backups",
            "https://demo:s3cr3t@nas/backups",
            "rest:https://demo:s3cr3t@nas",
        ] {
            assert_eq!(passwords(repository), ["s3cr3t"], "{repository}");
        }
    }

    #[test]
    fn a_percent_encoded_url_password_is_found_decoded_and_as_written() {
        assert_eq!(
            passwords("rest:https://demo:p%40ss%2Fw%20rd@nas/b"),
            ["p%40ss%2Fw%20rd", "p@ss/w rd"]
        );
    }

    #[test]
    fn an_unencoded_at_sign_in_the_password_stays_in_the_password() {
        assert_eq!(passwords("rest:https://demo:p@ss@nas/b"), ["p@ss"]);
    }

    #[test]
    fn a_password_that_is_not_valid_percent_encoding_is_taken_as_it_is() {
        assert_eq!(passwords("rest:https://demo:100%@nas/b"), ["100%"]);
        assert_eq!(passwords("rest:https://demo:a%zzb@nas/b"), ["a%zzb"]);
    }

    /// A raw `/` ends the authority, so the URL doesn't parse, and rustic's
    /// error quotes the whole string.
    #[test]
    fn a_raw_slash_in_the_password_does_not_hide_it() {
        let redactor = for_repository("rest:https://demo:ab/cd@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("URL `rest:https://demo:ab/cd@nas/b` parsing failed"),
            "URL `rest:https://demo:***@nas/b` parsing failed"
        );
    }

    #[test]
    fn a_repository_string_without_a_url_password_has_none() {
        for repository in [
            "/backups/home",
            "local:/backups/home",
            "opendal:s3",
            "rclone:remote:path",
            "rest:https://demo@nas/b",
            "rest:https://nas:8000/b",
            // The @ is in the path, not in the authority.
            "rest:https://nas/b/x:y@z",
        ] {
            assert_eq!(passwords(repository), Vec::<String>::new(), "{repository}");
        }
    }

    #[test]
    fn a_repository_redactor_masks_the_password_and_the_url_password_in_any_form() {
        let redactor = for_repository("rest:https://demo:p%40ss@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("open rest:https://demo:p%40ss@nas/b with hunter2: p@ss"),
            "open rest:https://demo:***@nas/b with ***: ***"
        );
    }

    /// rustic quotes a URL as the url crate writes it: a raw `@` in the
    /// password comes back as `%40`.
    #[test]
    fn a_raw_password_is_masked_as_the_url_crate_rewrites_it() {
        let redactor = for_repository("rest:https://demo:P@ssw0rd!@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("join https://demo:P%40ssw0rd!@nas/b/config"),
            "join https://demo:***@nas/b/config"
        );
    }

    #[test]
    fn a_repository_with_an_empty_password_redacts_nothing() {
        let redactor = for_repository("/backups/home", "");
        assert_eq!(redactor.redact("nothing to hide"), "nothing to hide");
    }

    fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn a_repository_redactor_masks_the_value_of_every_option_that_holds_a_credential() {
        let options = options(&[
            ("sas_token", "sv=2024&sig=abc123"),
            ("secret_access_key", "AKIA-demo-secret"),
            ("application_key_id", "0021a7c1"),
            ("bucket", "demo-bucket"),
        ]);
        let redactor = Redactor::for_repository(
            "opendal:azblob",
            &SecretValue::new("hunter2"),
            &options,
            &[],
        );
        assert_eq!(
            redactor.redact(
                "url: http://nas/demo-bucket/config?sv=2024&sig=abc123 key AKIA-demo-secret id 0021a7c1"
            ),
            "url: http://nas/demo-bucket/config?*** key *** id 0021a7c1"
        );
    }

    #[test]
    fn a_repository_redactor_masks_what_the_references_in_its_settings_resolved_to() {
        let resolved = [
            SecretValue::new("canary-from-a-file"),
            SecretValue::new("canary-from-env"),
        ];
        let redactor = Redactor::for_repository(
            "opendal:s3",
            &SecretValue::new("hunter2"),
            &BTreeMap::new(),
            &resolved,
        );
        assert_eq!(
            redactor.redact("GET https://canary-from-env/canary-from-a-file/config failed"),
            "GET https://***/***/config failed"
        );
    }

    /// An endpoint isn't a secret, but a URL may carry credentials in its
    /// userinfo, and OpenDAL's errors quote the URL it built from it.
    #[test]
    fn a_repository_redactor_masks_the_password_of_a_url_an_option_holds() {
        let options = options(&[
            ("endpoint", "https://demo:s3cr3t@minio.example:9000"),
            ("root", "/backups/home"),
        ]);
        let redactor =
            Redactor::for_repository("opendal:s3", &SecretValue::new("hunter2"), &options, &[]);
        assert_eq!(
            redactor.redact("url: https://demo:s3cr3t@minio.example:9000/demo/config failed"),
            "url: https://demo:***@minio.example:9000/demo/config failed"
        );
    }

    /// The control is the third URL: without it, a redactor that looks at no
    /// option would pass this test too.
    #[test]
    fn an_option_url_without_a_password_hides_nothing() {
        let options = options(&[
            ("endpoint", "https://demo@minio.example:9000/data"),
            ("rest-url", "https://minio.example:9000"),
            ("proxy", "https://demo:s3cr3t@proxy.example:3128"),
        ]);
        let redactor =
            Redactor::for_repository("opendal:s3", &SecretValue::new("hunter2"), &options, &[]);
        assert_eq!(
            redactor.redact(
                "https://demo@minio.example:9000/data/demo/config via https://demo:s3cr3t@proxy.example:3128"
            ),
            "https://demo@minio.example:9000/data/demo/config via https://demo:***@proxy.example:3128"
        );
    }

    #[test]
    fn a_redactor_knows_how_long_the_longest_text_it_replaces_is() {
        assert_eq!(redactor(&[]).longest_match(), 0);
        assert_eq!(redactor(&[""]).longest_match(), 0);
        assert_eq!(redactor(&["abc", "abcdef", "xy"]).longest_match(), 6);
        // `p%40ss` is the longest form of `p@ss` that a URL can hold.
        assert_eq!(redactor(&["p@ss"]).longest_match(), 6);
    }

    /// A worker's stderr reaches the daemon a line at a time, so no needle
    /// of a whole key could match any of it.
    #[test]
    fn a_secret_that_spans_lines_is_masked_line_by_line_too() {
        let key = "-----BEGIN KEY-----\r\n  MIIEvQIBADANBgkqhkiG9w0B  \nshort\nAQEFAASCBKcwggSj\n-----END KEY-----\n";
        let redactor = redactor(&[key]);
        for line in [
            "-----BEGIN KEY-----",
            "MIIEvQIBADANBgkqhkiG9w0B",
            "AQEFAASCBKcwggSj",
            "-----END KEY-----",
        ] {
            assert_eq!(
                redactor.redact(&format!("rclone: {line} failed")),
                "rclone: *** failed"
            );
        }
        // The whole text is still replaced as one.
        assert_eq!(redactor.redact(&format!("[{key}]")), "[***]");
        // A line too short to stand for the secret doesn't mask other text.
        assert_eq!(redactor.redact("a short line"), "a short line");
    }

    #[test]
    fn a_line_of_a_secret_is_a_needle_of_its_own_from_eight_bytes_on() {
        let redactor = redactor(&["1234567\n12345678\nabcdefgh"]);
        assert_eq!(redactor.redact("7: 1234567"), "7: 1234567");
        assert_eq!(redactor.redact("8: 12345678"), "8: ***");
        assert_eq!(redactor.redact("9: abcdefgh"), "9: ***");
    }

    #[test]
    fn only_the_first_lines_of_a_very_long_secret_are_needles_of_their_own() {
        let lines: Vec<String> = (0..70).map(|n| format!("line-number-{n:03}")).collect();
        let redactor = redactor(&[lines.join("\n").as_str()]);
        assert_eq!(redactor.redact("line-number-063"), "***");
        assert_eq!(redactor.redact("line-number-064"), "line-number-064");
    }

    /// `${VAR}` gives a value as it is, and a log line doesn't have its
    /// newline.
    #[test]
    fn the_white_space_around_a_secret_is_masked_without_it() {
        let redactor = redactor(&["hunter2\n", "  pad  "]);
        assert_eq!(
            redactor.redact("login hunter2 failed, pad."),
            "login *** failed, ***."
        );
        // The secret with its white space is replaced as a whole.
        assert_eq!(redactor.redact("[hunter2\n]"), "[***]");
        // A secret of nothing but white space has no trimmed form.
        let blank = super::Redactor::new(&[SecretValue::new(" \n ")]);
        assert_eq!(blank.redact("a b"), "a b");
        assert_eq!(blank.redact("x \n y"), "x***y");
    }
}
