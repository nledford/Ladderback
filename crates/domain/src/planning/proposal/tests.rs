use std::collections::BTreeMap;

use super::*;
use crate::RepositoryAlias;
use crate::execution::plan_tags::UserTag;
use crate::planning::{
    GroupBy, Hooks, Hostname, KeepCount, Maintenance, RepositoryConfig, RepositorySettings, Source,
    Violation,
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
    config_over(&["nas"], plans)
}

/// A repository for each of `repositories` and a plan for each of `plans`.
fn config_over(repositories: &[&str], plans: &[&str]) -> AgentConfig {
    let entry = |name: &str| RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: RepositorySettings {
            repository: format!("/srv/{name}"),
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
        repositories
            .iter()
            .map(|name| (alias(name), entry(name)))
            .collect(),
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
        ConfigRevision::of(b"base"),
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
        ConfigRevision::of(b"base"),
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
fn an_id_shows_as_its_characters_where_a_path_or_message_names_it() {
    let id = ProposalId::parse("bcdefghij234").expect("an ID");
    assert_eq!(
        format!("/api/v1/proposals/{id}"),
        "/api/v1/proposals/bcdefghij234"
    );
}

#[test]
fn an_id_of_another_length_or_alphabet_is_none() {
    assert_eq!(ProposalId::parse("aaaaaaaaaaa"), None);
    assert_eq!(ProposalId::parse("aaaaaaaaaaaaa"), None);
    assert_eq!(ProposalId::parse("aaaaaaaaaaa1"), None);
    assert_eq!(ProposalId::parse("AAAAAAAAAAAA"), None);
}

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn a_revision_is_the_sha256_of_the_files_bytes_in_lower_case_hex() {
    assert_eq!(ConfigRevision::of(b"").as_str(), EMPTY_SHA256);
    assert_eq!(
        ConfigRevision::of(b"abc").as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn a_revision_reads_back_from_its_hex() {
    let revision = ConfigRevision::of(b"schema = 1\n");
    assert_eq!(ConfigRevision::parse(revision.as_str()), Some(revision));
    assert_eq!(
        ConfigRevision::parse(EMPTY_SHA256),
        Some(ConfigRevision::of(b""))
    );
}

#[test]
fn a_shown_revision_is_the_sha256_of_the_key_then_the_revision() {
    let key = RevisionKey::new([7; REVISION_KEY_BYTES]);
    let shown = ShownRevision::of(&key, &ConfigRevision::of(b""));
    // SHA-256 of 32 bytes of 7, then the 64 hex digits of the empty file's
    // revision.
    assert_eq!(
        shown.as_str(),
        "6b6906290822e563c1e3b3398850f5673effd51721cb81dadb4ab80ac0cce4e6"
    );
    assert_ne!(shown.as_str(), EMPTY_SHA256);
    assert_eq!(ShownRevision::parse(shown.as_str()), Some(shown));
}

#[test]
fn the_same_file_is_shown_another_revision_under_another_key() {
    let revision = ConfigRevision::of(b"schema = 1\n");
    let one = ShownRevision::of(&RevisionKey::new([1; REVISION_KEY_BYTES]), &revision);
    let two = ShownRevision::of(&RevisionKey::new([2; REVISION_KEY_BYTES]), &revision);
    assert_ne!(one, two);
    assert_ne!(one.as_str(), revision.as_str());
    assert_eq!(
        one,
        ShownRevision::of(&RevisionKey::new([1; REVISION_KEY_BYTES]), &revision)
    );
    assert_ne!(
        one,
        ShownRevision::of(
            &RevisionKey::new([1; REVISION_KEY_BYTES]),
            &ConfigRevision::of(b"schema = 2\n")
        )
    );
}

#[test]
fn a_revision_key_never_shows_its_bytes() {
    let key = RevisionKey::new([0xab; REVISION_KEY_BYTES]);
    let shown = format!("{key:?}");
    assert_eq!(shown, "RevisionKey([hidden])");
    assert!(!shown.contains("171") && !shown.contains("ab"), "{shown}");
}

#[test]
fn a_revision_of_another_length_or_alphabet_is_none() {
    for text in [
        EMPTY_SHA256.chars().skip(1).collect(),
        format!("{EMPTY_SHA256}0"),
        EMPTY_SHA256.to_uppercase(),
        EMPTY_SHA256.replacen('e', "g", 1),
        String::new(),
    ] {
        assert_eq!(ConfigRevision::parse(&text), None, "{text}");
    }
}

// What a person decides, and the limits on proposing (decision 89).

fn minutes(n: i64) -> SignedDuration {
    SignedDuration::from_mins(n)
}

#[test]
fn a_proposer_with_as_many_pending_as_it_may_is_refused_first() {
    let record = ProposerRecord {
        pending: 2,
        made: Vec::new(),
        last_rejected: Some(at(1_800_000_000)),
    };
    assert_eq!(record.admits(at(1_800_000_000), 2), Err(TooSoon::Pending));
    assert!(matches!(
        record.admits(at(1_800_000_000), 3),
        Err(TooSoon::AfterRejection { .. })
    ));
}

#[test]
fn a_proposer_waits_five_minutes_after_a_rejection() {
    let rejected = at(1_800_000_000);
    let record = ProposerRecord {
        last_rejected: Some(rejected),
        ..ProposerRecord::default()
    };
    let until = rejected.checked_add(minutes(5)).unwrap();
    let just_before = until.checked_sub(SignedDuration::from_millis(1)).unwrap();
    assert_eq!(
        record.admits(just_before, MAX_PENDING),
        Err(TooSoon::AfterRejection { until })
    );
    assert_eq!(record.admits(until, MAX_PENDING), Ok(()));
}

#[test]
fn a_proposer_makes_thirty_an_hour_and_one_more_once_the_oldest_is_an_hour_old() {
    let now = at(1_800_003_600);
    // 31 made: one more than an hour ago, which no longer counts, and 30
    // within the hour, a minute apart, the oldest 59 minutes ago.
    let mut made: Vec<Timestamp> = (0..30)
        .map(|n| now.checked_sub(minutes(59 - n)).unwrap())
        .collect();
    made.push(now.checked_sub(minutes(61)).unwrap());
    let record = ProposerRecord {
        made,
        ..ProposerRecord::default()
    };
    let until = now.checked_add(minutes(1)).unwrap();
    assert_eq!(
        record.admits(now, MAX_PENDING),
        Err(TooSoon::Hourly { until })
    );
    assert_eq!(record.admits(until, MAX_PENDING), Ok(()));
    let fewer = ProposerRecord {
        made: record.made.iter().skip(1).copied().collect(),
        ..ProposerRecord::default()
    };
    assert_eq!(fewer.admits(now, MAX_PENDING), Ok(()));
}

#[test]
fn a_rejection_gives_one_reason_per_change_each_at_most_a_thousand_characters() {
    let long = "x".repeat(MAX_REASON_CHARS);
    let rejection = Rejection::new(2, vec![long.clone(), String::new()]).unwrap();
    assert_eq!(
        rejection
            .reasons()
            .iter()
            .map(RejectReason::as_str)
            .collect::<Vec<_>>(),
        [long.as_str(), ""]
    );
    assert_eq!(
        Rejection::new(2, vec![String::new()]),
        Err(RejectionRefused::Count { changes: 2 })
    );
    assert_eq!(
        Rejection::new(2, vec![String::new(), format!("{long}é")]),
        Err(RejectionRefused::TooLong { index: 1 })
    );
    assert_eq!(RejectReason::parse(format!("{long}x")), None);
}

#[test]
fn a_rejection_a_store_kept_gives_its_reasons_in_order() {
    let reasons = ["Back up weekly instead", "", "Keep the old host"]
        .map(|text| RejectReason::parse(text.to_owned()).unwrap());
    let kept = Rejection::kept(reasons.to_vec());
    assert_eq!(kept.reasons(), reasons);
}

#[test]
fn an_approval_with_sign_in_off_is_confirmed_by_the_short_or_whole_id() {
    let id = proposal_id();
    let short = short_proposal_id(&id);
    assert_eq!(short.len(), SHORT_PROPOSAL_ID_CHARS);
    assert!(id.as_str().starts_with(short));
    assert!(confirms_proposal(
        &format!(" {} ", short.to_uppercase()),
        &id
    ));
    assert!(confirms_proposal(id.as_str(), &id));
    let other = ProposalId::from_bytes(&[9; TOKEN_BYTES]);
    assert!(!confirms_proposal(short_proposal_id(&other), &id));
    assert!(!confirms_proposal("", &id));
    assert!(!confirms_proposal(id.as_str().get(..5).unwrap(), &id));
}

#[test]
fn a_diff_hash_is_the_sha256_of_the_text() {
    let hash = DiffHash::of("+[plans.home]\n");
    assert_eq!(hash.as_str().len(), 64);
    assert_eq!(DiffHash::parse(hash.as_str()), Some(hash.clone()));
    assert_ne!(DiffHash::of("+[plans.homes]\n"), hash);
    assert_eq!(DiffHash::parse("not a hash"), None);
}

#[test]
fn the_summary_says_what_each_change_does_against_what_the_ones_before_made() {
    let config = config(&["home"]);
    let mut photos = plan("photos");
    photos.label = Some("demo".to_owned());
    let mut changed = plan("home");
    changed.label = Some("demo".to_owned());
    changed.sources = vec![Source::parse("/home/demo/other").unwrap()];
    let proposal = drafted(
        vec![
            ConfigChange::AddPlan(plan("photos")),
            ConfigChange::ChangePlan(photos),
            ConfigChange::ChangePlan(changed),
            ConfigChange::RemovePlan(id("photos")),
        ],
        &config,
    );
    assert_eq!(
        proposal.summary(&config),
        [
            SummaryLine::AddsPlan(id("photos")),
            SummaryLine::ChangesPlan {
                plan: id("photos"),
                settings: vec![PlanSetting::Label],
            },
            SummaryLine::ChangesPlan {
                plan: id("home"),
                settings: vec![PlanSetting::Label, PlanSetting::Sources],
            },
            SummaryLine::RemovesPlan(id("photos")),
        ]
    );
}

#[test]
fn the_summary_ends_at_a_change_that_no_longer_applies() {
    let proposal = drafted(
        vec![
            ConfigChange::RemovePlan(id("home")),
            ConfigChange::AddPlan(plan("photos")),
        ],
        &config(&["home"]),
    );
    assert_eq!(proposal.summary(&config(&[])), []);
}

#[test]
fn a_plan_over_the_same_sources_as_another_is_a_warning_not_a_refusal() {
    let config = config(&["home"]);
    let mut copy = plan("copy");
    copy.sources = plan("home").sources;
    let changes = vec![
        ConfigChange::AddPlan(plan("photos")),
        ConfigChange::AddPlan(copy),
    ];
    assert!(Proposal::check(&config, &changes).is_ok());
    assert_eq!(
        Proposal::warnings(&config, &changes),
        [ProposalWarning::SameSources {
            index: 1,
            plan: id("copy"),
            other: id("home"),
        }]
    );
    assert_eq!(
        Proposal::warnings(&config, &[ConfigChange::RemovePlan(id("home"))]),
        []
    );
}

#[test]
fn a_glob_that_can_never_match_is_a_warning() {
    let config = config(&[]);
    let mut home = plan("home");
    home.filter.globs = vec!["!/elsewhere/cache".to_owned()];
    let warnings = Proposal::warnings(&config, &[ConfigChange::AddPlan(home)]);
    assert_eq!(
        warnings,
        [ProposalWarning::GlobNeverMatches {
            index: 0,
            glob: "!/elsewhere/cache".to_owned(),
        }]
    );
}

#[test]
fn a_glob_that_may_match_a_source_is_no_warning_and_a_sibling_path_is_one() {
    // The plan's one source is `/home/demo/home`. A wildcard in the last
    // segment leaves `/home/demo` as the fixed folder, which holds the
    // source; a glob of the source itself names it.
    let config = config(&[]);
    let mut home = plan("home");
    home.filter.globs = vec![
        "/home/demo/home/cache".to_owned(),
        "!/home/demo/*.tmp".to_owned(),
        "/home/demo/ho*".to_owned(),
        "/home/demo/home".to_owned(),
        "*.log".to_owned(),
        "/home/demo/homework".to_owned(),
    ];
    home.filter.iglobs = vec!["/HOME/Demo/Home/Mail/*".to_owned()];
    let warnings = Proposal::warnings(&config, &[ConfigChange::AddPlan(home)]);
    assert_eq!(
        warnings,
        [ProposalWarning::GlobNeverMatches {
            index: 0,
            glob: "/home/demo/homework".to_owned(),
        }]
    );
}

fn kept_daily(plan: &mut Plan) {
    plan.retention.keep_daily = Some(KeepCount::new(7).unwrap());
}

fn needs_preview(changes: Vec<ConfigChange>, config: &AgentConfig) -> bool {
    drafted(changes, config).needs_forget_preview(config)
}

#[test]
fn a_retention_change_needs_the_forget_preview() {
    let config = config(&["home"]);
    let mut kept = plan("home");
    kept_daily(&mut kept);
    assert!(needs_preview(vec![ConfigChange::ChangePlan(kept)], &config));
}

#[test]
fn a_new_plan_with_a_retention_policy_needs_the_forget_preview() {
    // Its ID may be an orphaned group's plan, which only the repository
    // tells; with no policy it forgets nothing, whatever the ID.
    let config = config(&["home"]);
    let mut kept = plan("photos");
    kept_daily(&mut kept);
    assert!(needs_preview(vec![ConfigChange::AddPlan(kept)], &config));
    assert!(!needs_preview(
        vec![ConfigChange::AddPlan(plan("photos"))],
        &config
    ));
}

#[test]
fn a_change_of_a_plans_repository_needs_the_forget_preview() {
    let config = config_over(&["nas", "usb"], &["home"]);
    let mut moved = plan("home");
    moved.repository = alias("usb");
    assert!(needs_preview(
        vec![ConfigChange::ChangePlan(moved)],
        &config
    ));
}

#[test]
fn a_change_of_a_plans_host_needs_the_forget_preview() {
    let config = config(&["home"]);
    let mut hosted = plan("home");
    hosted.host = Some(Hostname::parse("demo-host").unwrap());
    assert!(needs_preview(
        vec![ConfigChange::ChangePlan(hosted)],
        &config
    ));
}

#[test]
fn every_removal_needs_the_forget_preview() {
    let config = config(&["home"]);
    assert!(needs_preview(
        vec![ConfigChange::RemovePlan(id("home"))],
        &config
    ));
}

#[test]
fn a_change_that_can_not_delete_needs_no_forget_preview() {
    let config = config(&["home"]);
    let mut changed = plan("home");
    changed.label = Some("demo".to_owned());
    changed.sources = vec![Source::parse("/home/demo/other").unwrap()];
    changed.tags = vec![UserTag::new("demo").unwrap()];
    assert!(!needs_preview(
        vec![
            ConfigChange::AddPlan(plan("photos")),
            ConfigChange::ChangePlan(changed),
        ],
        &config
    ));
}

#[test]
fn each_change_is_read_against_what_the_ones_before_made_for_the_forget_preview() {
    let config = config(&[]);
    let mut kept = plan("photos");
    kept_daily(&mut kept);
    assert!(needs_preview(
        vec![
            ConfigChange::AddPlan(plan("photos")),
            ConfigChange::ChangePlan(kept),
        ],
        &config
    ));
}

#[test]
fn a_change_that_no_longer_applies_needs_the_forget_preview() {
    let proposal = drafted(
        vec![ConfigChange::ChangePlan(plan("home"))],
        &config(&["home"]),
    );
    assert!(proposal.needs_forget_preview(&config(&[])));
}

fn touched(changes: Vec<ConfigChange>, config: &AgentConfig) -> Vec<RepositoryAlias> {
    drafted(changes, config).repositories(config)
}

#[test]
fn a_plan_moved_to_another_repository_touches_both_in_the_order_named() {
    let config = config_over(&["nas", "usb"], &["home", "photos"]);
    let mut moved = plan("home");
    moved.repository = alias("usb");
    assert_eq!(
        touched(
            vec![
                ConfigChange::ChangePlan(moved),
                ConfigChange::RemovePlan(id("photos")),
            ],
            &config
        ),
        [alias("nas"), alias("usb")],
        "each repository once, the old one first"
    );
}

#[test]
fn a_plan_moved_twice_touches_every_repository_it_passes_through() {
    // `usb` holds the plan only between the two changes.
    let config = config_over(&["nas", "usb", "cloud"], &["home"]);
    let mut to_usb = plan("home");
    to_usb.repository = alias("usb");
    let mut to_cloud = plan("home");
    to_cloud.repository = alias("cloud");
    assert_eq!(
        touched(
            vec![
                ConfigChange::ChangePlan(to_usb),
                ConfigChange::ChangePlan(to_cloud),
            ],
            &config
        ),
        [alias("nas"), alias("usb"), alias("cloud")]
    );
}

#[test]
fn a_new_plan_touches_its_repository_and_a_removal_the_removed_plans() {
    let config = config_over(&["nas", "usb"], &["home"]);
    let mut photos = plan("photos");
    photos.repository = alias("usb");
    assert_eq!(
        touched(vec![ConfigChange::AddPlan(photos)], &config),
        [alias("usb")]
    );
    assert_eq!(
        touched(vec![ConfigChange::RemovePlan(id("home"))], &config),
        [alias("nas")]
    );
}

#[test]
fn a_change_that_no_longer_applies_still_names_its_repository() {
    // Drafted when `ghost` and `nobody` were plans; both are gone since:
    // the change names `usb`, and a removal of no plan names none.
    let drafted_against = config_over(&["nas", "usb"], &["home", "ghost", "nobody"]);
    let mut ghost = plan("ghost");
    ghost.repository = alias("usb");
    let proposal = drafted(
        vec![
            ConfigChange::ChangePlan(ghost),
            ConfigChange::RemovePlan(id("nobody")),
        ],
        &drafted_against,
    );
    assert_eq!(
        proposal.repositories(&config_over(&["nas", "usb"], &["home"])),
        [alias("usb")]
    );
}
