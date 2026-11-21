//! An agent's hostname as pairing registers it (§16, decision 120,
//! "Hostnames"): what the agent is called on the hub, and the one variable
//! field of the signed `Pair` message.

use std::fmt;

/// The most bytes of UTF-8 a hostname has.
pub const MAX_HOSTNAME_BYTES: usize = 255;

/// 1 to 255 bytes of UTF-8 with no control, format, bidirectional-control
/// or other invisible character, and no space at either end. Not limited to
/// ASCII: machines are named "Nick's MacBook Pro" or in other scripts. How
/// two names compare is the registry's rule.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Hostname(String);

/// Why text isn't a hostname.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidHostname {
    Empty,
    TooLong,
    /// A control or format character, the bidirectional controls among
    /// them, another character that shows as nothing, or a space at either
    /// end, any of which can make a name show as another.
    Hidden(char),
}

impl Hostname {
    pub fn parse(text: &str) -> Result<Self, InvalidHostname> {
        if text.is_empty() {
            return Err(InvalidHostname::Empty);
        }
        if text.len() > MAX_HOSTNAME_BYTES {
            return Err(InvalidHostname::TooLong);
        }
        if let Some(hidden) = text
            .chars()
            .find(|&c| c.is_control() || is_format(c) || is_invisible(c))
        {
            return Err(InvalidHostname::Hidden(hidden));
        }
        let ends = [text.chars().next(), text.chars().next_back()];
        if let Some(space) = ends.into_iter().flatten().find(|c| c.is_whitespace()) {
            return Err(InvalidHostname::Hidden(space));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hostname({:?})", self.0)
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Unicode's format characters (general category Cf), which include the
/// bidirectional controls: the table of Unicode 17.0, the same as 16.0's.
/// The standard library has no table of them, and `unicode-normalization`,
/// which the hub's name comparison will use (§16, decision 120,
/// "Hostnames"), has none either.
const FORMAT: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

fn is_format(c: char) -> bool {
    in_ranges(FORMAT, c)
}

/// Characters outside Cf that show as nothing, or as a break, in a name
/// (#1065): Unicode 17.0's assigned default-ignorable characters that
/// aren't format characters (the combining grapheme joiner, the Hangul
/// fillers, the Khmer inherent vowels and the variation selectors), and the
/// line and paragraph separators.
const INVISIBLE: &[(char, char)] = &[
    ('\u{034F}', '\u{034F}'),
    ('\u{115F}', '\u{1160}'),
    ('\u{17B4}', '\u{17B5}'),
    ('\u{180B}', '\u{180D}'),
    ('\u{180F}', '\u{180F}'),
    ('\u{2028}', '\u{2029}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{FE00}', '\u{FE0F}'),
    ('\u{FFA0}', '\u{FFA0}'),
    ('\u{E0100}', '\u{E01EF}'),
];

fn is_invisible(c: char) -> bool {
    in_ranges(INVISIBLE, c)
}

fn in_ranges(ranges: &[(char, char)], c: char) -> bool {
    ranges
        .iter()
        .any(|&(first, last)| (first..=last).contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_in_any_script_is_kept_as_typed() {
        for name in ["laptop", "Nick's MacBook Pro", "ノートパソコン", "café"] {
            assert_eq!(Hostname::parse(name).unwrap().as_str(), name);
        }
    }

    /// Debug names the type around the quoted name; Display is the name alone.
    #[test]
    fn a_name_shows_as_typed() {
        let name = Hostname::parse("Nick's NAS").unwrap();
        assert_eq!(format!("{name:?}"), r#"Hostname("Nick's NAS")"#);
        assert_eq!(name.to_string(), "Nick's NAS");
    }

    #[test]
    fn an_empty_name_is_refused() {
        assert_eq!(Hostname::parse(""), Err(InvalidHostname::Empty));
    }

    /// 255 bytes is the most; a character of several bytes counts each.
    #[test]
    fn a_name_over_255_bytes_is_refused() {
        assert!(Hostname::parse(&"a".repeat(255)).is_ok());
        assert_eq!(
            Hostname::parse(&"a".repeat(256)),
            Err(InvalidHostname::TooLong)
        );
        assert_eq!(
            Hostname::parse(&"é".repeat(128)),
            Err(InvalidHostname::TooLong)
        );
    }

    #[test]
    fn control_format_and_bidirectional_characters_are_refused() {
        for hidden in [
            '\n',
            '\u{7f}',
            '\u{85}',
            '\u{ad}',
            '\u{200b}',
            '\u{200e}',
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{feff}',
            '\u{e0041}',
        ] {
            assert_eq!(
                Hostname::parse(&format!("lap{hidden}top")),
                Err(InvalidHostname::Hidden(hidden)),
                "U+{:04X}",
                u32::from(hidden)
            );
        }
    }

    /// Characters that show as nothing though they aren't format
    /// characters, the assigned default-ignorable ones (the Hangul fillers,
    /// the variation selectors, the combining grapheme joiner and the
    /// Khmer inherent vowels), and the line and paragraph separators, are
    /// refused anywhere in a name.
    #[test]
    fn fillers_variation_selectors_and_separators_are_refused() {
        for hidden in [
            '\u{34f}',
            '\u{115f}',
            '\u{1160}',
            '\u{3164}',
            '\u{ffa0}',
            '\u{17b4}',
            '\u{17b5}',
            '\u{180b}',
            '\u{180f}',
            '\u{fe00}',
            '\u{fe0f}',
            '\u{e0100}',
            '\u{e01ef}',
            '\u{2028}',
            '\u{2029}',
        ] {
            assert_eq!(
                Hostname::parse(&format!("lap{hidden}top")),
                Err(InvalidHostname::Hidden(hidden)),
                "U+{:04X}",
                u32::from(hidden)
            );
        }
    }

    /// A name that begins or ends with a space of any kind is refused,
    /// naming the space; spaces inside a name are kept.
    #[test]
    fn a_space_at_either_end_is_refused() {
        for space in [' ', '\u{a0}', '\u{3000}', '\u{2003}'] {
            for text in [format!("{space}laptop"), format!("laptop{space}")] {
                assert_eq!(
                    Hostname::parse(&text),
                    Err(InvalidHostname::Hidden(space)),
                    "{text:?}"
                );
            }
        }
        assert!(Hostname::parse("Nick's MacBook Pro").is_ok());
    }

    /// The characters at each range's edges are outside it.
    #[test]
    fn characters_beside_the_format_ranges_are_kept() {
        for kept in [
            '\u{ac}',
            '\u{ae}',
            '\u{200a}',
            '\u{2010}',
            '\u{2065}',
            '\u{2070}',
            '\u{34e}',
            '\u{350}',
            '\u{115e}',
            '\u{1161}',
            '\u{17b3}',
            '\u{17b6}',
            '\u{3163}',
            '\u{3165}',
            '\u{ff9f}',
            '\u{ffa1}',
            '\u{180a}',
            '\u{1810}',
            '\u{fdff}',
            '\u{fe10}',
            '\u{e00ff}',
            '\u{e01f0}',
            '\u{2027}',
        ] {
            assert!(Hostname::parse(&format!("lap{kept}top")).is_ok(), "{kept}");
        }
    }
}
