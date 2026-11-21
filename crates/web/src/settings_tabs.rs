//! The decisions behind Settings' tabs (technical design §4.4, §4.5): one
//! tab per section, the address that opens each, and whether a section's
//! panel is a Tab stop of its own. Host-tested here; `settings.rs` is the
//! glue that draws the tabs, and `page.rs` reads and writes the address.

use crate::components::tabs::Holds;

/// How many sections a build has: the Agents one only with the hub.
const SECTIONS: usize = if cfg!(feature = "hub") { 9 } else { 8 };

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
    /// What AI agents may do without a token (decision 127).
    AiAgents,
    /// The pairing tokens a hub makes for another machine (decision 121),
    /// only in a build with the hub and on a daemon serving as one.
    #[cfg(feature = "hub")]
    Agents,
    /// The dry-run allowlist: the folders a plan dry run an AI agent asks
    /// for may read (decision 89).
    DryRunAllowlist,
    /// The restart of Ladderback.
    Restart,
}

impl Section {
    /// Every section, in the order of the tabs.
    pub(crate) const ALL: [Self; SECTIONS] = [
        Self::Language,
        Self::PageWidth,
        Self::AdminPassword,
        Self::SigningIn,
        Self::ApiTokens,
        Self::AiAgents,
        #[cfg(feature = "hub")]
        Self::Agents,
        Self::DryRunAllowlist,
        Self::Restart,
    ];

    /// The section's name in its tab's address, after the `#`
    /// (`/settings#api-tokens`). No element of the page has it as its `id`:
    /// a browser that changes the address's fragment to an element's `id`
    /// takes focus from where it was (Chromium does), and choosing a tab
    /// leaves focus where it is.
    pub(crate) const fn fragment(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::PageWidth => "page-width",
            Self::AdminPassword => "admin-password",
            Self::SigningIn => "signing-in",
            Self::ApiTokens => "api-tokens",
            Self::AiAgents => "ai-agents",
            #[cfg(feature = "hub")]
            Self::Agents => "agents",
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
    /// Ladderback; and the dry-run allowlist's, whose controls show only once
    /// the daemon has read what it holds, so a read refused or unanswered
    /// leaves words alone. The AI agents' panel holds its choices once the
    /// daemon has read them (`ai_agents_read`), and its first choice is then
    /// the stop, since the panel is taller than a window; until then, and
    /// after a read refused or unanswered, it holds only words.
    pub(crate) fn holds(self, signed_in: bool, ai_agents_read: bool) -> Holds {
        match self {
            Self::Language | Self::PageWidth | Self::AdminPassword => Holds::Controls,
            Self::AiAgents if ai_agents_read => Holds::Controls,
            // Shown only with sign-in off or in a session, always with its
            // form.
            #[cfg(feature = "hub")]
            Self::Agents => Holds::Controls,
            Self::SigningIn | Self::ApiTokens if signed_in => Holds::Controls,
            Self::SigningIn
            | Self::ApiTokens
            | Self::AiAgents
            | Self::DryRunAllowlist
            | Self::Restart => Holds::Text,
        }
    }
}

/// The tab, its index among the tabs `shown` in their order, that the
/// fragment `hash` of the page's address names, as `Location.hash` gives it:
/// the tab an address changed while Settings is open chooses. No fragment
/// names the first tab, as Settings' own address opens there. `None` when it
/// names no section, or one whose tab isn't shown, which leaves the tab as it
/// is.
pub(crate) fn tab_named(hash: &str, shown: &[Section]) -> Option<usize> {
    if matches!(hash, "" | "#") {
        return (!shown.is_empty()).then_some(0);
    }
    let named = Section::named(hash)?;
    shown.iter().position(|section| *section == named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_section_has_a_name_of_its_own_in_its_address() {
        let names: Vec<_> = Section::ALL.map(Section::fragment).into();
        assert_eq!(
            names,
            [
                "language",
                "page-width",
                "admin-password",
                "signing-in",
                "api-tokens",
                "ai-agents",
                #[cfg(feature = "hub")]
                "agents",
                "dry-run-allowlist",
                "restart"
            ]
        );
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
    fn an_address_changed_while_settings_is_open_chooses_the_tab_of_the_section_it_names() {
        for (index, section) in Section::ALL.into_iter().enumerate() {
            let hash = format!("#{}", section.fragment());
            assert_eq!(tab_named(&hash, &Section::ALL), Some(index), "{hash}");
        }
        // Without the admin password's and signing in's tabs, the API
        // tokens' is the third.
        let shown = [
            Section::Language,
            Section::PageWidth,
            Section::ApiTokens,
            Section::DryRunAllowlist,
            Section::Restart,
        ];
        assert_eq!(tab_named("#api-tokens", &shown), Some(2));
        assert_eq!(tab_named("#restart", &shown), Some(4));
    }

    // Settings' own address, `/settings` with no fragment, is its first
    // tab's, as the page opens there: going Back to it shows that tab.
    #[test]
    fn an_address_changed_to_settings_own_with_no_fragment_chooses_the_first_tab() {
        assert_eq!(tab_named("", &Section::ALL), Some(0));
        assert_eq!(tab_named("#", &Section::ALL), Some(0));
        assert_eq!(tab_named("", &[]), None);
    }

    #[test]
    fn an_address_changed_to_name_no_section_or_one_not_shown_leaves_the_tab_as_it_is() {
        for hash in ["#backups", "#API-tokens", "##", "restart"] {
            assert_eq!(tab_named(hash, &Section::ALL), None, "{hash:?}");
        }
        let shown = [Section::Language, Section::PageWidth, Section::Restart];
        assert_eq!(tab_named("#admin-password", &shown), None);
        assert_eq!(tab_named("#signing-in", &shown), None);
    }

    #[test]
    fn signed_in_every_section_but_the_dry_run_allowlist_and_the_restart_holds_a_control() {
        let held: Vec<_> = Section::ALL.map(|section| section.holds(true, true)).into();
        assert_eq!(
            held,
            [
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                #[cfg(feature = "hub")]
                Holds::Controls,
                Holds::Text,
                Holds::Text
            ]
        );
    }

    #[test]
    fn with_sign_in_off_signing_in_the_api_tokens_the_dry_run_allowlist_and_the_restart_may_hold_only_words()
     {
        let held: Vec<_> = Section::ALL
            .map(|section| section.holds(false, true))
            .into();
        assert_eq!(
            held,
            [
                Holds::Controls,
                Holds::Controls,
                Holds::Controls,
                Holds::Text,
                Holds::Text,
                Holds::Controls,
                #[cfg(feature = "hub")]
                Holds::Controls,
                Holds::Text,
                Holds::Text
            ]
        );
    }

    #[test]
    fn the_ai_agents_panel_holds_words_alone_until_the_daemon_has_read_its_permissions() {
        for signed_in in [true, false] {
            assert_eq!(Section::AiAgents.holds(signed_in, false), Holds::Text);
            assert_eq!(Section::AiAgents.holds(signed_in, true), Holds::Controls);
            for section in Section::ALL.into_iter().filter(|s| *s != Section::AiAgents) {
                assert_eq!(
                    section.holds(signed_in, false),
                    section.holds(signed_in, true),
                    "{section:?}"
                );
            }
        }
    }
}
