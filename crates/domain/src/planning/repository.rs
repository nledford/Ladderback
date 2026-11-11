//! What an agent's config says about one repository: where it is and how to
//! open it (technical design §10.2), who is asked to maintain it (§7.3), how
//! snapshots without a plan are grouped, and the hooks that wrap every job on
//! it.

use std::collections::BTreeMap;
use std::fmt;

use super::{Hooks, InvalidValue, ValueKind};
use crate::RepositoryId;
use crate::secrets::{
    InlineSecret, MAX_LOCATION_BYTES, MAX_OPTION_VALUE_BYTES, MAX_OPTIONS, REDACTED, Redactor,
    RepositorySecrets, SecretRef, SecretValue, has_reference, is_secret_option,
    location_secret_written, url_password_written,
};

/// Refuses a location longer than [`MAX_LOCATION_BYTES`]. A location may
/// hold a password, so the refusal keeps none of its text.
pub fn check_location(location: &str) -> Result<(), InvalidValue> {
    let _ = location;
    todo!()
}

/// Refuses the value of an option longer than [`MAX_OPTION_VALUE_BYTES`].
/// A value may be a secret, so the refusal keeps none of its text.
pub fn check_option_value(value: &str) -> Result<(), InvalidValue> {
    let _ = (value, MAX_OPTION_VALUE_BYTES);
    todo!()
}

/// Refuses a table of options (`options`, `options-hot`, `options-cold`)
/// that holds more than [`MAX_OPTIONS`].
pub fn check_options(options: &BTreeMap<String, String>) -> Result<(), InvalidValue> {
    let _ = (options, MAX_OPTIONS);
    todo!()
}

/// A repository as the configuration describes it: its fields may hold
/// `${VAR}` and `${file:/path}` references, which are resolved when a job
/// starts. `password` says where the password is, or is the password.
#[derive(Clone, PartialEq, Eq)]
pub struct RepositorySettings {
    pub repository: String,
    /// rustic's `repo-hot`: the hot half of a hot/cold repository.
    pub repo_hot: Option<String>,
    pub password: SecretRef,
    pub options: BTreeMap<String, String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
}

impl RepositorySettings {
    /// Refuses settings that hold more than a job takes: a location longer
    /// than [`MAX_LOCATION_BYTES`], a table of options with more than
    /// [`MAX_OPTIONS`], or an option's value longer than
    /// [`MAX_OPTION_VALUE_BYTES`]. The configuration and an import refuse
    /// each at its own key, with [`check_location`], [`check_options`] and
    /// [`check_option_value`]; this is for settings that come whole, from a
    /// request.
    pub fn check_sizes(&self) -> Result<(), InvalidValue> {
        todo!()
    }

    /// Every secret these settings hold as text: a credential written in
    /// `repository` or `repo-hot` ([`location_secret_written`]), the password
    /// when it is inline, and each option whose name says it is a credential
    /// ([`is_secret_option`]) and whose value has no `${…}` reference in it,
    /// or whose value is a URL with a password written in it
    /// ([`url_password_written`]). In the order `repository`, `repo-hot`,
    /// password, `options`, `options-hot`, `options-cold`, each set by name.
    /// A secret that is empty is nothing to move.
    pub fn inline_secrets(&self) -> Vec<InlineSecret> {
        let mut found = Vec::new();
        if location_secret_written(&self.repository) {
            found.push(InlineSecret::Location);
        }
        if self
            .repo_hot
            .as_deref()
            .is_some_and(location_secret_written)
        {
            found.push(InlineSecret::LocationHot);
        }
        if matches!(&self.password, SecretRef::Inline(value) if !value.expose().is_empty()) {
            found.push(InlineSecret::Password);
        }
        let mut credentials = |set: &BTreeMap<String, String>,
                               secret: fn(String) -> InlineSecret| {
            for (name, value) in set {
                let credential =
                    is_secret_option(name) && !value.is_empty() && !has_reference(value);
                if credential || url_password_written(value) {
                    found.push(secret(name.clone()));
                }
            }
        };
        credentials(&self.options, InlineSecret::Option);
        credentials(&self.options_hot, InlineSecret::OptionHot);
        credentials(&self.options_cold, InlineSecret::OptionCold);
        found
    }

