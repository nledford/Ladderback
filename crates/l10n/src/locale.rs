//! Which locales to speak (technical design §8.4): the preferences a
//! command-line program reads from the POSIX environment, and the negotiation
//! of those preferences against the catalogs the binary carries.
//!
//! Tags are parsed and never used as file names, because the catalogs are
//! compiled in. A preference that doesn't parse is ignored.

use unic_langid::LanguageIdentifier;

/// The most preferences one environment variable contributes. `LANGUAGE`
/// is a list, and the environment is outside input.
pub(crate) const MAX_PREFERENCES: usize = 16;

/// The longest locale name read, in bytes. The longest real ones, such as
/// `sr_RS.UTF-8@latin`, are well under it.
pub(crate) const MAX_NAME_LEN: usize = 64;

/// The four environment variables a POSIX program reads its language from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PosixEnv {
    pub language: Option<String>,
    pub lc_all: Option<String>,
    pub lc_messages: Option<String>,
    pub lang: Option<String>,
}

impl PosixEnv {
    /// The process's own environment.
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the variables through `get`, which takes a variable's name.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let _ = get;
        todo!()
    }

    /// The languages asked for, best first.
    ///
    /// The locale is the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is
    /// set and not empty. When it is `C` or `POSIX`, which ask for no
    /// translation, nothing is asked for, whatever `LANGUAGE` says. Otherwise
    /// the entries of `LANGUAGE`, a colon-separated list, come first and the
    /// locale last. A value is normalized to a language tag: the codeset and
    /// the modifier are dropped (`en_US.UTF-8@euro` is `en-US`) and `_`
    /// becomes `-`. What is not a language tag is left out.
    pub fn preferences(&self) -> Vec<LanguageIdentifier> {
        todo!()
    }
}

/// Normalizes one POSIX locale value (`en_US.UTF-8@euro`) to a language tag
/// (`en-US`), or `None` for `C`, `POSIX`, an empty value and anything that
/// isn't a language tag.
pub(crate) fn posix_tag(raw: &str) -> Option<LanguageIdentifier> {
    let _ = raw;
    todo!()
}

