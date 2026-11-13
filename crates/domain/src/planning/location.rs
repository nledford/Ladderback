//! A repository's locations (technical design §6.1, decision 80): the places
//! one repository can be reached at, in the order they are tried. The first
//! is the entry's own `repository`, `repo-hot` and option sets; each further
//! one is a `[[repositories.<alias>.locations]]` table with the same keys. A
//! location's options never apply to another location, so one server's
//! credential is never sent to another; the password is the repository's.

use std::collections::BTreeMap;
use std::fmt;

use super::repository::Place;
use super::{
    InvalidValue, LocationRule, NetworkKind, Plan, RepositoryConfig, RepositorySettings, ValueKind,
    masked_options,
};
use crate::PlanId;
use crate::secrets::{
    REDACTED, Redactor, RepositorySecrets, SecretRef, SecretValue, is_secret_option,
    location_secret_written,
};

/// The most locations a repository has, the first included.
pub const MAX_LOCATIONS: usize = 4;

/// How long a probe of one location may take, connect, read, open and ID
/// together, before it is killed and the location is skipped as silent.
pub const LOCATION_PROBE_SECONDS: i64 = 8;

/// The longest address an operation keeps of a location, in bytes, cut on a
/// character boundary: the record names it, it doesn't reproduce it.
pub const MAX_RECORDED_ADDRESS_BYTES: usize = 256;

/// One place a repository can be reached: a `repository` string, an optional
/// `repo-hot`, and the option sets with the credentials they hold. Its
/// `Debug` form masks what can be a secret, as [`RepositorySettings`]' does.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Location {
    pub repository: String,
    pub repo_hot: Option<String>,
    pub options: BTreeMap<String, String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
    /// When the location may be tried; `None` is always.
    pub when: Option<LocationRule>,
}

impl Location {
    /// The location `settings` describe: everything but the password.
    pub fn of(settings: &RepositorySettings) -> Self {
        Self {
            repository: settings.repository.clone(),
            repo_hot: settings.repo_hot.clone(),
            options: settings.options.clone(),
            options_hot: settings.options_hot.clone(),
            options_cold: settings.options_cold.clone(),
            when: None,
        }
    }

    /// The settings that open the repository at this location, with the
    /// repository's `password`.
    pub fn with_password(&self, password: SecretRef) -> RepositorySettings {
        RepositorySettings {
            repository: self.repository.clone(),
            repo_hot: self.repo_hot.clone(),
            password,
            options: self.options.clone(),
            options_hot: self.options_hot.clone(),
            options_cold: self.options_cold.clone(),
        }
    }

    /// Whether any option set holds a credential: an option whose name says
    /// it is one, written or a reference.
    fn carries_credential_option(&self) -> bool {
        [&self.options, &self.options_hot, &self.options_cold]
            .into_iter()
            .flatten()
            .any(|(name, value)| is_secret_option(name) && !value.is_empty())
    }

    /// Whether `repository` or `repo-hot` carries a credential in its text: a
    /// user and password in its URL (written or a reference), or an rclone
    /// connection string's.
    fn carries_credential_in_address(&self) -> bool {
        std::iter::once(self.repository.as_str())
            .chain(self.repo_hot.as_deref())
            .any(|address| has_userinfo(address) || location_secret_written(address))
    }

    fn carries_credential(&self) -> bool {
        self.carries_credential_in_address() || self.carries_credential_option()
    }

    /// The value of the option `name` in any set, compared without case.
    fn option(&self, name: &str) -> Option<&str> {
        [&self.options, &self.options_hot, &self.options_cold]
            .into_iter()
            .flatten()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Whether the URL in `address` has a userinfo (`user:password@`): what
/// follows the first `://` up to the first `/`, `?` or `#` holds an `@`.
fn has_userinfo(address: &str) -> bool {
    address.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    })
}

/// Whether `text` starts with `prefix`, compared without ASCII case.
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

impl Location {
    /// The address as written, with the credentials written in it masked:
    /// what an operation keeps of a location, never a secret.
    pub fn masked_address(&self) -> String {
        self.masked(&self.repository)
    }

    /// `repo-hot` as written, masked the same way.
    pub fn masked_repo_hot(&self) -> Option<String> {
        self.repo_hot.as_deref().map(|hot| self.masked(hot))
    }

