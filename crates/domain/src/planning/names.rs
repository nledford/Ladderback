//! The small named values of a configuration: a host name, a path to back up,
//! a locale.

use super::{InvalidValue, ValueKind};

/// The most bytes a host name takes: the longest a DNS name can be.
const MAX_HOSTNAME_BYTES: usize = 255;
/// The most bytes a path takes: `PATH_MAX` on Linux, and macOS's too.
const MAX_SOURCE_BYTES: usize = 4096;
/// The most bytes a BCP 47 language tag takes in practice (RFC 5646 §4.6).
const MAX_LOCALE_BYTES: usize = 35;
/// The most bytes one subtag of a language tag takes.
const MAX_SUBTAG_BYTES: usize = 8;

/// The name a machine goes by in its snapshots and on the hub (§7.1): the
/// agent's own, or a plan's override of it. One to 255 bytes with no control
/// character; macOS names such as `Nathan's MacBook Pro` have spaces and
/// apostrophes, so nothing else is refused.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hostname(String);

impl Hostname {
    pub fn parse(_text: &str) -> Result<Self, InvalidValue> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A folder or file to back up. The path is absolute: a relative one would
/// mean what the daemon's working directory says on the day, and the daemon
/// has none to speak of. At most 4096 bytes and without a NUL.
///
/// It is the path as written, not checked against the disk: a source that
/// doesn't exist yet is a warning when the plan runs, not a refusal here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Source(String);

impl Source {
    pub fn parse(_text: &str) -> Result<Self, InvalidValue> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The language of the agent's notifications (§8.4), such as `en-US`. Only
/// the shape of a BCP 47 tag is checked: up to 35 bytes in subtags of one to
/// eight ASCII letters or digits, separated by single hyphens, the first one
/// letters only. Whether a translation exists for it is for the localizer,
/// which falls back to `en-US`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locale(String);

impl Locale {
    pub fn parse(_text: &str) -> Result<Self, InvalidValue> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(kind: ValueKind, text: &str) -> InvalidValue {
        InvalidValue::new(kind, text)
    }

    #[test]
    fn a_hostname_is_one_to_255_bytes_without_control_characters() {
        let widest = format!("{}a", "é".repeat(127));
        assert_eq!(widest.len(), 255);
        for text in [
            "a",
            "demo-mbp-2021",
            "Demo's MacBook Pro",
            "Büro.local",
            &"a".repeat(255),
            &widest,
        ] {
            assert_eq!(Hostname::parse(text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn anything_else_is_not_a_hostname() {
        // 128 two-byte letters are 128 characters but 256 bytes.
        let too_wide = "é".repeat(128);
        for text in [
            "",
            &"a".repeat(256),
            &too_wide,
            "two\nlines",
            "tab\there",
            "nul\0byte",
            "\u{7f}",
        ] {
            assert_eq!(
                Hostname::parse(text),
                Err(refused(ValueKind::Hostname, text))
            );
        }
    }

    #[test]
    fn a_source_is_an_absolute_path_of_at_most_4096_bytes() {
        let longest = format!("/{}", "a".repeat(4095));
        assert_eq!(longest.len(), 4096);
        for text in [
            "/",
            "/Users/demo",
            "/home/demo/My Documents",
            "/srv/données",
            &longest,
        ] {
            assert_eq!(Source::parse(text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn a_relative_or_oversized_path_is_not_a_source() {
        let too_long = format!("/{}", "a".repeat(4096));
        for text in [
            "",
            "relative/dir",
            "./here",
            "~/docs",
            "C:\\Users\\demo",
            "/with\0nul",
            &too_long,
        ] {
            assert_eq!(Source::parse(text), Err(refused(ValueKind::Source, text)));
        }
    }

    #[test]
    fn a_locale_is_the_shape_of_a_bcp_47_tag() {
        // 18 one-letter subtags and 17 hyphens: 35 bytes, the longest.
        let longest = ('a'..='r').map(String::from).collect::<Vec<_>>().join("-");
        assert_eq!(longest.len(), 35);
        for text in [
            "en",
            "en-US",
            "zh-Hant-TW",
            "de-CH-1996",
            "x-private",
            "aaaaaaaa-bbbbbbbb",
            &longest,
        ] {
            assert_eq!(Locale::parse(text).unwrap().as_str(), text);
        }
    }

    #[test]
    fn anything_else_is_not_a_locale() {
        // 17 one-letter subtags and a two-letter one: 36 bytes, one too many.
        let letters = ('a'..='q').map(String::from).collect::<Vec<_>>();
        let one_byte_over = format!("{}-xy", letters.join("-"));
        assert_eq!(one_byte_over.len(), 36);
        for text in [
            "",
            "en_US",
            "en US",
            "-en",
            "en-",
            "en--US",
            "en-ninechars",
            "e1",
            "1n",
            "ñ",
            "en-ü",
            &one_byte_over,
        ] {
            assert_eq!(Locale::parse(text), Err(refused(ValueKind::Locale, text)));
        }
    }
}
