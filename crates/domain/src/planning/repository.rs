//! What an agent's config says about one repository: where it is and how to
//! open it (technical design §10.2), who is asked to maintain it (§7.3), how
//! snapshots without a plan are grouped, and the hooks that wrap every job on
//! it.

use std::collections::BTreeMap;
use std::fmt;

use super::{Hooks, InvalidValue, ValueKind};
use crate::secrets::{REDACTED, Redactor, RepositorySecrets, SecretRef, SecretValue};

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
    /// Who is asked to maintain it.
    pub maintenance: Maintenance,
    pub group_by: GroupBy,
    /// Wraps every job on the repository, forget, prune and check included.
    pub hooks: Hooks,
}

#[cfg(test)]
mod tests {
    use super::*;

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