/// The locales among `available` to look a message up in, best first.
///
/// Each preference in turn brings the available locales of its language,
/// closest first (`fluent-langneg`, filtering). `en-US` comes last when
/// nothing brought it earlier, and the chain ends there: every message
/// exists in `en-US`, so a locale after it could never be used.
pub(crate) fn negotiate(
    preferences: &[LanguageIdentifier],
    available: &[LanguageIdentifier],
) -> Vec<LanguageIdentifier> {
    let _ = (preferences, available);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(raw: &str) -> LanguageIdentifier {
        raw.parse().unwrap()
    }

    fn tags(raw: &[&str]) -> Vec<LanguageIdentifier> {
        raw.iter().map(|raw| tag(raw)).collect()
    }

    fn env(
        language: Option<&str>,
        lc_all: Option<&str>,
        lc_messages: Option<&str>,
        lang: Option<&str>,
    ) -> PosixEnv {
        let own = |value: Option<&str>| value.map(str::to_owned);
        PosixEnv {
            language: own(language),
            lc_all: own(lc_all),
            lc_messages: own(lc_messages),
            lang: own(lang),
        }
    }

    #[test]
    fn a_posix_locale_is_normalized_to_a_language_tag() {
        assert_eq!(posix_tag("en_US.UTF-8"), Some(tag("en-US")));
        assert_eq!(posix_tag("de_DE@euro"), Some(tag("de-DE")));
        assert_eq!(posix_tag("sr_RS.UTF-8@latin"), Some(tag("sr-RS")));
        assert_eq!(posix_tag("ja_JP.eucJP"), Some(tag("ja-JP")));
        assert_eq!(posix_tag("fr"), Some(tag("fr")));
        assert_eq!(posix_tag("  pt_BR "), Some(tag("pt-BR")));
        assert_eq!(posix_tag("EN_us"), Some(tag("en-US")));
    }

    #[test]
    fn text_that_is_not_a_language_asks_for_no_language() {
        let too_long = "a".repeat(MAX_NAME_LEN + 1);
        for raw in [
            "",
            "   ",
            "C",
            "POSIX",
            "C.UTF-8",
            "und",
            "x-private",
            "../../etc/passwd",
            "en/US",
            too_long.as_str(),
        ] {
            assert_eq!(posix_tag(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn the_locale_is_the_first_of_the_three_that_is_set() {
        let all = env(
            None,
            Some("ja_JP.UTF-8"),
            Some("pt_BR.UTF-8"),
            Some("en_US"),
        );
        assert_eq!(all.preferences(), tags(&["ja-JP"]));
        // An empty value counts as unset.
        let messages = env(None, Some(""), Some("pt_BR.UTF-8"), Some("en_US"));
        assert_eq!(messages.preferences(), tags(&["pt-BR"]));
        let lang = env(None, None, None, Some("de_DE.UTF-8"));
        assert_eq!(lang.preferences(), tags(&["de-DE"]));
    }

    #[test]
    fn language_comes_before_the_locale_and_skips_what_is_not_a_tag() {
        let env = env(
            Some("de::xx-!!:fr_CA.UTF-8:C"),
            None,
            None,
            Some("en_US.UTF-8"),
        );
        assert_eq!(env.preferences(), tags(&["de", "fr-CA", "en-US"]));
    }

    #[test]
    fn language_alone_is_honored() {
        let env = env(Some("de:fr"), None, None, None);
        assert_eq!(env.preferences(), tags(&["de", "fr"]));
    }

    #[test]
    fn the_language_list_is_read_up_to_four_entries_for_each_one_kept() {
        let looked_at = MAX_PREFERENCES * 4;
        // Entries that aren't language tags, then a language: it is read as
        // the last entry looked at, and not as the one after.
        let junk = |count: usize| vec!["1"; count].join(":");
        let last = env(
            Some(&format!("{}:de", junk(looked_at - 1))),
            None,
            None,
            None,
        );
        assert_eq!(last.preferences(), tags(&["de"]));
        let past = env(Some(&format!("{}:de", junk(looked_at))), None, None, None);
        assert_eq!(past.preferences(), tags(&[]));
    }

    #[test]
    fn a_name_of_the_longest_length_read_is_read_and_one_byte_more_is_not() {
        // The codeset after the language counts toward the length.
        let padded = |len: usize| format!("en_US.{}", "x".repeat(len - "en_US.".len()));
        assert_eq!(posix_tag(&padded(MAX_NAME_LEN)), Some(tag("en-US")));
        assert_eq!(posix_tag(&padded(MAX_NAME_LEN + 1)), None);
    }

    #[test]
    fn the_process_environment_is_read_through_the_four_variables() {
        // The test changes no variable of this process. It runs itself again
        // with the variables set, and that copy does the checking.
        const IN_COPY: &str = "STEADING_L10N_ENV_TEST_COPY";
        const NAME: &str =
            "locale::tests::the_process_environment_is_read_through_the_four_variables";
        if std::env::var_os(IN_COPY).is_some() {
            let read = PosixEnv::from_process();
            assert_eq!(read.language.as_deref(), Some("fr:de"));
            assert_eq!(read.lc_all.as_deref(), Some("fr_FR.UTF-8"));
            assert_eq!(read.lc_messages.as_deref(), Some("de_DE"));
            assert_eq!(read.lang.as_deref(), Some("en_GB"));
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture"])
            .env(IN_COPY, "1")
            .env("LANGUAGE", "fr:de")
            .env("LC_ALL", "fr_FR.UTF-8")
            .env("LC_MESSAGES", "de_DE")
            .env("LANG", "en_GB")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "1 passed" shows that the copy ran the test, and did not just match none.
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}{stderr}"
        );
    }

    #[test]
    fn the_c_locale_asks_for_no_translation_whatever_else_is_set() {
        for locale in ["C", "POSIX", "C.UTF-8"] {
            let forced = env(Some("de"), Some(locale), None, Some("fr_FR.UTF-8"));
            assert_eq!(forced.preferences(), tags(&[]), "LC_ALL={locale}");
            let by_default = env(Some("de"), None, None, Some(locale));
            assert_eq!(by_default.preferences(), tags(&[]), "LANG={locale}");
        }
        assert_eq!(PosixEnv::default().preferences(), tags(&[]));
    }

    #[test]
    fn the_environment_is_read_through_the_lookup_it_is_given() {
        let read = PosixEnv::from_lookup(|name| match name {
            "LANGUAGE" => Some("de".to_owned()),
            "LC_ALL" => Some("fr".to_owned()),
            "LC_MESSAGES" => Some("ja".to_owned()),
            "LANG" => Some("pt".to_owned()),
            _ => None,
        });
        assert_eq!(read, env(Some("de"), Some("fr"), Some("ja"), Some("pt")));
    }

    #[test]
    fn a_hostile_environment_is_read_within_bounds() {
        let flood = "de:".repeat(100_000);
        let preferences = env(Some(&flood), None, None, Some("en_US")).preferences();
        assert!(
            preferences.len() <= MAX_PREFERENCES + 1,
            "{}",
            preferences.len()
        );
        let huge = "a".repeat(1 << 20);
        assert_eq!(env(Some(&huge), None, None, None).preferences(), tags(&[]));
    }

    #[test]
    fn a_preference_is_served_by_every_available_locale_of_its_language() {
        let available = tags(&["en-US", "de", "de-AT", "ja"]);
        let chain = |preferences: &[&str]| negotiate(&tags(preferences), &available);
        assert_eq!(chain(&["de-DE"]), tags(&["de", "de-AT", "en-US"]));
        assert_eq!(chain(&["fr", "de"]), tags(&["de", "de-AT", "en-US"]));
        assert_eq!(
            chain(&["ja-JP", "de"]),
            tags(&["ja", "de", "de-AT", "en-US"])
        );
    }

    #[test]
    fn a_preference_nothing_serves_falls_back_to_en_us() {
        let available = tags(&["en-US", "de"]);
        assert_eq!(
            negotiate(&tags(&["pt-BR", "zh"]), &available),
            tags(&["en-US"])
        );
        assert_eq!(negotiate(&[], &available), tags(&["en-US"]));
        assert_eq!(negotiate(&tags(&["en-GB"]), &available), tags(&["en-US"]));
    }

    #[test]
    fn the_chain_ends_at_en_us_because_nothing_after_it_could_be_used() {
        let available = tags(&["en-US", "de", "ja"]);
        let chain = |preferences: &[&str]| negotiate(&tags(preferences), &available);
        assert_eq!(chain(&["en-US", "de"]), tags(&["en-US"]));
        assert_eq!(chain(&["de", "en-US", "ja"]), tags(&["de", "en-US"]));
    }
}
