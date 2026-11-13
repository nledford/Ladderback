//! A location's rule (technical design §3.1, decision 80): when a location
//! may be tried, as a condition on the machine's network link. A rule says
//! which address to try, never whether to trust it. Evaluating it is pure;
//! reading the link is the `NetworkLink` port's.

use super::{InvalidValue, ValueKind};

/// The most alternatives a location's `when` lists.
pub const MAX_RULE_ALTERNATIVES: usize = 8;

/// The network link the machine reaches others by: the interface its default
/// route leaves by, as the platform read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// An Ethernet interface, not a virtual one.
    Wired,
    /// A Wi-Fi interface.
    WiFi,
    /// A tunnel, a bridge, a cellular modem, a phone's tethering.
    Other,
    /// Not Wi-Fi, and the platform can't tell whether it is wired: macOS
    /// until a run on Ethernet has shown how a Mac names its wired ports.
    NotWiFi,
    /// No default route, as just after a wake: nothing says where the next
    /// one will leave by.
    NoRoute,
}

/// The kind of network one alternative of a rule asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkKind {
    /// `{ network = "wired" }`.
    Wired,
    /// `{ network = "wifi" }`: Wi-Fi, whatever network.
    WiFi,
}

impl NetworkKind {
    /// The kind `text` names, as `config.toml` writes it.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        match text {
            "wired" => Ok(Self::Wired),
            "wifi" => Ok(Self::WiFi),
            _ => Err(InvalidValue::new(ValueKind::LocationRule, text)),
        }
    }

    /// The kind as `config.toml` writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::WiFi => "wifi",
        }
    }

    /// What this alternative says of `link`.
    fn verdict(self, link: Link) -> RuleVerdict {
        match (self, link) {
            (Self::Wired, Link::Wired) | (Self::WiFi, Link::WiFi) => RuleVerdict::Holds,
            // Nothing says where the next route leaves by.
            (_, Link::NoRoute) => RuleVerdict::Unknown,
            // Not Wi-Fi may still be wired: the platform can't tell.
            (Self::Wired, Link::NotWiFi) => RuleVerdict::Unknown,
            (Self::Wired, Link::WiFi | Link::Other)
            | (Self::WiFi, Link::Wired | Link::Other | Link::NotWiFi) => RuleVerdict::NotMet,
        }
    }
}

/// What a rule says of the link the machine is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleVerdict {
    /// The location may be tried.
    Holds,
    /// The link was read, and the rule isn't met on it.
    NotMet,
    /// The rule can't be evaluated: the link couldn't be read, or the
    /// platform can't tell what the rule asks. It doesn't hold.
    Unknown,
}

/// A location's `when`: a list of alternatives, of which any one holding
/// makes the location eligible. A location with no rule is always eligible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationRule {
    alternatives: Vec<NetworkKind>,
}

impl LocationRule {
    /// A rule of `alternatives`: at least one, at most
    /// [`MAX_RULE_ALTERNATIVES`].
    pub fn new(alternatives: Vec<NetworkKind>) -> Result<Self, InvalidValue> {
        if alternatives.is_empty() || alternatives.len() > MAX_RULE_ALTERNATIVES {
            return Err(InvalidValue::new(
                ValueKind::LocationRule,
                alternatives.len().to_string(),
            ));
        }
        Ok(Self { alternatives })
    }

    /// The alternatives, in the order written.
    pub fn alternatives(&self) -> &[NetworkKind] {
        &self.alternatives
    }