    fn masked(&self, text: &str) -> String {
        let none = SecretValue::new("");
        Redactor::for_repository(RepositorySecrets {
            repository: &self.repository,
            repo_hot: self.repo_hot.as_deref(),
            password: &none,
            options: &self.options,
            options_hot: &self.options_hot,
            options_cold: &self.options_cold,
            resolved: &[],
        })
        .redact(text)
    }
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Location")
            .field("repository", &self.masked_address())
            .field("repo_hot", &self.repo_hot.as_ref().map(|_| REDACTED))
            .field("options", &masked_options(&self.options))
            .field("options_hot", &masked_options(&self.options_hot))
            .field("options_cold", &masked_options(&self.options_cold))
            .field("when", &self.when)
            .finish()
    }
}

/// The locations of an entry after its first, in the order written. Empty
/// for a repository reached at one location, which is unchanged in every way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locations {
    /// The first location's rule: its `when` is in the repository's table.
    pub first_when: Option<LocationRule>,
    pub further: Vec<Location>,
}

/// Why a location's credentials can be read by whoever answers at its
/// address (§10.6): the cause the warning `location-plain-http-credentials`
/// names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposedCredentials {
    /// A `rest:` location written `http://` that carries a user and password
    /// or a credential option.
    PlainHttp,
    /// An `https` REST location with `http-insecure-tls` true: any
    /// certificate is accepted, so an impostor reads the credentials.
    UncheckedCertificate,
    /// An OpenDAL location whose `endpoint` or `url` is `http://`, or an
    /// rclone connection string whose `url` is, that carries a credential.
    PlainEndpoint,
    /// `opendal:ftp` with a credential.
    PlainFtp,
}

impl ExposedCredentials {
    /// The cause's code, as the warning carries it.
    pub fn code(self) -> &'static str {
        match self {
            Self::PlainHttp => "plain-http",
            Self::UncheckedCertificate => "unchecked-certificate",
            Self::PlainEndpoint => "plain-endpoint",
            Self::PlainFtp => "plain-ftp",
        }
    }

    /// The cause of `settings`, a location as written or as resolved, when
    /// whoever answers at its address can read its credentials.
    pub fn of(settings: &RepositorySettings) -> Option<Self> {
        Self::of_location(&Location::of(settings))
    }

    /// The cause of `location`, as written or as resolved.
    pub fn of_location(location: &Location) -> Option<Self> {
        let address = location.repository.as_str();
        if starts_with_ignore_case(address, "rest:http://") {
            return location.carries_credential().then_some(Self::PlainHttp);
        }
        if starts_with_ignore_case(address, "rest:https://") {
            let insecure = location
                .option("http-insecure-tls")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));
            return insecure.then_some(Self::UncheckedCertificate);
        }
        if let Some(service) = address.get("opendal:".len()..)
            && starts_with_ignore_case(address, "opendal:")
        {
            let scheme = service
                .split_once(':')
                .map_or(service, |(scheme, _)| scheme);
            if scheme.eq_ignore_ascii_case("ftp") {
                return location.carries_credential().then_some(Self::PlainFtp);
            }
            let plain = ["endpoint", "url"].into_iter().any(|name| {
                location
                    .option(name)
                    .is_some_and(|value| starts_with_ignore_case(value.trim(), "http://"))
            });
            return (plain && location.carries_credential()).then_some(Self::PlainEndpoint);
        }
        if starts_with_ignore_case(address, "rclone:") {
            // rclone quotes a value that holds a `:`, such as a URL.
            let plain = address
                .to_ascii_lowercase()
                .replace(['"', '\''], "")
                .contains("url=http://");
            return (plain && location.carries_credential()).then_some(Self::PlainEndpoint);
        }
        None
    }
}

/// A location of an entry whose credentials can be read by whoever answers
/// at its address. It names the location by its position, from 1, and
/// never its address or a credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocationWarning {
    pub position: u8,
    pub cause: ExposedCredentials,
}