    /// The location `secret` is written in: `repository` for
    /// [`InlineSecret::Location`], `repo-hot` for
    /// [`InlineSecret::LocationHot`], and none for a secret in a setting of
    /// its own. How to move it out depends on the location
    /// ([`rclone_location`](crate::secrets::rclone_location)).
    pub fn location_of(&self, secret: &InlineSecret) -> Option<&str> {
        match secret {
            InlineSecret::Location => Some(&self.repository),
            InlineSecret::LocationHot => self.repo_hot.as_deref(),
            InlineSecret::Password
            | InlineSecret::Option(_)
            | InlineSecret::OptionHot(_)
            | InlineSecret::OptionCold(_) => None,
        }
    }

    /// Whether `other` points at the same place: the same `repository`,
    /// `repo-hot` and the three option sets, where an `opendal:` backend
    /// keeps its bucket or root. The password and the options that hold a
    /// credential ([`is_secret_option`], as the `inline-secret` warning
    /// takes them) are how to open it, not where it is: one moved from text
    /// to `${VAR}`, added or removed leaves the same location. On a drive
    /// (opendal's Dropbox, Google Drive, OneDrive and Yandex Disk) a
    /// credential is the one thing that names the account,
    /// so the sets that reach it keep theirs: `options` reach both halves,
    /// `options-hot` the `repo-hot` one and `options-cold` the other.
    pub fn same_location(&self, other: &Self) -> bool {
        let cold = is_drive(&self.repository);
        let hot = self.repo_hot.as_deref().is_some_and(is_drive);
        self.repository == other.repository
            && self.repo_hot == other.repo_hot
            && same_places(&self.options, &other.options, cold || hot)
            && same_places(&self.options_hot, &other.options_hot, hot)
            && same_places(&self.options_cold, &other.options_cold, cold)
    }
}

/// The opendal services, by opendal's own scheme name, where only a
/// credential (`access_token`, `refresh_token`) names the account: two
/// accounts can hold one `root`, so their credentials are part of the
/// location. Every other backend names its account or bucket in an option
/// that is not a credential (`bucket`, `account_name`, `endpoint`, `user`).
const DRIVES: [&str; 4] = ["dropbox", "gdrive", "onedrive", "yandex-disk"];

/// Whether a `repository` string opens one of the [`DRIVES`]: rustic reads
/// `opendal:<scheme>`, and opendal the scheme up to a `:`, without case.
fn is_drive(location: &str) -> bool {
    location.strip_prefix("opendal:").is_some_and(|path| {
        let scheme = path.split_once(':').map_or(path, |(scheme, _)| scheme);
        DRIVES
            .iter()
            .any(|drive| drive.eq_ignore_ascii_case(scheme))
    })
}

/// Whether two option sets are equal, their credentials left out unless
/// `credentials` says they name the account.
fn same_places(
    one: &BTreeMap<String, String>,
    other: &BTreeMap<String, String>,
    credentials: bool,
) -> bool {
    places(one, credentials).eq(places(other, credentials))
}

/// The options of `set` that say where the repository is, by name.
fn places(
    set: &BTreeMap<String, String>,
    credentials: bool,
) -> impl Iterator<Item = (&String, &String)> {
    set.iter()
        .filter(move |(name, _)| credentials || !is_secret_option(name))
}

/// An option written in the config can be a secret as it stands, so what the
/// options hold is masked, and so are the passwords in the repository URLs.
impl fmt::Debug for RepositorySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inline = self
            .password
            .inline()
            .cloned()
            .unwrap_or_else(|| SecretValue::new(""));
        let redactor = Redactor::for_repository(RepositorySecrets {
            repository: &self.repository,
            repo_hot: self.repo_hot.as_deref(),
            password: &inline,
            options: &self.options,
            options_hot: &self.options_hot,
            options_cold: &self.options_cold,
            resolved: &[],
        });
        f.debug_struct("RepositorySettings")
            .field("repository", &redactor.redact(&self.repository))
            .field("repo_hot", &self.repo_hot.as_ref().map(|_| REDACTED))
            .field("password", &self.password)
            .field("options", &masked_options(&self.options))
            .field("options_hot", &masked_options(&self.options_hot))
            .field("options_cold", &masked_options(&self.options_cold))
            .finish()
    }
}

/// The option names with every value hidden: the names help to diagnose.
pub fn masked_options(options: &BTreeMap<String, String>) -> BTreeMap<&str, &str> {
    options
        .keys()
        .map(|name| (name.as_str(), REDACTED))
        .collect()
}

