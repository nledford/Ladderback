//! A location's rule (technical design §3.1, decision 80): when a location
//! may be tried, as a condition on the machine's network link and, for a
//! Wi-Fi network, its name. A rule says which address to try, never whether
//! to trust it. Evaluating it is pure; reading the link and the name is the
//! `NetworkLink` port's.

use std::fmt;

use super::{InvalidValue, ValueKind};

/// The most alternatives a location's `when` lists.
pub const MAX_RULE_ALTERNATIVES: usize = 8;

/// The most names an alternative's `only` or `except` lists.
pub const MAX_NETWORK_NAMES: usize = 16;

/// The longest a Wi-Fi network's name can be, in bytes (IEEE 802.11's SSID).
pub const MAX_NETWORK_NAME_BYTES: usize = 32;

/// A Wi-Fi network's name as a rule lists it: what the person wrote, 1 to
/// [`MAX_NETWORK_NAME_BYTES`] bytes. Not a secret: the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkName(String);

impl NetworkName {
    /// The name `text` is, refused when empty or longer than a network's
    /// name can be.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        if text.is_empty() || text.len() > MAX_NETWORK_NAME_BYTES {
            return Err(InvalidValue::new(ValueKind::LocationRule, text));
        }
        Ok(Self(text.to_owned()))
    }

    /// The name as the configuration writes it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The name of the Wi-Fi network the machine is on, as the platform read it:
/// bytes, which need not be UTF-8. It tells where the machine is, so it is
/// compared with a rule's names and never shown, logged or recorded
/// (decision 80, point 3): it has no `Display`, and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct CurrentNetworkName(Box<[u8]>);

impl CurrentNetworkName {
    /// The name `bytes` are; `None` when empty or longer than a network's
    /// name can be, which no reader takes for a name.
    pub fn new(bytes: &[u8]) -> Option<Self> {
        (!bytes.is_empty() && bytes.len() <= MAX_NETWORK_NAME_BYTES).then(|| Self(bytes.into()))
    }
}

impl fmt::Debug for CurrentNetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CurrentNetworkName(hidden)")
    }
}

/// The names an alternative lists, and what they mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameFilter {
    /// `only = [...]`: the network's name is one of these.
    Only(Vec<NetworkName>),
    /// `except = [...]`: the network's name is none of these.
    Except(Vec<NetworkName>),
}

impl NameFilter {
    /// The filter `only` and `except` make, as the configuration lists
    /// them: none for neither, refused for both.
    pub fn of(
        only: Option<Vec<NetworkName>>,
        except: Option<Vec<NetworkName>>,
    ) -> Result<Option<Self>, InvalidValue> {
        match (only, except) {
            (None, None) => Ok(None),
            (Some(only), None) => Ok(Some(Self::Only(only))),
            (None, Some(except)) => Ok(Some(Self::Except(except))),
            (Some(_), Some(_)) => Err(InvalidValue::new(
                ValueKind::LocationRule,
                "only and except",
            )),
        }
    }

    /// The names, in the order written.
    pub fn names(&self) -> &[NetworkName] {
        match self {
            Self::Only(names) | Self::Except(names) => names,
        }
    }
}

/// One alternative of a rule: a kind of network and, for Wi-Fi, the names
/// it may or may not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alternative {
    kind: NetworkKind,
    names: Option<NameFilter>,
}

impl Alternative {
    /// An alternative of `kind`, with `names` only for Wi-Fi: a list of 1
    /// to [`MAX_NETWORK_NAMES`].
    pub fn new(kind: NetworkKind, names: Option<NameFilter>) -> Result<Self, InvalidValue> {
        let Some(names) = names else {
            return Ok(Self { kind, names: None });
        };
        let count = names.names().len();
        if kind != NetworkKind::WiFi || count == 0 || count > MAX_NETWORK_NAMES {
            return Err(InvalidValue::new(
                ValueKind::LocationRule,
                format!("{} with {count} names", kind.as_str()),
            ));
        }
        Ok(Self {
            kind,
            names: Some(names),
        })
    }

    /// The kind of network it asks for.
    pub fn kind(&self) -> NetworkKind {
        self.kind
    }