/// What the Repositories page notes about an entry's locations' rules
/// (decision 80, point 8): what they mean for the runs of the plans that
/// back up to it. Notes, not errors, and never a network's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleNote {
    /// Every location has a rule: a scheduled run is skipped on a network
    /// where none is known to hold, and any other run fails there.
    EveryLocationRuled,
    /// The last location has a rule, and `plan`, which has a schedule,
    /// doesn't retry a failed run: a run that finds no location usable, as
    /// while a slow LAN comes up after a wake, fails until the next one.
    LastRuledWithoutRetry { plan: PlanId },
    /// The rule of the location at `position`, from 1, asks for a wired
    /// network, which this machine can't tell from other links (macOS until
    /// a run on Ethernet has shown how a Mac names its wired ports): that
    /// alternative never holds here.
    WiredUnknown { position: u8 },
    /// The rule of the location at `position`, from 1, lists Wi-Fi network
    /// names, and this machine can't read the name of the network it is on
    /// (a macOS release that hides it, a Linux machine without
    /// NetworkManager): those alternatives never hold here.
    NameUnreadable { position: u8 },
}

impl RepositoryConfig {
    /// How many locations the entry has: 1 and the further ones.
    pub fn location_count(&self) -> usize {
        self.locations.further.len().saturating_add(1)
    }

    /// Whether the entry has more than one location: only then is a job's
    /// location chosen, by a probe of each.
    pub fn has_several_locations(&self) -> bool {
        !self.locations.further.is_empty()
    }

    /// The settings that open the repository at each location, in order,
    /// each with the repository's password.
    pub fn location_settings(&self) -> Vec<RepositorySettings> {
        std::iter::once(self.settings.clone())
            .chain(
                self.locations
                    .further
                    .iter()
                    .map(|location| location.with_password(self.settings.password.clone())),
            )
            .collect()
    }

    /// Each location's rule, in order, the first included; `None` for a
    /// location with no rule.
    pub fn location_rules(&self) -> Vec<Option<&LocationRule>> {
        std::iter::once(self.locations.first_when.as_ref())
            .chain(
                self.locations
                    .further
                    .iter()
                    .map(|location| location.when.as_ref()),
            )
            .collect()
    }

    /// Whether a job's location is chosen before it opens the repository:
    /// when there are several, or when one has a rule.
    pub fn chooses_location(&self) -> bool {
        self.has_several_locations() || self.location_rules().iter().any(Option::is_some)
    }

    /// The positions, from 1, of the locations that have a rule.
    pub fn ruled_positions(&self) -> Vec<u8> {
        self.location_rules()
            .into_iter()
            .zip(1u8..)
            .filter_map(|(rule, position)| rule.map(|_| position))
            .collect()
    }

    /// Where each location points, in order, the first included.
    pub(super) fn places(&self) -> impl Iterator<Item = Place<'_>> {
        std::iter::once(self.settings.place()).chain(self.locations.further.iter().map(
            |location| Place {
                repository: &location.repository,
                repo_hot: location.repo_hot.as_deref(),
                options: &location.options,
                options_hot: &location.options_hot,
                options_cold: &location.options_cold,
            },
        ))
    }

    /// Whether any location of the entry is at `place`.
    pub(super) fn has_location(&self, place: &Place<'_>) -> bool {
        self.places().any(|own| own.is(place))
    }

    /// The first further location, by its index among the further ones,
    /// that is at the same location as an earlier location of the entry:
    /// two locations of one entry may not be one (decision 80, point 8).
    pub fn repeated_location(&self) -> Option<usize> {
        self.places()
            .enumerate()
            .skip(1)
            .find(|(n, place)| self.places().take(*n).any(|earlier| earlier.is(place)))
            .map(|(n, _)| n.saturating_sub(1))
    }

    /// The entry's locations whose credentials can be read by whoever
    /// answers at their address, as written: none for an entry with one
    /// location, which is unchanged in every way.
    pub fn location_warnings(&self) -> Vec<LocationWarning> {
        std::iter::once(Location::of(&self.settings))
            .chain(self.locations.further.iter().cloned())
            .zip(1u8..)
            .filter_map(|(location, position)| {
                ExposedCredentials::of_location(&location)
                    .map(|cause| LocationWarning { position, cause })
            })
            .collect()
    }

    /// The notes about the entry's locations' rules, given `plans`, the
    /// plans that back up to it, whether this machine can tell a wired link
    /// from other links, and whether it can read the Wi-Fi network's name:
    /// none for an entry whose locations have no rule.
    pub fn rule_notes(
        &self,
        plans: &[&Plan],
        tells_wired: bool,
        reads_names: bool,
    ) -> Vec<RuleNote> {
        let rules = self.location_rules();
        let mut notes = Vec::new();
        if rules.iter().all(Option::is_some) {
            notes.push(RuleNote::EveryLocationRuled);
        }
        if rules.last().is_some_and(Option::is_some) {
            notes.extend(
                plans
                    .iter()
                    .filter(|plan| plan.schedule.is_some() && plan.retry.is_none())
                    .map(|plan| RuleNote::LastRuledWithoutRetry {
                        plan: plan.id.clone(),
                    }),
            );
        }
        if !tells_wired {
            notes.extend(rules.iter().zip(1u8..).filter_map(|(rule, position)| {
                rule.filter(|rule| {
                    rule.alternatives()
                        .iter()
                        .any(|alternative| alternative.kind() == NetworkKind::Wired)
                })
                .map(|_| RuleNote::WiredUnknown { position })
            }));
        }
        if !reads_names {
            notes.extend(rules.iter().zip(1u8..).filter_map(|(rule, position)| {
                rule.filter(|rule| rule.asks_name())
                    .map(|_| RuleNote::NameUnreadable { position })
            }));
        }
        notes
    }
}