/// Which party an agent asks to run forget, prune and check on a repository
/// (§7.3). The request is not the grant: exactly one owner holds that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Maintenance {
    /// The hub runs it.
    Hub,
    /// This agent runs it.
    Local,
    /// Nobody is asked: the repository is never pruned from here. This is
    /// the default, because a prune is the one operation that deletes data
    /// for good, and an agent that was never told to do it must not.
    #[default]
    None,
}

impl Maintenance {
    /// Reads `hub`, `local` or `none`, in lower case.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        match text {
            "hub" => Ok(Self::Hub),
            "local" => Ok(Self::Local),
            "none" => Ok(Self::None),
            _ => Err(InvalidValue::new(ValueKind::Maintenance, text)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::Local => "local",
            Self::None => "none",
        }
    }
}

/// What snapshots are grouped by when they have no plan (§7.3): rustic's
/// `group-by`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupBy {
    pub host: bool,
    pub label: bool,
    pub paths: bool,
    pub tags: bool,
}

impl Default for GroupBy {
    /// Host, label and paths: rustic's own default.
    fn default() -> Self {
        Self {
            host: true,
            label: true,
            paths: true,
            tags: false,
        }
    }
}

impl GroupBy {
    /// Reads rustic's text form: `host`, `label`, `paths` and `tags`, in any
    /// order, separated by commas. As in rustic, an empty entry is skipped,
    /// so `""` groups by nothing, and a name that is not one of the four is
    /// refused.
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let mut group_by = Self {
            host: false,
            label: false,
            paths: false,
            tags: false,
        };
        for name in text.split(',') {
            match name {
                "" => {}
                "host" => group_by.host = true,
                "label" => group_by.label = true,
                "paths" => group_by.paths = true,
                "tags" => group_by.tags = true,
                _ => return Err(InvalidValue::new(ValueKind::GroupBy, text)),
            }
        }
        Ok(group_by)
    }
}

/// The names that are set, in the order `host,label,paths,tags`, joined by
/// commas: the form [`GroupBy::parse`] reads back.
impl fmt::Display for GroupBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (self.host, "host"),
            (self.label, "label"),
            (self.paths, "paths"),
            (self.tags, "tags"),
        ];
        let mut first = true;
        for (_, name) in names.into_iter().filter(|(set, _)| *set) {
            if !first {
                f.write_str(",")?;
            }
            f.write_str(name)?;
            first = false;
        }
        Ok(())
    }
}

/// One repository, as an agent's config describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryConfig {
    /// Where it is and how to open it.
    pub settings: RepositorySettings,
    /// The rustic repository ID Ladderback read when it connected or created
    /// the repository (§7.3). None for an entry written by hand or imported
    /// from a profile: Ladderback never opened those.
    pub id: Option<RepositoryId>,
    /// Who is asked to maintain it.
    pub maintenance: Maintenance,
    pub group_by: GroupBy,
    /// Wraps every job on the repository, forget, prune and check included.
    pub hooks: Hooks,
}

