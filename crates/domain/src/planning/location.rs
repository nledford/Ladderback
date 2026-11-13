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
    InvalidValue, LocationRule, Plan, RepositoryConfig, RepositorySettings, RuleVerdict, ValueKind,
    masked_options,
};
use crate::PlanId;
use crate::secrets::{
    REDACTED, Redactor, RepositorySecrets, SecretRef, SecretValue, is_secret_option,
    location_secret_written, url_has_userinfo,
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
            .any(|address| url_has_userinfo(address) || location_secret_written(address))
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

impl Location {
    /// Whether every address of this location makes whoever answers prove
    /// who it is before it is sent a credential, or sends none (decision
    /// NN): an absolute local path, or `rest:https://` whose certificate is
    /// checked. Only such a location is probed before its turn.
    pub fn checks_server_first(&self) -> bool {
        let accepts_any_certificate = self.accepts_any_certificate();
        std::iter::once(self.repository.as_str())
            .chain(self.repo_hot.as_deref())
            .all(|address| {
                address_checks_server_first(address)
                    && !(accepts_any_certificate
                        && starts_with_ignore_case(address, "rest:https://"))
            })
    }

    /// Whether any option set turns the certificate check off: the cold
    /// backend reads `options` and `options-cold`, the hot one `options` and
    /// `options-hot`, so one set saying `true` is enough.
    fn accepts_any_certificate(&self) -> bool {
        [&self.options, &self.options_hot, &self.options_cold]
            .into_iter()
            .flatten()
            .any(|(key, value)| {
                key.eq_ignore_ascii_case("http-insecure-tls")
                    && value.trim().eq_ignore_ascii_case("true")
            })
    }
}

/// Whether one address of a location makes the server prove who it is
/// before it is sent a credential, or sends none: `local:` and an absolute
/// path, an absolute path no `:` gives a scheme (rustic reads the text
/// before the first `:` as one), or `rest:` and an `https://` URL. Anything
/// else, a reference that may hold any scheme included, doesn't.
fn address_checks_server_first(address: &str) -> bool {
    if let Some(path) = address.strip_prefix("local:") {
        return path.starts_with('/');
    }
    if let Some(url) = address.strip_prefix("rest:") {
        return starts_with_ignore_case(url, "https://");
    }
    address.starts_with('/') && !address.contains(':')
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
    /// Whether the location has a rule. Every rule reads the network link,
    /// which a network made to look like the person's own passes too (a
    /// travel router's wired link, a copied Wi-Fi name), so a rule keeps
    /// the credentials from no impostor (decision 80, point 7).
    pub ruled: bool,
}

impl LocationWarning {
    /// The warning's `rule` argument: `network` for a location with a rule,
    /// `none` for one without.
    pub fn rule_code(self) -> &'static str {
        if self.ruled { "network" } else { "none" }
    }
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

    /// Which of the entry's locations, in order, the first included, a
    /// choice probes at its start rather than in its turn (decision NN),
    /// given each location's rule's verdict, in the same order: those whose
    /// rule holds (a location with none holds) and that check the server
    /// first. A location with no verdict doesn't hold.
    pub fn early_probes(&self, verdicts: &[RuleVerdict]) -> Vec<bool> {
        let first = Location::of(&self.settings);
        std::iter::once(&first)
            .chain(&self.locations.further)
            .zip(
                verdicts
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(RuleVerdict::Unknown)),
            )
            .map(|(location, verdict)| {
                verdict == RuleVerdict::Holds && location.checks_server_first()
            })
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
    /// answers at their address, as written, each saying whether it has a
    /// rule: none for an entry of one location and no rule, which is
    /// unchanged in every way.
    pub fn location_warnings(&self) -> Vec<LocationWarning> {
        if !self.chooses_location() {
            return Vec::new();
        }
        std::iter::once(Location::of(&self.settings))
            .chain(self.locations.further.iter().cloned())
            .zip(self.location_rules())
            .zip(1u8..)
            .filter_map(|((location, rule), position)| {
                ExposedCredentials::of_location(&location).map(|cause| LocationWarning {
                    position,
                    cause,
                    ruled: rule.is_some(),
                })
            })
            .collect()
    }

    /// The notes about the entry's locations' rules, given `plans`, the
    /// plans that back up to it, and whether this machine can read the
    /// Wi-Fi network's name: none for an entry whose locations have no rule.
    pub fn rule_notes(&self, plans: &[&Plan], reads_names: bool) -> Vec<RuleNote> {
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
    use crate::planning::NetworkKind;

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
    fn a_userinfo_a_file_reference_gives_is_exposed_over_plain_http() {
        let of = |address: &str| ExposedCredentials::of_location(&location(address, &[]));
        // The reference's path holds `/`, and may hold `?`, `#` or `@`: none
        // of them ends the authority or the userinfo.
        for userinfo in [
            "${file:/home/user/.config/secrets/nas-userinfo}",
            "${file:/home/user/nas?userinfo#1}",
            "${file:/home/user/nas@userinfo}",
            "demo:${file:/home/user/.config/secrets/nas-password}",
        ] {
            assert_eq!(
                of(&format!("rest:http://{userinfo}@nas.example:8000")),
                Some(ExposedCredentials::PlainHttp),
                "{userinfo}"
            );
            assert_eq!(
                of(&format!("rest:https://{userinfo}@nas.example:8000")),
                None,
                "{userinfo}"
            );
        }
        // An `@` after the authority's end is in the path, a `\` ending it as
        // `/` does in an `http` URL; an `@` inside a reference ends nothing.
        for address in [
            "rest:http://nas.example/a@b",
            "rest:http://nas.example?a@b",
            "rest:http://nas.example#a@b",
            "rest:http://nas.example\\a@b",
            "rest:http://nas.example:8000/${file:/home/user/a@b}",
        ] {
            assert_eq!(of(address), None, "{address}");
        }
        // A `${` never closed is no reference, so it is text as written.
        assert_eq!(
            of("rest:http://${file:/home/user/nas-userinfo@nas.example:8000"),
            None
        );
        assert_eq!(
            of("rest:http://demo:${file@nas.example:8000/r"),
            Some(ExposedCredentials::PlainHttp)
        );
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
                cause: ExposedCredentials::PlainHttp,
                ruled: false,
            }]
        );
    }

    /// A repository of one location and no rule is unchanged in every way
    /// (decision 80, point 2): its plain-HTTP credentials are no location's
    /// warning.
    #[test]
    fn an_entry_of_one_location_is_warned_about_no_location() {
        let lone = location("rest:http://nas/r", &[("password", "${PW2}")]);
        assert!(
            ExposedCredentials::of_location(&lone).is_some(),
            "the location alone would be warned about"
        );
        let entry = RepositoryConfig {
            settings: lone.with_password(SecretRef::Env("PW".to_owned())),
            locations: Locations::default(),
            id: None,
            maintenance: super::super::Maintenance::None,
            group_by: super::super::GroupBy::default(),
            hooks: super::super::Hooks::default(),
        };
        assert_eq!(entry.location_warnings(), Vec::new());
    }

    /// A rule reads only the network link, which an impostor's network
    /// passes too, so the warning says which location has one: a location
    /// guarded only by a rule is as exposed as one tried on every network.
    #[test]
    fn a_warning_says_whether_its_location_has_a_rule() {
        let mut entry = entry("rest:http://demo:pw@lan.example.lan/r", &[], Some("aa"));
        entry.locations.further.push(location(
            "rest:http://nas.home.arpa/r",
            &[("password", "${PW2}")],
        ));
        entry.locations.first_when = ruled(&[NetworkKind::Wired]);
        assert_eq!(
            entry.location_warnings(),
            vec![
                LocationWarning {
                    position: 1,
                    cause: ExposedCredentials::PlainHttp,
                    ruled: true,
                },
                LocationWarning {
                    position: 2,
                    cause: ExposedCredentials::PlainHttp,
                    ruled: false,
                },
            ]
        );
        entry.locations.first_when = None;
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi]);
        let rules: Vec<bool> = entry
            .location_warnings()
            .into_iter()
            .map(|warning| warning.ruled)
            .collect();
        assert_eq!(rules, [false, true]);
    }

    /// A `when` on a repository of one location makes it a location chosen
    /// by its rule (decision 80, point 2), so its plain-HTTP credentials are
    /// warned about as any ruled location's are.
    #[test]
    fn an_entry_of_one_location_with_a_rule_is_warned_about() {
        let mut entry = entry("rest:http://demo:pw@lan.example.lan/r", &[], None);
        assert_eq!(entry.location_warnings(), Vec::new());
        entry.locations.first_when = ruled(&[NetworkKind::Wired]);
        assert_eq!(
            entry.location_warnings(),
            vec![LocationWarning {
                position: 1,
                cause: ExposedCredentials::PlainHttp,
                ruled: true,
            }]
        );
    }

    #[test]
    fn a_warnings_rule_argument_says_network_or_none() {
        let warning = |ruled| LocationWarning {
            position: 1,
            cause: ExposedCredentials::PlainHttp,
            ruled,
        };
        assert_eq!(warning(true).rule_code(), "network");
        assert_eq!(warning(false).rule_code(), "none");
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
        assert_eq!(entry.rule_notes(&[&plan(true, false)], false), Vec::new());
    }

    #[test]
    fn every_location_ruled_is_noted_and_one_unruled_location_is_enough_to_drop_it() {
        let mut entry = entry("/srv/lan", &["/srv/public"], Some("aa"));
        entry.locations.first_when = ruled(&[NetworkKind::Wired]);
        assert_eq!(entry.rule_notes(&[&plan(true, true)], true), Vec::new());
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi]);
        assert_eq!(
            entry.rule_notes(&[&plan(true, true)], true),
            vec![RuleNote::EveryLocationRuled]
        );
    }

    #[test]
    fn a_ruled_last_location_is_noted_for_each_scheduled_plan_that_doesnt_retry() {
        let mut entry = entry("/srv/lan", &["/srv/public"], Some("aa"));
        entry.locations.further[0].when = ruled(&[NetworkKind::WiFi]);
        let home = PlanId::parse("home").unwrap();
        assert_eq!(
            entry.rule_notes(&[&plan(true, false)], true),
            vec![RuleNote::LastRuledWithoutRetry { plan: home }]
        );
        // A plan that retries, or that runs only when asked, isn't noted.
        assert_eq!(entry.rule_notes(&[&plan(true, true)], true), Vec::new());
        assert_eq!(entry.rule_notes(&[&plan(false, false)], true), Vec::new());
        // A rule on an earlier location only isn't noted either.
        entry.locations.further[0].when = None;
        entry.locations.first_when = ruled(&[NetworkKind::WiFi]);
        assert_eq!(entry.rule_notes(&[&plan(true, false)], true), Vec::new());
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
            entry.rule_notes(&[], false),
            vec![RuleNote::NameUnreadable { position: 2 }]
        );
        assert_eq!(entry.rule_notes(&[], true), Vec::new());
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

    #[test]
    fn an_address_is_kept_as_written_with_its_credentials_masked() {
        let plain = location("rest:https://nas/r", &[]);
        assert_eq!(plain.masked_address(), "rest:https://nas/r");
        assert_eq!(plain.masked_repo_hot(), None);
        let written = Location {
            repo_hot: Some("rest:http://demo:CANARY@hot.lan/r".to_owned()),
            ..location("rest:http://demo:CANARY@nas.lan/r", &[])
        };
        let address = written.masked_address();
        assert!(!address.contains("CANARY"), "{address}");
        assert!(address.ends_with("@nas.lan/r"), "{address}");
        let hot = written.masked_repo_hot().unwrap_or_default();
        assert!(!hot.contains("CANARY"), "{hot}");
        assert!(hot.ends_with("@hot.lan/r"), "{hot}");
        let hot_as_written = Location {
            repo_hot: Some("/srv/hot".to_owned()),
            ..plain
        };
        assert_eq!(
            hot_as_written.masked_repo_hot().as_deref(),
            Some("/srv/hot")
        );
    }

    #[test]
    fn a_userinfo_a_reference_gives_is_masked_whole() {
        let masked = |address: &str| location(address, &[]).masked_address();
        for userinfo in [
            "${file:/home/user/.config/secrets/nas-userinfo}",
            "${file:/home/user/nas?userinfo#1}",
            "${file:/home/user/nas@userinfo}",
            "${NAS_USERINFO}",
        ] {
            assert_eq!(
                masked(&format!("rest:http://{userinfo}@nas.example:8000")),
                "rest:http://***@nas.example:8000",
                "{userinfo}"
            );
        }
        // A password from a reference is masked as a written one is.
        assert_eq!(
            masked("rest:http://demo:${file:/home/user/nas-password}@nas.example:8000/r"),
            "rest:http://demo:***@nas.example:8000/r"
        );
        assert_eq!(
            masked("rest:http://nas.example/a@b"),
            "rest:http://nas.example/a@b"
        );
    }

    #[test]
    fn each_cause_of_exposed_credentials_has_its_wire_code() {
        for (cause, code) in [
            (ExposedCredentials::PlainHttp, "plain-http"),
            (
                ExposedCredentials::UncheckedCertificate,
                "unchecked-certificate",
            ),
            (ExposedCredentials::PlainEndpoint, "plain-endpoint"),
            (ExposedCredentials::PlainFtp, "plain-ftp"),
        ] {
            assert_eq!(cause.code(), code);
        }
    }

    #[test]
    fn settings_are_exposed_as_the_location_they_describe() {
        let settings = location("rest:http://demo:pw@nas/r", &[])
            .with_password(SecretRef::Env("PW".to_owned()));
        assert_eq!(
            ExposedCredentials::of(&settings),
            Some(ExposedCredentials::PlainHttp)
        );
    }

    #[test]
    fn an_rclone_location_is_exposed_only_over_plain_http_with_a_credential() {
        let of = |address| ExposedCredentials::of_location(&location(address, &[]));
        assert_eq!(
            of(r#"rclone::webdav,url="https://dav.lan",user=me,pass=pw:r"#),
            None
        );
        assert_eq!(of(r#"rclone::webdav,url="http://dav.lan":r"#), None);
    }

    #[test]
    fn a_location_is_chosen_for_several_locations_or_for_a_rule() {
        let mut lone = entry("/srv/lan", &[], Some("aa"));
        assert!(!lone.chooses_location());
        assert_eq!(lone.ruled_positions(), Vec::<u8>::new());
        lone.locations.first_when = ruled(&[NetworkKind::WiFi]);
        assert!(lone.chooses_location());
        assert_eq!(lone.ruled_positions(), vec![1]);
        let mut several = entry("/srv/lan", &["/srv/public", "/srv/far"], Some("aa"));
        assert!(several.chooses_location());
        assert_eq!(several.ruled_positions(), Vec::<u8>::new());
        several.locations.further[0].when = ruled(&[NetworkKind::Wired]);
        assert_eq!(several.ruled_positions(), vec![2]);
    }

    #[test]
    fn a_local_path_or_a_checked_https_location_checks_the_server_first() {
        let checks =
            |address, options: &[(&str, &str)]| location(address, options).checks_server_first();
        assert!(checks("/srv/lan", &[]));
        assert!(checks("local:/srv/lan", &[]));
        assert!(checks("rest:https://nas.example.lan/r", &[]));
        assert!(checks("rest:HTTPS://nas.example.lan/r", &[]));
        assert!(checks("rest:https://demo:pw@nas.example.lan/r", &[]));
        assert!(checks(
            "rest:https://nas.example.lan/r",
            &[("http-cacert", "/etc/ladderback/lan-ca.pem")]
        ));
        assert!(checks(
            "rest:https://nas.example.lan/r",
            &[("http-insecure-tls", "false")]
        ));
    }

    #[test]
    fn a_location_that_could_send_a_credential_to_an_impostor_doesnt_check_the_server_first() {
        let checks =
            |address, options: &[(&str, &str)]| location(address, options).checks_server_first();
        // Plain HTTP, with credentials or without: in its turn only.
        assert!(!checks("rest:http://demo:pw@nas.example.lan/r", &[]));
        assert!(!checks("rest:http://nas.example.lan/r", &[]));
        assert!(!checks(
            "rest:http://nas.example.lan/r",
            &[("password", "${PW}")]
        ));
        // Any certificate accepted: an impostor reads the credentials.
        assert!(!checks(
            "rest:https://demo:pw@nas.example.lan/r",
            &[("http-insecure-tls", "true")]
        ));
        assert!(!checks(
            "rest:https://nas.example.lan/r",
            &[("http-insecure-tls", " TRUE ")]
        ));
        // Backends whose check depends on settings Ladderback doesn't see.
        assert!(!checks(
            "opendal:s3",
            &[("endpoint", "https://s3.example.net")]
        ));
        assert!(!checks(
            "opendal:sftp",
            &[("endpoint", "ssh://nas.example.lan")]
        ));
        assert!(!checks(
            r#"rclone::webdav,url="https://dav.example.lan":r"#,
            &[]
        ));
        // Not known as written: a reference may hold any scheme.
        assert!(!checks("${LAN_URL}", &[]));
        // Relative, or a scheme rustic refuses.
        assert!(!checks("srv/lan", &[]));
        assert!(!checks("local:srv/lan", &[]));
        assert!(!checks("/srv/lan:r", &[]));
        assert!(!checks("REST:https://nas.example.lan/r", &[]));
        assert!(!checks("rest:https:nas.example.lan/r", &[]));
        assert!(!checks("", &[]));
    }

    #[test]
    fn a_location_whose_hot_half_doesnt_check_the_server_first_doesnt_either() {
        let mut split = location("rest:https://nas.example.lan/cold", &[]);
        split.repo_hot = Some("/srv/hot".to_owned());
        assert!(split.checks_server_first());
        split.repo_hot = Some("rest:http://demo:pw@nas.example.lan/hot".to_owned());
        assert!(!split.checks_server_first());
        split.repo_hot = Some("rest:http://nas.example.lan/hot".to_owned());
        assert!(!split.checks_server_first());
        // A hot half over HTTPS that accepts any certificate, whichever set
        // says so: the hot backend reads `options` and `options-hot`.
        let mut cold_local = location("/srv/cold", &[]);
        cold_local.repo_hot = Some("rest:https://demo:pw@nas.example.lan/hot".to_owned());
        assert!(cold_local.checks_server_first());
        let insecure = || [("http-insecure-tls".to_owned(), "true".to_owned())].into();
        let mut in_options = cold_local.clone();
        in_options.options = insecure();
        assert!(!in_options.checks_server_first());
        // The option means nothing to a local path.
        in_options.repo_hot = Some("/srv/hot".to_owned());
        assert!(in_options.checks_server_first());
        let mut in_options_hot = cold_local.clone();
        in_options_hot.options_hot = insecure();
        assert!(!in_options_hot.checks_server_first());
        // A set that turns the check off wins over one that leaves it on.
        in_options_hot.options = [("http-insecure-tls".to_owned(), "false".to_owned())].into();
        assert!(!in_options_hot.checks_server_first());
        let mut cold_https = location(
            "rest:https://demo:pw@nas.example.lan/cold",
            &[("http-insecure-tls", "false")],
        );
        cold_https.options_cold = insecure();
        assert!(!cold_https.checks_server_first());
    }

    #[test]
    fn only_a_location_whose_rule_holds_and_that_checks_the_server_first_is_probed_early() {
        use RuleVerdict::{Holds, NotMet, Unknown};
        let mut several = entry(
            "rest:https://lan.example.lan/r",
            &[
                "rest:http://demo:pw@backup.example.net/r",
                "/srv/far",
                "rest:https://backup.example.net/r",
            ],
            Some("aa"),
        );
        assert_eq!(
            several.early_probes(&[Holds, Holds, Holds, Holds]),
            vec![true, false, true, true]
        );
        assert_eq!(
            several.early_probes(&[NotMet, Holds, Unknown, Holds]),
            vec![false, false, false, true]
        );
        // A missing verdict doesn't hold.
        assert_eq!(
            several.early_probes(&[Holds, Holds]),
            vec![true, false, false, false]
        );
        several
            .settings
            .options
            .insert("http-insecure-tls".to_owned(), "true".to_owned());
        assert_eq!(
            several.early_probes(&[Holds, Holds, Holds, Holds]),
            vec![false, false, true, true]
        );
    }
}
