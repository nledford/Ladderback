//! An agent's hostname as pairing registers it (§16, decision 120,
//! "Hostnames"): what the agent is called on the hub, and the one variable
//! field of the signed `Pair` message.

use std::fmt;

/// The most bytes of UTF-8 a hostname has.
pub const MAX_HOSTNAME_BYTES: usize = 255;

/// 1 to 255 bytes of UTF-8 with no control, format or bidirectional-control
/// character. Not limited to ASCII: machines are named "Nick's MacBook Pro"
/// or in other scripts. How two names compare is the registry's rule.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Hostname(String);

/// Why text isn't a hostname.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidHostname {
    Empty,
    TooLong,
    /// A control or format character, the bidirectional controls among
    /// them, which can make a name show as another.
    Hidden(char),
}

impl Hostname {
    pub fn parse(text: &str) -> Result<Self, InvalidHostname> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        todo!()
    }
}

impl fmt::Debug for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
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
    todo!()
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

    /// The characters at each range's edges are outside it.
    #[test]
    fn characters_beside_the_format_ranges_are_kept() {
        for kept in [
            '\u{ac}', '\u{ae}', '\u{200a}', '\u{2010}', '\u{2065}', '\u{2070}',
        ] {
            assert!(Hostname::parse(&format!("lap{kept}top")).is_ok(), "{kept}");
        }
    }
}