    /// Whether the rule holds on `link`, `None` when the link couldn't be
    /// read: it holds when any alternative does, else it is unknown when any
    /// alternative can't be evaluated, else it isn't met.
    pub fn holds(&self, link: Option<Link>) -> RuleVerdict {
        let Some(link) = link else {
            return RuleVerdict::Unknown;
        };
        let mut verdict = RuleVerdict::NotMet;
        for alternative in &self.alternatives {
            match alternative.verdict(link) {
                RuleVerdict::Holds => return RuleVerdict::Holds,
                RuleVerdict::Unknown => verdict = RuleVerdict::Unknown,
                RuleVerdict::NotMet => {}
            }
        }
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(alternatives: &[NetworkKind]) -> LocationRule {
        LocationRule::new(alternatives.to_vec()).unwrap()
    }

    #[test]
    fn a_wired_rule_holds_on_a_wired_link_only() {
        let wired = rule(&[NetworkKind::Wired]);
        assert_eq!(wired.holds(Some(Link::Wired)), RuleVerdict::Holds);
        for link in [Link::WiFi, Link::Other] {
            assert_eq!(wired.holds(Some(link)), RuleVerdict::NotMet, "{link:?}");
        }
    }

    #[test]
    fn a_wifi_rule_holds_on_wi_fi_only() {
        let wifi = rule(&[NetworkKind::WiFi]);
        assert_eq!(wifi.holds(Some(Link::WiFi)), RuleVerdict::Holds);
        for link in [Link::Wired, Link::Other, Link::NotWiFi] {
            assert_eq!(wifi.holds(Some(link)), RuleVerdict::NotMet, "{link:?}");
        }
    }

    #[test]
    fn a_link_that_could_not_be_read_evaluates_no_rule() {
        for kind in [NetworkKind::Wired, NetworkKind::WiFi] {
            assert_eq!(rule(&[kind]).holds(None), RuleVerdict::Unknown, "{kind:?}");
        }
    }

    #[test]
    fn no_default_route_evaluates_no_rule() {
        for kind in [NetworkKind::Wired, NetworkKind::WiFi] {
            assert_eq!(
                rule(&[kind]).holds(Some(Link::NoRoute)),
                RuleVerdict::Unknown,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_link_that_may_be_wired_leaves_a_wired_rule_unknown() {
        assert_eq!(
            rule(&[NetworkKind::Wired]).holds(Some(Link::NotWiFi)),
            RuleVerdict::Unknown
        );
    }

    #[test]
    fn a_rule_holds_when_any_alternative_does() {
        let either = rule(&[NetworkKind::Wired, NetworkKind::WiFi]);
        assert_eq!(either.holds(Some(Link::WiFi)), RuleVerdict::Holds);
        assert_eq!(either.holds(Some(Link::Wired)), RuleVerdict::Holds);
        assert_eq!(either.holds(Some(Link::Other)), RuleVerdict::NotMet);
    }

    #[test]
    fn an_alternative_that_cannot_be_evaluated_wins_over_one_not_met() {
        let either = rule(&[NetworkKind::Wired, NetworkKind::WiFi]);
        // Wired can't be told on this link, and Wi-Fi isn't met: unknown,
        // whichever is written first.
        assert_eq!(either.holds(Some(Link::NotWiFi)), RuleVerdict::Unknown);
        let reversed = rule(&[NetworkKind::WiFi, NetworkKind::Wired]);
        assert_eq!(reversed.holds(Some(Link::NotWiFi)), RuleVerdict::Unknown);
    }

    #[test]
    fn a_rule_of_no_alternative_or_too_many_is_refused() {
        assert_eq!(
            LocationRule::new(Vec::new()).unwrap_err().kind,
            ValueKind::LocationRule
        );
        assert!(LocationRule::new(vec![NetworkKind::WiFi; MAX_RULE_ALTERNATIVES]).is_ok());
        assert!(LocationRule::new(vec![NetworkKind::WiFi; MAX_RULE_ALTERNATIVES + 1]).is_err());
    }

    #[test]
    fn a_kind_is_read_as_written_and_nothing_else() {
        assert_eq!(NetworkKind::parse("wired"), Ok(NetworkKind::Wired));
        assert_eq!(NetworkKind::parse("wifi"), Ok(NetworkKind::WiFi));
        for text in ["Wired", "wi-fi", "ethernet", ""] {
            assert_eq!(
                NetworkKind::parse(text).unwrap_err().kind,
                ValueKind::LocationRule,
                "{text:?}"
            );
        }
        for kind in [NetworkKind::Wired, NetworkKind::WiFi] {
            assert_eq!(NetworkKind::parse(kind.as_str()), Ok(kind));
        }
    }
}