impl RepositoryConfig {
    /// Whether this entry is the repository `settings` describe, whose
    /// repository ID is `id` when it is known. When both this entry and the
    /// request know their ID, the ID decides alone: one location may come
    /// to hold another repository, and one repository may be reached at two
    /// locations. Otherwise one location is one repository (§7.3), and a
    /// location is the whole of it: [`RepositorySettings::same_location`].
    pub fn is(&self, settings: &RepositorySettings, id: Option<&RepositoryId>) -> bool {
        match (self.id.as_ref(), id) {
            (Some(own), Some(id)) => own == id,
            _ => self.settings.same_location(settings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_holds_at_most_64_kib_and_its_refusal_holds_none_of_it() {
        assert_eq!(MAX_LOCATION_BYTES, 64 * 1024);
        let at_the_cap = format!(
            "rest:https://demo:{}@nas/",
            "s".repeat(MAX_LOCATION_BYTES - 23)
        );
        assert_eq!(at_the_cap.len(), MAX_LOCATION_BYTES);
        assert_eq!(check_location(&at_the_cap), Ok(()));
        let refused = Err(InvalidValue::new(ValueKind::Location, ""));
        assert_eq!(check_location(&format!("{at_the_cap}/")), refused);
        // The cap counts bytes, not characters.
        let wide = "é".repeat(MAX_LOCATION_BYTES / 2 + 1);
        assert_eq!(check_location(&wide), refused);
        assert_eq!(check_location(""), Ok(()));
    }

    #[test]
    fn an_option_value_holds_at_most_64_kib_and_its_refusal_holds_none_of_it() {
        assert_eq!(MAX_OPTION_VALUE_BYTES, 64 * 1024);
        let at_the_cap = "k".repeat(MAX_OPTION_VALUE_BYTES);
        assert_eq!(check_option_value(&at_the_cap), Ok(()));
        let refused = Err(InvalidValue::new(ValueKind::OptionValue, ""));
        assert_eq!(check_option_value(&format!("{at_the_cap}k")), refused);
        // The cap counts bytes, not characters.
        let wide = "é".repeat(MAX_OPTION_VALUE_BYTES / 2 + 1);
        assert_eq!(check_option_value(&wide), refused);
        assert_eq!(check_option_value(""), Ok(()));
    }

    fn options(count: usize) -> BTreeMap<String, String> {
        (0..count)
            .map(|n| (format!("o{n}"), "demo".to_owned()))
            .collect()
    }

    /// One option value a byte longer than a job takes.
    fn long_value(table: &mut BTreeMap<String, String>) {
        table.insert("o1".to_owned(), "v".repeat(MAX_OPTION_VALUE_BYTES + 1));
    }

    /// One option more than a table holds, when it is full.
    fn one_more(table: &mut BTreeMap<String, String>) {
        table.insert("extra".to_owned(), String::new());
    }

    /// A field's name, the change that takes it over its cap, and the kind refused.
    type OverCap = (&'static str, fn(&mut RepositorySettings), ValueKind);

    #[test]
    fn a_table_holds_at_most_64_options() {
        assert_eq!(MAX_OPTIONS, 64);
        assert_eq!(check_options(&options(0)), Ok(()));
        assert_eq!(check_options(&options(MAX_OPTIONS)), Ok(()));
        assert_eq!(
            check_options(&options(MAX_OPTIONS + 1)),
            Err(InvalidValue::new(ValueKind::Options, ""))
        );
    }

    #[test]
    fn settings_are_refused_when_any_field_holds_more_than_a_job_takes() {
        let long = |bytes: usize| "v".repeat(bytes);
        let full = || {
            let mut table = options(MAX_OPTIONS);
            table.insert("o0".to_owned(), long(MAX_OPTION_VALUE_BYTES));
            table
        };
        let at_the_caps = RepositorySettings {
            repository: long(MAX_LOCATION_BYTES),
            repo_hot: Some(long(MAX_LOCATION_BYTES)),
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: full(),
            options_hot: full(),
            options_cold: full(),
        };
        assert_eq!(at_the_caps.check_sizes(), Ok(()));
        let over = |change: fn(&mut RepositorySettings)| {
            let mut settings = at_the_caps.clone();
            change(&mut settings);
            settings.check_sizes().map_err(|refused| refused.kind)
        };
        let cases: [OverCap; 8] = [
            (
                "repository",
                |s| s.repository.push('/'),
                ValueKind::Location,
            ),
            (
                "repo-hot",
                |s| s.repo_hot = Some("v".repeat(MAX_LOCATION_BYTES + 1)),
                ValueKind::Location,
            ),
            (
                "options",
                |s| long_value(&mut s.options),
                ValueKind::OptionValue,
            ),
            (
                "options-hot",
                |s| long_value(&mut s.options_hot),
                ValueKind::OptionValue,
            ),
            (
                "options-cold",
                |s| long_value(&mut s.options_cold),
                ValueKind::OptionValue,
            ),
            ("options", |s| one_more(&mut s.options), ValueKind::Options),
            (
                "options-hot",
                |s| one_more(&mut s.options_hot),
                ValueKind::Options,
            ),
            (
                "options-cold",
                |s| one_more(&mut s.options_cold),
                ValueKind::Options,
            ),
        ];
        for (field, change, kind) in cases {
            assert_eq!(over(change), Err(kind), "{field}");
        }
    }

    #[test]
    fn a_repository_nobody_was_asked_about_gets_no_maintenance() {
        assert_eq!(Maintenance::default(), Maintenance::None);
    }

    #[test]
    fn a_maintenance_is_hub_local_or_none() {
        for (text, maintenance) in [
            ("hub", Maintenance::Hub),
            ("local", Maintenance::Local),
            ("none", Maintenance::None),
        ] {
            assert_eq!(Maintenance::parse(text), Ok(maintenance));
            assert_eq!(maintenance.as_str(), text);
        }
        for text in ["", "Hub", "LOCAL", "owner", "none ", "agent"] {
            assert_eq!(
                Maintenance::parse(text),
                Err(InvalidValue::new(ValueKind::Maintenance, text)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn snapshots_are_grouped_by_host_label_and_paths_unless_told_otherwise() {
        assert_eq!(
            GroupBy::default(),
            GroupBy {
                host: true,
                label: true,
                paths: true,
                tags: false
            }
        );
        assert_eq!(GroupBy::default().to_string(), "host,label,paths");
    }

    /// A name, with the setter of the flag it sets.
    type Name = (&'static str, fn(&mut GroupBy));

    /// The four names, with the flag each one sets.
    fn names() -> [Name; 4] {
        [
            ("host", |g| g.host = true),
            ("label", |g| g.label = true),
            ("paths", |g| g.paths = true),
            ("tags", |g| g.tags = true),
        ]
    }

    #[test]
    fn every_combination_of_names_is_read_and_written_in_one_order() {
        for mask in 0u8..16 {
            let chosen: Vec<_> = names()
                .into_iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, name)| name)
                .collect();
            let mut expected = GroupBy {
                host: false,
                label: false,
                paths: false,
                tags: false,
            };
            for (_, set) in &chosen {
                set(&mut expected);
            }
            let canonical = chosen.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(",");
            assert_eq!(GroupBy::parse(&canonical), Ok(expected), "{canonical:?}");
            assert_eq!(expected.to_string(), canonical);
            let backwards = chosen.iter().rev().map(|(n, _)| *n).collect::<Vec<_>>();
            assert_eq!(
                GroupBy::parse(&backwards.join(",")),
                Ok(expected),
                "{backwards:?}"
            );
        }
    }

    #[test]
    fn empty_entries_are_skipped_as_rustic_skips_them() {
        let host_and_tags = GroupBy {
            host: true,
            label: false,
            paths: false,
            tags: true,
        };
        for text in [",host,,tags,", "host,tags,host", "tags,,host"] {
            assert_eq!(GroupBy::parse(text), Ok(host_and_tags), "{text:?}");
        }
        assert_eq!(GroupBy::parse(",").unwrap().to_string(), "");
    }

    #[test]
    fn any_other_name_is_refused() {
        for text in [
            "hostname",
            "Host",
            " host",
            "host ",
            "host label",
            "host;label",
            "colour",
            "host,colour",
        ] {
            assert_eq!(
                GroupBy::parse(text),
                Err(InvalidValue::new(ValueKind::GroupBy, text)),
                "{text:?}"
            );
        }
    }

    fn settings() -> RepositorySettings {
        RepositorySettings {
            repository: "rest:http://demo:CANARY-url-password@nas.lan/backups".to_owned(),
            repo_hot: Some("rest:http://demo:CANARY-hot-password@nas.lan/hot".to_owned()),
            password: SecretRef::Inline(crate::secrets::SecretValue::new("CANARY-password")),
            options: BTreeMap::from([("sas_token".to_owned(), "CANARY-option".to_owned())]),
            options_hot: BTreeMap::from([("region".to_owned(), "CANARY-hot-option".to_owned())]),
            options_cold: BTreeMap::from([("bucket".to_owned(), "CANARY-cold-option".to_owned())]),
        }
    }

    #[test]
    fn the_debug_form_of_settings_shows_no_secret() {
        let shown = format!("{:?}", settings());
        assert!(!shown.contains("CANARY"), "{shown}");
        for kept in ["nas.lan/backups", "sas_token", "region", "bucket"] {
            assert!(
                shown.contains(kept),
                "{kept:?} helps to diagnose, so it stays: {shown}"
            );
        }
    }

    #[test]
    fn masked_options_keep_the_names_and_hide_every_value() {
        let options = BTreeMap::from([
            ("endpoint".to_owned(), "http://demo:CANARY@host".to_owned()),
            ("region".to_owned(), String::new()),
        ]);
        assert_eq!(
            masked_options(&options),
            BTreeMap::from([("endpoint", "***"), ("region", "***")])
        );
    }
}
