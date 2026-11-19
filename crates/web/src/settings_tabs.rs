//! The decisions behind Settings' tabs (technical design §4.4, §4.5): one
//! tab per section, the address that opens each, and whether a section's
//! panel is a Tab stop of its own. Host-tested here; `settings.rs` is the
//! glue that draws the tabs, and `page.rs` reads and writes the address.

use crate::components::tabs::Holds;
use crate::{language_choice, page_width};

/// A section of Settings, each a tab of its own, in the order the tabs
/// stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// The language of the web UI, the first tab.
    Language,
    /// The width of each page.
    PageWidth,
    /// The admin password.
    AdminPassword,
    /// Signing in: Stay signed in for and Sign out everywhere.
    SigningIn,
    /// The API tokens.
    ApiTokens,
    /// The dry-run allowlist: the folders a plan dry run an AI agent asks
    /// for may read (decision 89).
    DryRunAllowlist,
    /// The restart of Ladderback.
    Restart,
}

impl Section {
    /// Every section, in the order of the tabs.
    pub(crate) const ALL: [Self; 7] = [
        Self::Language,
        Self::PageWidth,
        Self::AdminPassword,
        Self::SigningIn,
        Self::ApiTokens,
        Self::DryRunAllowlist,
        Self::Restart,
    ];

    /// The section's name in its tab's address, after the `#`
    /// (`/settings#api-tokens`).
    pub(crate) const fn fragment(self) -> &'static str {
        match self {
            Self::Language => language_choice::SECTION_ID,
            Self::PageWidth => page_width::SECTION_ID,
            Self::AdminPassword => "admin-password",
            Self::SigningIn => "signing-in",
            Self::ApiTokens => "api-tokens",
            Self::DryRunAllowlist => "dry-run-allowlist",
            Self::Restart => "restart",
        }
    }

    /// The section the fragment `hash` of the page's address names, as
    /// `Location.hash` gives it (`#api-tokens`, or empty for none), or `None`
    /// when it names none. The address is outside input: anything but a
    /// section's own name, written exactly, names none.
    pub(crate) fn named(hash: &str) -> Option<Self> {
        let name = hash.strip_prefix('#')?;
        Self::ALL
            .into_iter()
            .find(|section| section.fragment() == name)
    }

    /// What the section's panel holds, which decides whether the panel is a
    /// Tab stop of its own: every section has a control but two with
    /// sign-in off (`signed_in` false), which then only say why; the
    /// restart, which has a button only where a service manager runs
    /// Ladderback; and the dry-run allowlist, whose controls show only once
    /// the daemon has read the list, so a read refused or unanswered leaves
    /// words alone.
    pub(crate) fn holds(self, signed_in: bool) -> Holds {
        match self {
            Self::Language | Self::PageWidth | Self::AdminPassword => Holds::Controls,
            Self::SigningIn | Self::ApiTokens if signed_in => Holds::Controls,
            Self::SigningIn | Self::ApiTokens | Self::DryRunAllowlist | Self::Restart => {
                Holds::Text
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_section_has_a_name_of_its_own_in_its_address_and_the_language_and_page_width_keep_their_sections_ids()
     {
        let names: Vec<_> = Section::ALL.map(Section::fragment).into();
        assert_eq!(
            names,
            [
                "language",
                "page-width",
                "admin-password",
                "signing-in",
                "api-tokens",
                "dry-run-allowlist",
                "restart"
            ]
        );
        assert_eq!(Section::Language.fragment(), language_choice::SECTION_ID);
        assert_eq!(Section::PageWidth.fragment(), page_width::SECTION_ID);
    }

    #[test]
    fn an_address_names_the_section_whose_name_follows_its_hash() {
        for section in Section::ALL {
            assert_eq!(
                Section::named(&format!("#{}", section.fragment())),
                Some(section)
            );
        }
    }

    #[test]
    fn an_address_without_a_section_s_own_name_after_its_hash_names_none() {
        for hash in [
            "",
            "#",
            "api-tokens",
            "#API-tokens",
            "#api-tokens ",
            "##api-tokens",
            "#backups",
        ] {
            assert_eq!(Section::named(hash), None, "{hash:?}");
        }
    }

    #[test]
    fn signed_in_every_section_but_the_dry_run_allowlist_and_the_restart_holds_a_control() {
        let held: Vec<_> = Section::ALL.map(|section| section.holds(true)).into();
        assert_eq!(
            held,
            [
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Text,
                Holds::Text
            ]
        );
    }

    #[test]
    fn with_sign_in_off_signing_in_the_api_tokens_the_dry_run_allowlist_and_the_restart_may_hold_only_words()
     {
        let held: Vec<_> = Section::ALL.map(|section| section.holds(false)).into();
        assert_eq!(
            held,
            [
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Text,
                Holds::Text,
                Holds::Text,
                Holds::Text
            ]
        );
    }
}
