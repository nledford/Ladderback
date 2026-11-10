//! Steps for the `@e2e` scenario of `features/maintenance/adopt.feature`:
//! the adoption runs against the real worker and a real scratch repository,
//! and the repository is inspected with `rustic_core` itself, never through
//! Ladderback.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use config_adapter::FileConfigStore;
use cucumber::{given, then, when};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, SnapshotId, Uuid};

use app::maintenance::{Adoption, AdoptionSource, MaintenanceKind, RunMaintenance};
use app::ports::ConfigStore;
use secrets::SystemSecrets;

use super::Agent;
use super::forget_steps::{open, snapshots_of};

const PASSWORD: &str = "e2e-scratch-password";

/// What the e2e adopt scenario set up, and what the run did.
#[derive(Default)]
pub struct AdoptE2e {
    /// The IDs the snapshots made without Ladderback had before the run.
    originals: Vec<SnapshotId>,
}

/// The two tags a snapshot of this agent's plan carries.
fn plan_tags(agent: &Agent, plan: &str) -> [String; 2] {
    [
        format!("ladderback-agent:{}", agent.agent_id.as_uuid().hyphenated()),
        format!("ladderback-plan:{plan}"),
    ]
}

/// Backs the scenario's folder up with plain rustic, carrying `tags`.
fn back_up_with_tags(agent: &Agent, tags: &[String]) -> rustic_core::repofile::SnapshotFile {
    let repository = open(agent);
    let source = agent.backup.source.clone().expect("a plan source");
    let paths = rustic_core::PathList::from_iter([source])
        .sanitize()
        .expect("the folder's path");
    let mut options = rustic_core::SnapshotOptions::default();
    for tag in tags {
        options = options.add_tags(tag).expect("a tag");
    }
    let snapshot = options.to_snapshot().expect("a snapshot");
    repository
        .backup(&rustic_core::BackupOptions::default(), &paths, snapshot)
        .expect("a plain rustic backup")
}

#[given(expr = "the plan {string} already holds a snapshot of the folder")]
fn plan_holds_snapshot(agent: &mut Agent, plan: String) {
    let tags = plan_tags(agent, &plan);
    back_up_with_tags(agent, &tags);
}

#[given(expr = "the folder was backed up without Ladderback with the tag {string}")]
fn backed_up_with_tag(agent: &mut Agent, tag: String) {
    back_up_with_tags(agent, &[tag]);
}

