//! Secrets context: resolved secret values and the redactor that keeps them
//! out of logs, warnings and errors (invariant I4, technical design §10.2).

/// A resolved secret, such as a repository password. Its `Debug` form never
/// shows the value, so it can't leak through a log or a test failure.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        todo!()
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
        todo!()
    }

    /// A redactor for a repository's password and for the password in the
    /// userinfo of the URL inside its repository string (`rest:https://…`),
    /// which rustic's errors quote.
    pub fn for_repository(repository: &str, password: &SecretValue) -> Self {
        todo!()
    }

    pub fn redact(&self, text: &str) -> String {
        todo!()
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
    todo!()
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
        Redactor::for_repository(repository, &SecretValue::new(password))
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
}