    /// The names it lists, if any.
    pub fn names(&self) -> Option<&NameFilter> {
        self.names.as_ref()
    }

    /// What this alternative says of `link`, with `name` the Wi-Fi
    /// network's name when it could be read.
    fn verdict(&self, link: Link, name: Option<&CurrentNetworkName>) -> RuleVerdict {
        let verdict = self.kind.verdict(link);
        match (&self.names, verdict) {
            // On Wi-Fi: the name decides, and a name not read can't.
            (Some(names), RuleVerdict::Holds) => match name {
                None => RuleVerdict::Unknown,
                Some(name) => {
                    let listed = names
                        .names()
                        .iter()
                        .any(|listed| listed.as_str().as_bytes() == &*name.0);
                    if listed == matches!(names, NameFilter::Only(_)) {
                        RuleVerdict::Holds
                    } else {
                        RuleVerdict::NotMet
                    }
                }
            },
            _ => verdict,
        }
    }
}

impl From<NetworkKind> for Alternative {
    fn from(kind: NetworkKind) -> Self {
        Self { kind, names: None }
    }
}

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
            (Self::Wired, Link::WiFi | Link::Other) | (Self::WiFi, Link::Wired | Link::Other) => {
                RuleVerdict::NotMet
            }
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
    /// The rule can't be evaluated: the link or the Wi-Fi network's name
    /// couldn't be read. It doesn't hold.
    Unknown,
}

/// A location's `when`: a list of alternatives, of which any one holding
/// makes the location eligible. A location with no rule is always eligible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationRule {
    alternatives: Vec<Alternative>,
}

impl LocationRule {
    /// A rule of `alternatives`: at least one, at most
    /// [`MAX_RULE_ALTERNATIVES`].
    pub fn new(alternatives: Vec<Alternative>) -> Result<Self, InvalidValue> {
        if alternatives.is_empty() || alternatives.len() > MAX_RULE_ALTERNATIVES {
            return Err(InvalidValue::new(
                ValueKind::LocationRule,
                alternatives.len().to_string(),
            ));
        }
        Ok(Self { alternatives })
    }

    /// The alternatives, in the order written.
    pub fn alternatives(&self) -> &[Alternative] {
        &self.alternatives
    }

    /// Whether an alternative lists names, so that evaluating the rule on a
    /// Wi-Fi link needs the network's name.
    pub fn asks_name(&self) -> bool {
        self.alternatives
            .iter()
            .any(|alternative| alternative.names.is_some())
    }