/// Refuses a set of further locations an entry can't hold: more than
/// [`MAX_LOCATIONS`] in all.
pub fn check_location_count(further: usize) -> Result<(), InvalidValue> {
    if further.saturating_add(1) > MAX_LOCATIONS {
        return Err(InvalidValue::new(ValueKind::Location, ""));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(address: &str, options: &[(&str, &str)]) -> Location {
        Location {
            repository: address.to_owned(),
            options: options
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            ..Location::default()
        }
    }

    #[test]
    fn a_location_and_its_password_make_the_settings_it_came_from() {
        let settings = location("rest:https://nas/r", &[("http-cacert", "/ca.pem")])
            .with_password(SecretRef::Env("PW".to_owned()));
        assert_eq!(
            Location::of(&settings),
            location("rest:https://nas/r", &[("http-cacert", "/ca.pem")])
        );
        assert_eq!(settings.password, SecretRef::Env("PW".to_owned()));
    }

    #[test]
    fn a_location_debug_form_masks_its_credentials() {
        let shown = format!(
            "{:?}",
            location("rest:http://demo:CANARY@nas/r", &[("password", "CANARY")])
        );
        assert!(!shown.contains("CANARY"), "{shown}");
        assert!(shown.contains("password"), "{shown}");
    }

    #[test]
    fn plain_http_with_credentials_is_exposed_and_without_them_is_not() {
        let of = |address, options: &[(&str, &str)]| {
            ExposedCredentials::of_location(&location(address, options))
        };
        assert_eq!(
            of("rest:http://demo:pw@nas/r", &[]),
            Some(ExposedCredentials::PlainHttp)
        );
        assert_eq!(
            of("rest:http://demo:${PW}@nas/r", &[]),
            Some(ExposedCredentials::PlainHttp)
        );
        assert_eq!(
            of("rest:HTTP://nas/r", &[("password", "${PW}")]),
            Some(ExposedCredentials::PlainHttp)
        );
        assert_eq!(of("rest:http://nas/r", &[]), None);
        assert_eq!(of("rest:http://nas/r", &[("timeout", "10s")]), None);
        assert_eq!(of("rest:https://demo:pw@nas/r", &[]), None);
        assert_eq!(of("/srv/nas", &[("password", "pw")]), None);
    }

    #[test]
    fn an_unchecked_certificate_is_exposed_only_when_insecure_tls_is_true() {
        let of = |value| {
            ExposedCredentials::of_location(&location(
                "rest:https://nas/r",
                &[("http-insecure-tls", value)],
            ))
        };
        assert_eq!(of("true"), Some(ExposedCredentials::UncheckedCertificate));
        assert_eq!(of("false"), None);
    }

    #[test]
    fn opendal_endpoints_and_ftp_are_exposed_with_a_credential() {
        let of = |address, options: &[(&str, &str)]| {
            ExposedCredentials::of_location(&location(address, options))
        };
        let s3 = [("endpoint", "http://s3.lan"), ("secret_access_key", "${K}")];
        assert_eq!(
            of("opendal:s3", &s3),
            Some(ExposedCredentials::PlainEndpoint)
        );
        assert_eq!(of("opendal:s3", &s3[..1]), None);
        assert_eq!(
            of(
                "opendal:s3",
                &[("endpoint", "https://s3.lan"), ("secret_access_key", "k")]
            ),
            None
        );
        assert_eq!(
            of("opendal:ftp", &[("password", "pw")]),
            Some(ExposedCredentials::PlainFtp)
        );
        assert_eq!(of("opendal:ftp", &[]), None);
        assert_eq!(
            of(
                r#"rclone::webdav,url="http://dav.lan",user=me,pass=pw:r"#,
                &[]
            ),
            Some(ExposedCredentials::PlainEndpoint)
        );
    }

    #[test]
    fn an_entry_counts_its_locations_and_warns_by_position() {
        let mut entry = RepositoryConfig {
            settings: location("/srv/one", &[]).with_password(SecretRef::Env("PW".to_owned())),
            locations: Locations::default(),
            id: None,
            maintenance: super::super::Maintenance::None,
            group_by: super::super::GroupBy::default(),
            hooks: super::super::Hooks::default(),
        };
        assert_eq!(entry.location_count(), 1);
        assert!(!entry.has_several_locations());
        assert_eq!(entry.location_warnings(), Vec::new());
        entry
            .locations
            .further
            .push(location("rest:http://nas/r", &[("password", "${PW2}")]));
        assert_eq!(entry.location_count(), 2);
        assert!(entry.has_several_locations());
        let settings = entry.location_settings();
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[1].repository, "rest:http://nas/r");
        assert_eq!(settings[1].password, SecretRef::Env("PW".to_owned()));
        assert!(settings[0].options.is_empty(), "no option is inherited");
        assert_eq!(
            entry.location_warnings(),
            vec![LocationWarning {
                position: 2,
                cause: ExposedCredentials::PlainHttp
            }]
        );
    }

    fn entry(first: &str, further: &[&str], id: Option<&str>) -> RepositoryConfig {
        RepositoryConfig {
            settings: location(first, &[]).with_password(SecretRef::Env("PW".to_owned())),
            locations: Locations {
                first_when: None,
                further: further
                    .iter()
                    .map(|address| location(address, &[]))
                    .collect(),
            },
            id: id.map(|hex| crate::RepositoryId::parse(&hex.repeat(32)).unwrap()),
            maintenance: super::super::Maintenance::None,
            group_by: super::super::GroupBy::default(),
            hooks: super::super::Hooks::default(),
        }
    }

    fn ruled(kinds: &[NetworkKind]) -> Option<LocationRule> {
        Some(LocationRule::new(kinds.iter().copied().map(Into::into).collect()).unwrap())
    }

    /// A plan "home" backing up to the entry, with a schedule when
    /// `scheduled` and a retry when `retries`.
    fn plan(scheduled: bool, retries: bool) -> Plan {
        let mut plan = Plan::new(
            PlanId::parse("home").unwrap(),
            crate::RepositoryAlias::parse("nas").unwrap(),
            Vec::new(),
        );
        plan.schedule = scheduled
            .then(|| super::super::Schedule::new(super::super::Interval::parse("1d").unwrap()));
        plan.retry = retries.then(|| {
            super::super::RetryPolicy::new(3, jiff::SignedDuration::from_mins(10)).unwrap()
        });
        plan
    }

    #[test]
    fn an_entry_whose_locations_have_no_rule_has_no_notes() {
        let entry = entry("/srv/lan", &["/srv/public"], Some("aa"));
        assert_eq!(
            entry.rule_notes(&[&plan(true, false)], false, true),
            Vec::new()
        );
    }

    #[test]
    fn every_location_ruled_is_noted_and_one_unruled_location_is_enough_to_drop_it() {
        let mut entry = entry("/srv/lan", &["/srv/public"], Some("aa"));
        entry.locations.first_when = ruled(&[NetworkKind::Wired]);
        assert_eq!(
            entry.rule_notes(&[&plan(true, true)], true, true),
            Vec::new()
        );
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi]);
        assert_eq!(
            entry.rule_notes(&[&plan(true, true)], true, true),
            vec![RuleNote::EveryLocationRuled]
        );
    }

    #[test]
    fn a_ruled_last_location_is_noted_for_each_scheduled_plan_that_doesnt_retry() {
        let mut entry = entry("/srv/lan", &["/srv/public"], Some("aa"));
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi]);
        let home = PlanId::parse("home").unwrap();
        assert_eq!(
            entry.rule_notes(&[&plan(true, false)], true, true),
            vec![RuleNote::LastRuledWithoutRetry { plan: home }]
        );
        // A plan that retries, or that runs only when asked, isn't noted.
        assert_eq!(
            entry.rule_notes(&[&plan(true, true)], true, true),
            Vec::new()
        );
        assert_eq!(
            entry.rule_notes(&[&plan(false, false)], true, true),
            Vec::new()
        );
        // A rule on an earlier location only isn't noted either.
        entry.locations.further[0].when = None;
        entry.locations.first_when = ruled(&[NetworkKind::WiFi]);
        assert_eq!(
            entry.rule_notes(&[&plan(true, false)], true, true),
            Vec::new()
        );
    }

    #[test]
    fn a_wired_alternative_is_noted_by_position_where_the_machine_cant_tell_wired() {
        let mut entry = entry("/srv/lan", &["/srv/public", "/srv/far"], Some("aa"));
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi, NetworkKind::Wired]);
        entry.locations.first_when = ruled(&[NetworkKind::WiFi]);
        assert_eq!(
            entry.rule_notes(&[], false, true),
            vec![RuleNote::WiredUnknown { position: 2 }]
        );
        assert_eq!(entry.rule_notes(&[], true, true), Vec::new());
    }

    #[test]
    fn a_rule_with_names_is_noted_by_position_where_the_machine_cant_read_the_name() {
        let mut entry = entry("/srv/lan", &["/srv/public", "/srv/far"], Some("aa"));
        let office = crate::planning::NetworkName::parse("Office").unwrap();
        let named = crate::planning::Alternative::new(
            NetworkKind::WiFi,
            Some(crate::planning::NameFilter::Only(vec![office])),
        )
        .unwrap();
        entry.locations.first_when = ruled(&[NetworkKind::WiFi]);
        entry.locations.further[0].when =
            Some(LocationRule::new(vec![NetworkKind::Wired.into(), named]).unwrap());
        assert_eq!(
            entry.rule_notes(&[], true, false),
            vec![RuleNote::NameUnreadable { position: 2 }]
        );
        assert_eq!(entry.rule_notes(&[], true, true), Vec::new());
    }

    #[test]
    fn a_further_location_at_an_earlier_ones_place_is_named_by_its_index() {
        assert_eq!(
            entry("/srv/a", &["/srv/b", "/srv/c"], None).repeated_location(),
            None
        );
        assert_eq!(
            entry("/srv/a", &["/srv/a"], None).repeated_location(),
            Some(0)
        );
        assert_eq!(
            entry("/srv/a", &["/srv/b", "/srv/c", "/srv/b"], None).repeated_location(),
            Some(2)
        );
        // Credentials don't make a place: the same address with another
        // password is the same location.
        let mut moved = entry("rest:https://nas/r", &[], None);
        moved
            .locations
            .further
            .push(location("rest:https://nas/r", &[("password", "${PW2}")]));
        assert_eq!(moved.repeated_location(), Some(0));
    }

    #[test]
    fn two_entries_with_no_known_id_are_one_repository_at_any_shared_location() {
        let laptop = entry("/srv/lan", &["/srv/public"], None);
        assert!(laptop.is_entry(&entry("/srv/public", &[], None)));
        assert!(entry("/srv/other", &["/srv/lan"], None).is_entry(&laptop));
        assert!(!laptop.is_entry(&entry("/srv/other", &["/srv/elsewhere"], None)));
        assert!(laptop.is(
            &location("/srv/public", &[]).with_password(SecretRef::Env("PW".to_owned())),
            None
        ));
        // When both know their ID, the ID decides alone.
        let known = entry("/srv/lan", &["/srv/public"], Some("aa"));
        assert!(!known.is_entry(&entry("/srv/public", &[], Some("bb"))));
        assert!(known.is_entry(&entry("/srv/elsewhere", &[], Some("aa"))));
        assert!(known.is_entry(&entry("/srv/public", &[], None)));
    }

    #[test]
    fn at_most_four_locations_are_held() {
        assert_eq!(check_location_count(3), Ok(()));
        assert_eq!(
            check_location_count(4).map_err(|error| error.kind),
            Err(ValueKind::Location)
        );
    }
}