#[given(expr = "an earlier adoption into the plan {string} saved its copy but left the original")]
fn interrupted_adoption(agent: &mut Agent, plan: String) {
    // What a worker killed between saving the copies and removing the
    // originals leaves: each original, and a copy of it with the plan's tags.
    let repository = open(agent);
    let mut copies = repository.get_all_snapshots().expect("the snapshots load");
    let tags: rustic_core::repofile::StringList = plan_tags(agent, &plan)
        .join(",")
        .parse()
        .expect("the plan's tags");
    for copy in &mut copies {
        copy.tags.add_list(tags.clone());
    }
    repository
        .save_snapshots(copies)
        .expect("the copies are saved");
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// Saves the configuration the adoption loads: the scenario's repository
/// under `demo`, and the destination plan with no host of its own, so it
/// matches the snapshots plain rustic made on this machine.
async fn save_config(agent: &mut Agent) {
    let cold = agent.backup.repository.clone().expect("a repository");
    let source = agent.backup.source.clone().expect("a plan source");
    let plan = Plan::new(
        agent.backup.plan.clone().expect("a plan"),
        alias(),
        vec![
            domain::planning::Source::parse(source.to_str().expect("a UTF-8 path"))
                .expect("a source"),
        ],
    );
    let mut repositories = BTreeMap::new();
    repositories.insert(
        alias(),
        RepositoryConfig {
            settings: RepositorySettings {
                repository: cold.to_str().expect("a UTF-8 path").to_owned(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new(PASSWORD)),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::default(),
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        },
    );
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        repositories,
        vec![plan],
    )
    .expect("the scenario's configuration is valid");
    fs::create_dir_all(agent.scratch().join("config")).expect("the config folder");
    FileConfigStore::new(
        agent.scratch().join("config").join("config.toml"),
        Arc::new(platform::SystemClock),
    )
    .save(&config)
    .await
    .expect("the configuration saves");
}

#[when(expr = "the owner adopts the unmanaged history that matches the plan {string}")]
async fn adopt(agent: &mut Agent, plan: String) {
    save_config(agent).await;
    let config = FileConfigStore::new(
        agent.scratch().join("config").join("config.toml"),
        Arc::new(platform::SystemClock),
    );
    let resolve = app::secrets::ResolveRepository::new(Arc::new(SystemSecrets::new()));
    let use_case = RunMaintenance::new(
        agent.engine.clone(),
        Arc::new(platform::SystemClock),
        agent.store().await,
        resolve,
        Arc::new(engine::RusticRetention),
        Arc::new(config),
        agent.agent_id,
        platform::hostname(),
        Arc::new(engine::RusticSources),
    );
    agent.runs += 1;
    let operation = OperationId::from_uuid(Uuid::from_u128(agent.runs));
    let kind = MaintenanceKind::Adopt(Adoption {
        from: AdoptionSource::Unmanaged,
        into: domain::PlanId::try_from(plan).expect("a plan ID"),
    });
    let prepared = use_case
        .prepare(operation, &alias(), kind)
        .await
        .expect("the history matches the plan");
    // What the run replaces, read before anything is written.
    agent.adopt.originals = snapshots_of(agent)
        .into_iter()
        .filter(|(_, _, tags)| !tags.iter().any(|tag| tag.starts_with("ladderback-")))
        .map(|(id, _, _)| id)
        .collect();
    assert!(
        !agent.adopt.originals.is_empty(),
        "there is history to adopt"
    );
    agent.finished = Some(use_case.execute(prepared, futures::future::pending()).await);
}

#[then("every snapshot of the folder carries this agent's and the plan's tags")]
fn tagged(agent: &mut Agent) {
    let plan = agent.backup.plan.clone().expect("a plan");
    let plan_tag = format!("ladderback-plan:{}", plan.as_str());
    let agent_tag = format!("ladderback-agent:{}", agent.agent_id.as_uuid().hyphenated());
    let after = snapshots_of(agent);
    assert_eq!(
        after.len(),
        agent.adopt.originals.len(),
        "only the adopted history is there: {:?}",
        after.iter().map(|(id, _, _)| id).collect::<Vec<_>>()
    );
    for (id, _, tags) in &after {
        assert!(
            tags.iter().any(|tag| tag == &plan_tag) && tags.iter().any(|tag| tag == &agent_tag),
            "the snapshot {id:?} carries both tags: {tags:?}"
        );
    }
}

#[then("no snapshot made without Ladderback is left")]
fn originals_gone(agent: &mut Agent) {
    let present: Vec<SnapshotId> = snapshots_of(agent)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    for original in &agent.adopt.originals {
        assert!(
            !present.contains(original),
            "the original {:?} is still there: {present:?}",
            original
        );
    }
}

#[then(expr = "the adoption reports {int} snapshot(s) rewritten")]
fn reports_rewritten(agent: &mut Agent, count: usize) {
    let finished = agent.finished.as_ref().expect("the adoption has run");
    let Some(app::ports::EngineResult::Rewrite(summary)) = &finished.result else {
        panic!("a rewrite's result, not {:?}", finished.result)
    };
    assert_eq!(summary.created.len(), count, "{:?}", summary.created);
    let present: Vec<SnapshotId> = snapshots_of(agent)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    for created in &summary.created {
        assert!(
            present.contains(created),
            "{created:?} is in the repository"
        );
    }
}

#[then(expr = "the adopted snapshot still carries the tag {string}")]
fn keeps_user_tag(agent: &mut Agent, tag: String) {
    let finished = agent.finished.as_ref().expect("the adoption has run");
    let Some(app::ports::EngineResult::Rewrite(summary)) = &finished.result else {
        panic!("a rewrite's result, not {:?}", finished.result)
    };
    let plan = agent.backup.plan.clone().expect("a plan");
    let expected = plan_tags(agent, plan.as_str());
    for (id, _, tags) in snapshots_of(agent) {
        if summary.created.contains(&id) {
            assert!(tags.contains(&tag), "{id:?} kept {tag:?}: {tags:?}");
            for plan_tag in &expected {
                assert!(
                    tags.contains(plan_tag),
                    "{id:?} carries {plan_tag}: {tags:?}"
                );
            }
        }
    }
}
