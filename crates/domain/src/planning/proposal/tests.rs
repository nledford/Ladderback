use std::collections::BTreeMap;

use super::*;
use crate::RepositoryAlias;
use crate::planning::{
    GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings, Source, Violation,
};
use crate::secrets::SecretRef;

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).unwrap()
}

fn id(text: &str) -> PlanId {
    PlanId::parse(text).unwrap()
}

fn plan(name: &str) -> Plan {
    Plan::new(
        id(name),
        alias("nas"),
        vec![Source::parse(&format!("/home/demo/{name}")).unwrap()],
    )
}

/// The repository `nas` and a plan for each of `plans`.
fn config(plans: &[&str]) -> AgentConfig {
    let entry = RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: RepositorySettings {
            repository: "/srv/nas".to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    AgentConfig::new(
        Default::default(),
        Hooks::default(),
        BTreeMap::from([(alias("nas"), entry)]),
        plans.iter().map(|name| plan(name)).collect(),
    )
    .unwrap()
}

fn ids(config: &AgentConfig) -> Vec<&str> {
    config.plans().iter().map(|plan| plan.id.as_str()).collect()
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn proposal_id() -> ProposalId {
    ProposalId::from_bytes(&[7; TOKEN_BYTES])
}

fn drafted(changes: Vec<ConfigChange>, config: &AgentConfig) -> Proposal {
    Proposal::draft(
        proposal_id(),
        Proposer::NoToken,
        ConfigRevision::new("base".to_owned()),
        changes,
        Rationale::default(),
        config,
        at(1_800_000_000),
    )
    .unwrap()
}

#[test]
fn each_change_is_made_by_the_rule_of_its_kind() {
    let before = config(&["home", "docs"]);
    let added = before.apply(&ConfigChange::AddPlan(plan("media"))).unwrap();
    assert_eq!(added, before.add_plan(plan("media")).unwrap());

    let mut changed = plan("docs");
    changed.label = Some("Documents".to_owned());
    let after = before
        .apply(&ConfigChange::ChangePlan(changed.clone()))
        .unwrap();
    assert_eq!(after.plan(&id("docs")), Some(&changed));

    let removed = before.apply(&ConfigChange::RemovePlan(id("home"))).unwrap();
    assert_eq!(ids(&removed), ["docs"]);

    assert_eq!(
        before.apply(&ConfigChange::RemovePlan(id("media"))),
        Err(ChangeRefused::UnknownPlan(id("media")))
    );
}

#[test]
fn changes_are_checked_in_order_each_on_what_the_ones_before_made() {
    let before = config(&[]);
    let mut changed = plan("home");
    changed.label = Some("Home".to_owned());
    let after = Proposal::check(
        &before,
        &[
            ConfigChange::AddPlan(plan("home")),
            ConfigChange::ChangePlan(changed.clone()),
        ],
    )
    .unwrap();
    assert_eq!(after.plan(&id("home")), Some(&changed));
}

#[test]
fn the_first_change_that_does_not_apply_is_refused_by_its_index() {
    let before = config(&["home"]);
    assert_eq!(
        Proposal::check(
            &before,
            &[
                ConfigChange::AddPlan(plan("docs")),
                ConfigChange::AddPlan(plan("home")),
                ConfigChange::RemovePlan(id("media")),
            ],
        ),
        Err(ProposalRefused::Change {
            index: 1,
            refused: ChangeRefused::Violation(Violation::DuplicatePlan {
                index: 2,
                plan: id("home"),
            }),
        })
    );
}

#[test]
fn a_proposal_holds_one_change_at_least_and_fifty_at_most() {
    let before = config(&[]);
    assert_eq!(
        Proposal::check(&before, &[]),
        Err(ProposalRefused::NoChanges)
    );
    let fifty: Vec<_> = (0..MAX_CHANGES)
        .map(|n| ConfigChange::AddPlan(plan(&format!("plan-{n}"))))
        .collect();
    assert_eq!(Proposal::check(&before, &fifty).unwrap().plans().len(), 50);
    let mut fifty_one = fifty;
    fifty_one.push(ConfigChange::AddPlan(plan("one-more")));
    assert_eq!(
        Proposal::check(&before, &fifty_one),
        Err(ProposalRefused::TooManyChanges)
    );
}

/// The plan `name` with a hook that runs `program` before it.
fn hooked(name: &str, program: &str) -> Plan {
    let mut plan = plan(name);
    plan.hooks
        .run_before
        .push(crate::planning::HookEntry::parse(program).unwrap());
    plan
}

#[test]
fn a_proposal_gives_no_plan_hooks_of_its_own() {
    let before = config(&["home"])
        .apply(&ConfigChange::ChangePlan(hooked("home", "demo-dump")))
        .unwrap();
    // A changed plan may keep the hooks it has.
    let mut weekly = hooked("home", "demo-dump");
    weekly.label = Some("Weekly".to_owned());
    assert!(Proposal::check(&before, &[ConfigChange::ChangePlan(weekly)]).is_ok());
    for change in [
        ConfigChange::AddPlan(hooked("docs", "demo-dump")),
        ConfigChange::ChangePlan(hooked("home", "demo-other")),
        ConfigChange::ChangePlan(plan("home")),
    ] {
        assert_eq!(
            Proposal::check(
                &before,
                &[ConfigChange::AddPlan(plan("media")), change.clone()]
            )
            .map(|_| ()),
            Err(ProposalRefused::Hooks { index: 1 }),
            "{change:?}"
        );
    }
}

#[test]
fn a_rationale_is_at_most_4000_characters_of_any_width() {
    let longest = "é".repeat(MAX_RATIONALE_CHARS);
    assert_eq!(Rationale::parse(longest.clone()).unwrap().as_str(), longest);
    assert_eq!(
        Rationale::parse(format!("{longest}e")),
        Err(ProposalRefused::RationaleTooLong)
    );
}

#[test]
fn a_draft_is_pending_until_48_hours_after_it_was_made_and_changes_nothing() {
    let before = config(&[]);
    let proposal = drafted(vec![ConfigChange::AddPlan(plan("home"))], &before);
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert_eq!(proposal.created_at, at(1_800_000_000));
    assert_eq!(proposal.expires_at, at(1_800_000_000 + 48 * 3600));
    assert_eq!(proposal.changes, [ConfigChange::AddPlan(plan("home"))]);
    assert!(before.plans().is_empty());
}

#[test]
fn a_draft_whose_change_does_not_apply_is_refused() {
    let refused = Proposal::draft(
        proposal_id(),
        Proposer::NoToken,
        ConfigRevision::new("base".to_owned()),
        vec![ConfigChange::ChangePlan(plan("home"))],
        Rationale::default(),
        &config(&[]),
        at(1_800_000_000),
    );
    assert_eq!(
        refused,
        Err(ProposalRefused::Change {
            index: 0,
            refused: ChangeRefused::UnknownPlan(id("home")),
        })
    );
}

#[test]
fn a_pending_proposal_expires_at_its_expiry_and_not_a_second_before() {
    let before = config(&[]);
    let proposal = drafted(vec![ConfigChange::AddPlan(plan("home"))], &before);
    let expiry = proposal.expires_at.as_second();
    assert_eq!(
        proposal.status_at(at(expiry - 1), &before),
        ProposalStatus::Pending
    );
    assert_eq!(
        proposal.status_at(at(expiry), &before),
        ProposalStatus::Expired
    );
}

#[test]
fn a_pending_proposal_whose_change_no_longer_applies_is_stale() {
    let before = config(&[]);
    let proposal = drafted(vec![ConfigChange::AddPlan(plan("home"))], &before);
    let now = at(1_800_000_060);
    assert_eq!(
        proposal.status_at(now, &config(&["docs"])),
        ProposalStatus::Pending
    );
    assert_eq!(
        proposal.status_at(now, &config(&["home"])),
        ProposalStatus::Stale
    );
}

#[test]
fn an_expired_proposal_is_expired_even_when_it_no_longer_applies() {
    let before = config(&[]);
    let proposal = drafted(vec![ConfigChange::AddPlan(plan("home"))], &before);
    assert_eq!(
        proposal.status_at(proposal.expires_at, &config(&["home"])),
        ProposalStatus::Expired
    );
}

#[test]
fn a_proposal_that_is_not_pending_keeps_its_status() {
    let before = config(&[]);
    let mut proposal = drafted(vec![ConfigChange::AddPlan(plan("home"))], &before);
    for status in ProposalStatus::ALL
        .into_iter()
        .filter(|status| *status != ProposalStatus::Pending)
    {
        proposal.status = status;
        assert_eq!(
            proposal.status_at(proposal.expires_at, &config(&["home"])),
            status
        );
    }
}

#[test]
fn the_lifecycle_allows_these_steps_and_no_others() {
    use ProposalStatus::*;
    let allowed = [
        (Pending, Applying),
        (Pending, Rejected),
        (Pending, Withdrawn),
        (Pending, Expired),
        (Pending, Stale),
        (Applying, Applied),
        (Applying, Failed),
        (Applying, Pending),
        (Failed, Applying),
    ];
    for from in ProposalStatus::ALL {
        for to in ProposalStatus::ALL {
            assert_eq!(
                from.can_become(to),
                allowed.contains(&(from, to)),
                "{from:?} to {to:?}"
            );
        }
    }
}

#[test]
fn only_pending_and_applying_proposals_have_not_ended() {
    for status in ProposalStatus::ALL {
        assert_eq!(
            status.has_ended(),
            !matches!(status, ProposalStatus::Pending | ProposalStatus::Applying),
            "{status:?}"
        );
    }
}

#[test]
fn a_status_reads_back_from_its_name() {
    for status in ProposalStatus::ALL {
        assert_eq!(ProposalStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(ProposalStatus::parse("approved"), None);
}

#[test]
fn an_id_is_twelve_characters_of_base32_from_the_first_sixty_bits() {
    let id = ProposalId::from_bytes(&[0xff; TOKEN_BYTES]);
    assert_eq!(id.as_str(), "777777777777");
    let mut bytes = [0; TOKEN_BYTES];
    bytes[0] = 0b0000_1000;
    bytes[7] = 0b0001_0000;
    let id = ProposalId::from_bytes(&bytes);
    assert_eq!(id.as_str(), "baaaaaaaaaab");
    assert_eq!(ProposalId::parse(id.as_str()), Some(id));
}

#[test]
fn an_id_of_another_length_or_alphabet_is_none() {
    assert_eq!(ProposalId::parse("aaaaaaaaaaa"), None);
    assert_eq!(ProposalId::parse("aaaaaaaaaaaaa"), None);
    assert_eq!(ProposalId::parse("aaaaaaaaaaa1"), None);
    assert_eq!(ProposalId::parse("AAAAAAAAAAAA"), None);
}
