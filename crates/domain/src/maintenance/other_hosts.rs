//! Other hosts: the warning a repository maintained here gives when other
//! machines write to it (technical design §7.3). Without a hub, an agent
//! whose repository entry asks for `local` maintenance runs forget, prune
//! and adoption itself, and nothing can tell it that another machine does
//! the same (I1). What it can see is who else writes to the repository: a
//! snapshot whose host is not one this agent records came from another
//! machine, and that machine may maintain the repository too.

use super::Snapshot;
use crate::execution::{Warning, WarningKind};
use crate::planning::AgentConfig;
use crate::{AgentId, RepositoryAlias};

/// How many other hosts a warning names: the rest are only counted, so a
/// repository written by many machines can't make the warning, or what is
/// kept of it, endless (§7.3).
pub const MAX_NAMED_OTHER_HOSTS: usize = 5;

/// The hosts, other than this one, that a repository maintained here holds
/// snapshots from: how many, and the first [`MAX_NAMED_OTHER_HOSTS`] of them
/// by name, in order and each once. Empty for a repository this agent isn't
/// asked to maintain, whoever writes to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OtherHosts {
    named: Vec<String>,
    count: usize,
}

impl OtherHosts {
    /// The other hosts of the repository `alias` of `config`, among
    /// `snapshots`, for the agent `agent` on the machine called `machine`.
    /// A snapshot is this host's when this agent tagged it, whatever host it
    /// records, as one made before the machine was renamed; else when its
    /// host is one this agent records: the machine's own name, the agent's
    /// `hostname`, or the `host` of any of its plans. Only a repository whose
    /// entry asks for `local` maintenance has any.
    pub fn of<'a>(
        config: &AgentConfig,
        alias: &RepositoryAlias,
        agent: &AgentId,
        machine: &str,
        snapshots: impl IntoIterator<Item = &'a Snapshot>,
    ) -> Self {
        let _ = (config, alias, agent, machine, snapshots.into_iter().count());
        todo!("the other hosts of a repository maintained here")
    }

    /// The first [`MAX_NAMED_OTHER_HOSTS`] of them, in order.
    pub fn hosts(&self) -> &[String] {
        &self.named
    }

    /// How many there are, named or not.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The warning a run of `during` (`forget`, `adopt` or `prune`) carries
    /// of them, as [`WarningKind::OtherHosts`] describes it: nothing when
    /// there are none.
    pub fn warning(&self, during: &str) -> Option<Warning> {
        let _ = (during, WarningKind::OtherHosts);
        todo!("the warning a run carries of the other hosts")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
    use crate::planning::{
        AgentSettings, GroupBy, Hooks, Hostname, Maintenance, Plan, RepositoryConfig,
        RepositorySettings, Source,
    };
    use crate::secrets::SecretRef;
    use crate::{PlanId, SnapshotId, Timestamp, Uuid};

    const MACHINE: &str = "demo-host";

    fn agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(1))
    }

    fn alias(text: &str) -> RepositoryAlias {
        RepositoryAlias::parse(text).expect("an alias")
    }

    /// A repository at `location` that asks for `maintenance`.
    fn repository(location: &str, maintenance: Maintenance) -> RepositoryConfig {
        RepositoryConfig {
            id: None,
            settings: RepositorySettings {
                repository: location.to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    /// A configuration whose repository `demo` asks for `maintenance`, beside
    /// a repository `other`, with the agent's `hostname` and one plan of each
    /// repository whose `host` is as given.
    fn config(
        maintenance: Maintenance,
        hostname: Option<&str>,
        plan_hosts: [Option<&str>; 2],
    ) -> AgentConfig {
        let host = |text: &str| Hostname::parse(text).expect("a hostname");
        let plan = |id: &str, repository: &str, plan_host: Option<&str>| {
            let mut plan = Plan::new(
                PlanId::parse(id).expect("a plan ID"),
                alias(repository),
                vec![Source::parse("/home/demo").expect("a source")],
            );
            plan.host = plan_host.map(host);
            plan
        };
        let [demo_host, other_host] = plan_hosts;
        AgentConfig::new(
            AgentSettings {
                hostname: hostname.map(host),
                ..AgentSettings::default()
            },
            Hooks::default(),
            BTreeMap::from([
                (alias("demo"), repository("/srv/demo-repo", maintenance)),
                (
                    alias("other"),
                    repository("/srv/other-repo", Maintenance::None),
                ),
            ]),
            vec![
                plan("home", "demo", demo_host),
                plan("photos", "other", other_host),
            ],
        )
        .expect("a valid configuration")
    }

    fn snapshot(number: u64, hostname: &str) -> Snapshot {
        Snapshot {
            id: SnapshotId::parse(&format!("{number:064x}")).expect("a snapshot ID"),
            time: Timestamp::from_second(1_800_000_000).expect("a time"),
            hostname: hostname.to_owned(),
            label: String::new(),
            tags: Vec::new(),
            paths: vec!["/home/demo".to_owned()],
        }
    }

    fn snapshots(hosts: &[&str]) -> Vec<Snapshot> {
        hosts
            .iter()
            .zip(1..)
            .map(|(host, number)| snapshot(number, host))
            .collect()
    }

    fn other_hosts(config: &AgentConfig, hosts: &[&str]) -> Vec<String> {
        OtherHosts::of(config, &alias("demo"), &agent(), MACHINE, &snapshots(hosts))
            .hosts()
            .to_vec()
    }

    #[test]
    fn a_snapshot_this_agent_tagged_is_this_hosts_whatever_host_it_records() {
        let config = config(Maintenance::Local, None, [None, None]);
        let tagged = |number, agent: &AgentId, host| Snapshot {
            tags: vec![
                format!("{PLAN_TAG_PREFIX}home"),
                format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
            ],
            ..snapshot(number, host)
        };
        let another = AgentId::from_uuid(Uuid::from_u128(2));
        let found = OtherHosts::of(
            &config,
            &alias("demo"),
            &agent(),
            MACHINE,
            &[
                // Made here before the machine was renamed.
                tagged(1, &agent(), "demo-old-name"),
                // Another agent's, on another machine.
                tagged(2, &another, "demo-nas"),
            ],
        );
        assert_eq!(found.hosts(), ["demo-nas"]);
    }

    #[test]
    fn a_repository_maintained_here_names_each_other_host_once_in_order() {
        let config = config(Maintenance::Local, None, [None, None]);
        assert_eq!(
            other_hosts(
                &config,
                &[
                    "demo-nas",
                    MACHINE,
                    "demo-desktop",
                    "demo-nas",
                    "demo-desktop"
                ]
            ),
            ["demo-desktop", "demo-nas"]
        );
    }

    #[test]
    fn a_repository_whose_snapshots_all_come_from_this_machine_has_no_other_host() {
        let config = config(Maintenance::Local, None, [None, None]);
        let found = OtherHosts::of(
            &config,
            &alias("demo"),
            &agent(),
            MACHINE,
            &snapshots(&[MACHINE]),
        );
        assert!(found.is_empty(), "{found:?}");
        assert!(other_hosts(&config, &[]).is_empty());
    }

    #[test]
    fn the_first_five_other_hosts_are_named_and_every_one_counted() {
        let config = config(Maintenance::Local, None, [None, None]);
        let five = "demo-host-1, demo-host-2, demo-host-3, demo-host-4, demo-host-5";
        for count in [MAX_NAMED_OTHER_HOSTS, MAX_NAMED_OTHER_HOSTS + 1] {
            let hosts: Vec<String> = (1..=count).map(|n| format!("demo-host-{n}")).collect();
            let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
            let found = OtherHosts::of(
                &config,
                &alias("demo"),
                &agent(),
                MACHINE,
                &snapshots(&hosts),
            );
            assert_eq!(found.hosts().join(", "), five, "{count} hosts");
            assert_eq!(found.count(), count);
            assert_eq!(
                found.warning("prune"),
                Some(Warning {
                    kind: WarningKind::OtherHosts,
                    item: None,
                    during: "prune".to_owned(),
                    detail: format!("{count}: {five}"),
                }),
            );
        }
    }

    #[test]
    fn no_other_host_is_no_warning() {
        let config = config(Maintenance::Local, None, [None, None]);
        let found = OtherHosts::of(
            &config,
            &alias("demo"),
            &agent(),
            MACHINE,
            &snapshots(&[MACHINE]),
        );
        assert_eq!(found.count(), 0);
        assert_eq!(found.warning("forget"), None);
    }

    #[test]
    fn the_agents_hostname_and_any_plans_host_are_this_hosts_names_too() {
        let config = config(
            Maintenance::Local,
            Some("demo-laptop"),
            [Some("demo-old-laptop"), Some("demo-media")],
        );
        assert_eq!(
            other_hosts(
                &config,
                &[
                    MACHINE,
                    "demo-laptop",
                    "demo-old-laptop",
                    "demo-media",
                    "demo-desktop"
                ]
            ),
            ["demo-desktop"]
        );
    }

    #[test]
    fn a_host_is_compared_exactly() {
        let config = config(Maintenance::Local, Some("demo-laptop"), [None, None]);
        assert_eq!(
            other_hosts(&config, &["Demo-Laptop", "demo-laptop.local", "demo-host "]),
            ["Demo-Laptop", "demo-host ", "demo-laptop.local"]
        );
    }

    #[test]
    fn a_repository_not_maintained_here_has_no_other_host_whoever_writes_to_it() {
        for maintenance in [Maintenance::None, Maintenance::Hub] {
            let config = config(maintenance, None, [None, None]);
            assert!(
                other_hosts(&config, &["demo-desktop"]).is_empty(),
                "{maintenance:?}"
            );
        }
        // The repository `other` asks for none, and one this configuration
        // doesn't have asks for nothing.
        let config = config(Maintenance::Local, None, [None, None]);
        for elsewhere in ["other", "ghost"] {
            let found = OtherHosts::of(
                &config,
                &alias(elsewhere),
                &agent(),
                MACHINE,
                &snapshots(&["demo-desktop"]),
            );
            assert!(found.is_empty(), "{elsewhere}: {found:?}");
        }
    }
}