    /// Whether the rule holds on `link`, `None` when the link couldn't be
    /// read, with `name` the Wi-Fi network's name, `None` when it wasn't or
    /// couldn't be read: it holds when any alternative does, else it is
    /// unknown when any alternative can't be evaluated, else it isn't met.
    pub fn holds(&self, link: Option<Link>, name: Option<&CurrentNetworkName>) -> RuleVerdict {
        let Some(link) = link else {
            return RuleVerdict::Unknown;
        };
        let mut verdict = RuleVerdict::NotMet;
        for alternative in &self.alternatives {
            match alternative.verdict(link, name) {
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
        LocationRule::new(
            alternatives
                .iter()
                .copied()
                .map(Alternative::from)
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn a_wired_rule_holds_on_a_wired_link_only() {
        let wired = rule(&[NetworkKind::Wired]);
        assert_eq!(wired.holds(Some(Link::Wired), None), RuleVerdict::Holds);
        for link in [Link::WiFi, Link::Other] {
            assert_eq!(
                wired.holds(Some(link), None),
                RuleVerdict::NotMet,
                "{link:?}"
            );
        }
    }

    #[test]
    fn a_wifi_rule_holds_on_wi_fi_only() {
        let wifi = rule(&[NetworkKind::WiFi]);
        assert_eq!(wifi.holds(Some(Link::WiFi), None), RuleVerdict::Holds);
        for link in [Link::Wired, Link::Other] {
            assert_eq!(
                wifi.holds(Some(link), None),
                RuleVerdict::NotMet,
                "{link:?}"
            );
        }
    }

    #[test]
    fn a_link_that_could_not_be_read_evaluates_no_rule() {
        for kind in [NetworkKind::Wired, NetworkKind::WiFi] {
            assert_eq!(
                rule(&[kind]).holds(None, None),
                RuleVerdict::Unknown,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn no_default_route_evaluates_no_rule() {
        for kind in [NetworkKind::Wired, NetworkKind::WiFi] {
            assert_eq!(
                rule(&[kind]).holds(Some(Link::NoRoute), None),
                RuleVerdict::Unknown,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_rule_holds_when_any_alternative_does() {
        let either = rule(&[NetworkKind::Wired, NetworkKind::WiFi]);
        assert_eq!(either.holds(Some(Link::WiFi), None), RuleVerdict::Holds);
        assert_eq!(either.holds(Some(Link::Wired), None), RuleVerdict::Holds);
        assert_eq!(either.holds(Some(Link::Other), None), RuleVerdict::NotMet);
    }

    #[test]
    fn an_alternative_that_cannot_be_evaluated_wins_over_one_not_met() {
        // On Wi-Fi with no name read, a wired alternative isn't met and one
        // that lists names can't be evaluated: unknown, whichever is
        // written first.
        let names = only(&["Office"]).alternatives().to_vec();
        let wired = Alternative::from(NetworkKind::Wired);
        let either = LocationRule::new([vec![wired.clone()], names.clone()].concat()).unwrap();
        assert_eq!(either.holds(Some(Link::WiFi), None), RuleVerdict::Unknown);
        let reversed = LocationRule::new([names, vec![wired]].concat()).unwrap();
        assert_eq!(reversed.holds(Some(Link::WiFi), None), RuleVerdict::Unknown);
    }

    #[test]
    fn a_rule_of_no_alternative_or_too_many_is_refused() {
        assert_eq!(
            LocationRule::new(Vec::new()).unwrap_err().kind,
            ValueKind::LocationRule
        );
        assert!(LocationRule::new(vec![NetworkKind::WiFi.into(); MAX_RULE_ALTERNATIVES]).is_ok());
        assert!(
            LocationRule::new(vec![NetworkKind::WiFi.into(); MAX_RULE_ALTERNATIVES + 1]).is_err()
        );
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

    fn names(names: &[&str]) -> Vec<NetworkName> {
        names
            .iter()
            .map(|name| NetworkName::parse(name).unwrap())
            .collect()
    }

    fn only(listed: &[&str]) -> LocationRule {
        let names = Some(NameFilter::Only(names(listed)));
        LocationRule::new(vec![Alternative::new(NetworkKind::WiFi, names).unwrap()]).unwrap()
    }

    fn except(listed: &[&str]) -> LocationRule {
        let names = Some(NameFilter::Except(names(listed)));
        LocationRule::new(vec![Alternative::new(NetworkKind::WiFi, names).unwrap()]).unwrap()
    }

    fn on(name: &[u8]) -> CurrentNetworkName {
        CurrentNetworkName::new(name).unwrap()
    }

    #[test]
    fn an_only_rule_holds_on_a_listed_network_alone() {
        let office = only(&["Office", "Lab"]);
        let wifi = Some(Link::WiFi);
        assert_eq!(office.holds(wifi, Some(&on(b"Office"))), RuleVerdict::Holds);
        assert_eq!(office.holds(wifi, Some(&on(b"Lab"))), RuleVerdict::Holds);
        assert_eq!(office.holds(wifi, Some(&on(b"Guest"))), RuleVerdict::NotMet);
        // Byte for byte, case and all.
        assert_eq!(
            office.holds(wifi, Some(&on(b"office"))),
            RuleVerdict::NotMet
        );
        assert_eq!(
            office.holds(wifi, Some(&on(b"Office "))),
            RuleVerdict::NotMet
        );
    }

    #[test]
    fn an_except_rule_holds_on_any_network_not_listed() {
        let not_guest = except(&["Guest"]);
        let wifi = Some(Link::WiFi);
        assert_eq!(
            not_guest.holds(wifi, Some(&on(b"Guest"))),
            RuleVerdict::NotMet
        );
        assert_eq!(
            not_guest.holds(wifi, Some(&on(b"Office"))),
            RuleVerdict::Holds
        );
    }

    #[test]
    fn a_name_that_is_not_utf_8_matches_no_listed_name() {
        let latin1 = on(&[0x43, 0x61, 0x66, 0xe9]);
        let wifi = Some(Link::WiFi);
        assert_eq!(
            only(&["Café"]).holds(wifi, Some(&latin1)),
            RuleVerdict::NotMet
        );
        assert_eq!(
            except(&["Café"]).holds(wifi, Some(&latin1)),
            RuleVerdict::Holds
        );
    }

    #[test]
    fn a_name_that_could_not_be_read_evaluates_no_name_rule() {
        for rule in [only(&["Office"]), except(&["Guest"])] {
            assert_eq!(rule.holds(Some(Link::WiFi), None), RuleVerdict::Unknown);
        }
    }

    #[test]
    fn a_name_rule_is_not_met_off_wi_fi_whatever_the_name() {
        for rule in [only(&["Office"]), except(&["Guest"])] {
            for link in [Link::Wired, Link::Other] {
                assert_eq!(
                    rule.holds(Some(link), None),
                    RuleVerdict::NotMet,
                    "{link:?}"
                );
            }
            assert_eq!(rule.holds(Some(Link::NoRoute), None), RuleVerdict::Unknown);
        }
    }

    #[test]
    fn only_a_rule_with_names_asks_for_the_name() {
        assert!(only(&["Office"]).asks_name());
        assert!(!rule(&[NetworkKind::WiFi, NetworkKind::Wired]).asks_name());
    }

    #[test]
    fn a_listed_name_is_one_to_32_bytes() {
        assert_eq!(NetworkName::parse("Office").unwrap().as_str(), "Office");
        assert!(NetworkName::parse(&"é".repeat(16)).is_ok());
        for text in [
            String::new(),
            "a".repeat(MAX_NETWORK_NAME_BYTES + 1),
            "é".repeat(17),
        ] {
            assert_eq!(
                NetworkName::parse(&text).unwrap_err().kind,
                ValueKind::LocationRule,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_read_name_is_one_to_32_bytes_and_never_shown() {
        assert!(CurrentNetworkName::new(b"").is_none());
        assert!(CurrentNetworkName::new(&[b'a'; MAX_NETWORK_NAME_BYTES + 1]).is_none());
        let name = on(b"canary-network");
        // A log line that holds the value still says what it held.
        assert_eq!(format!("{name:?}"), "CurrentNetworkName(hidden)");
    }

    #[test]
    fn only_or_except_makes_its_filter_both_are_refused_and_neither_lists_none() {
        assert_eq!(
            NameFilter::of(Some(names(&["Office"])), None),
            Ok(Some(NameFilter::Only(names(&["Office"]))))
        );
        assert_eq!(
            NameFilter::of(None, Some(names(&["Guest"]))),
            Ok(Some(NameFilter::Except(names(&["Guest"]))))
        );
        assert_eq!(NameFilter::of(None, None), Ok(None));
        assert_eq!(
            NameFilter::of(Some(names(&["Office"])), Some(names(&["Guest"])))
                .unwrap_err()
                .kind,
            ValueKind::LocationRule
        );
    }

    #[test]
    fn an_alternative_keeps_the_names_it_lists() {
        let listed = NameFilter::Except(names(&["Guest", "Lab"]));
        let alternative = Alternative::new(NetworkKind::WiFi, Some(listed.clone())).unwrap();
        assert_eq!(alternative.names(), Some(&listed));
        assert_eq!(Alternative::from(NetworkKind::WiFi).names(), None);
    }

    #[test]
    fn names_go_with_wi_fi_only_in_a_list_of_1_to_16() {
        let listed = |count: usize| names(&vec!["Office"; count]);
        assert!(Alternative::new(NetworkKind::WiFi, Some(NameFilter::Only(listed(16)))).is_ok());
        for (kind, names) in [
            (NetworkKind::Wired, NameFilter::Only(listed(1))),
            (NetworkKind::WiFi, NameFilter::Only(Vec::new())),
            (
                NetworkKind::WiFi,
                NameFilter::Except(listed(MAX_NETWORK_NAMES + 1)),
            ),
        ] {
            assert_eq!(
                Alternative::new(kind, Some(names.clone()))
                    .unwrap_err()
                    .kind,
                ValueKind::LocationRule,
                "{kind:?} {names:?}"
            );
        }
    }
}
