//! Secrets written in the configuration itself, where a reference, a file, a
//! command or the keyring would keep them out of it (technical design §10.2).

/// Where in a repository's settings a secret is written as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineSecret {
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
    /// Where the secret is, as the configuration names it: `password`,
    /// `options.NAME`, `options-hot.NAME` or `options-cold.NAME`, with the name
    /// as one bounded line. Never the secret itself (I4).
    pub fn field(&self) -> String {
        match self {
            Self::Password => "password".to_owned(),
            Self::Option(name) => format!("options.{}", shown(name)),
            Self::OptionHot(name) => format!("options-hot.{}", shown(name)),
            Self::OptionCold(name) => format!("options-cold.{}", shown(name)),
        }
    }
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
