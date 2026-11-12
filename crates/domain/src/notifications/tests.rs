use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;

use super::*;
use crate::execution::{FailureReason, Target, Trigger};
use crate::planning::{AgentConfig, AgentSettings, Hooks, Plan, Source};
use crate::secrets::SecretValue;
use crate::{RepositoryAlias, Uuid};

fn at() -> Timestamp {
    "2026-11-12T09:30:00Z".parse().unwrap()
}

fn plan(id: &str) -> PlanId {
    PlanId::parse(id).unwrap()
}

fn channel_name(name: &str) -> ChannelName {
    ChannelName::parse(name).unwrap()
}

fn rule_name(name: &str) -> RuleName {
    RuleName::parse(name).unwrap()
}

fn env(name: &str) -> SecretRef {
    SecretRef::Env(name.to_owned())
}

fn webhook() -> Channel {
    Channel::new(ChannelKind::Webhook, env("DEMO_WEBHOOK_URL"), None).unwrap()
}

fn events(kinds: &[EventKind]) -> BTreeSet<EventKind> {
    kinds.iter().copied().collect()
}

fn rule(kinds: &[EventKind], channel: &str, plans: &[&str]) -> Rule {
    Rule::new(
        events(kinds),
        channel_name(channel),
        plans.iter().map(|id| plan(id)).collect(),
        None,
    )
    .unwrap()
}

fn backup_event(kind: EventKind, plan_id: Option<&str>) -> Event {
    Event {
        kind,
        operation: OperationId::from_uuid(Uuid::from_u128(7)),
        plan: plan_id.map(plan),
        repository: Some(RepositoryAlias::parse("nas").unwrap()),
        at: at(),
        code: None,
    }
}

fn operation(kind: OperationKind, plan_id: Option<&str>) -> Operation {
    let mut operation = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(7)),
        kind,
        Trigger::Manual,
        Target {
            plan: plan_id.map(plan),
            repository: None,
        },
        at(),
    );
    operation.start(at()).unwrap();
    operation
}

#[test]
fn each_event_is_named_as_section_11_names_it_and_read_back_by_that_name() {
    let names: Vec<&str> = EventKind::ALL.iter().map(|kind| kind.name()).collect();
    assert_eq!(
        names,
        [
            "backup.started",
            "backup.succeeded",
            "backup.warning",
            "backup.failed",
            "forget.failed",
            "prune.failed",
            "check.failed",
        ]
    );
    for kind in EventKind::ALL {
        assert_eq!(EventKind::parse(kind.name()), Ok(kind));
    }
}

#[test]
fn a_name_that_is_no_event_is_refused() {
    for text in ["", "backup", "Backup.Failed", "backup.retrying", "backup.failed "] {
        let refused = EventKind::parse(text).unwrap_err();
        assert_eq!(refused.kind, ValueKind::NotificationEvent, "{text:?}");
    }
}

#[test]
fn a_start_pings_start_a_success_or_warning_pings_success_and_a_failure_pings_fail() {
    assert_eq!(EventKind::BackupStarted.ping(), Ping::Start);
    assert_eq!(EventKind::BackupSucceeded.ping(), Ping::Success);
    assert_eq!(EventKind::BackupWarning.ping(), Ping::Success);
    for kind in [
        EventKind::BackupFailed,
        EventKind::ForgetFailed,
        EventKind::PruneFailed,
        EventKind::CheckFailed,
    ] {
        assert_eq!(kind.ping(), Ping::Fail, "{kind:?}");
    }
}

#[test]
fn only_a_backup_tells_its_start() {
    assert_eq!(
        EventKind::started(OperationKind::Backup),
        Some(EventKind::BackupStarted)
    );
    for kind in [
        OperationKind::Forget,
        OperationKind::Prune,
        OperationKind::Check,
        OperationKind::Init,
    ] {
        assert_eq!(EventKind::started(kind), None, "{kind:?}");
    }
}

#[test]
fn a_backup_tells_each_ending_and_maintenance_only_its_failure() {
    use Outcome::{Failed, Succeeded, SucceededWithWarnings};
    let told = [
        (OperationKind::Backup, Succeeded, Some(EventKind::BackupSucceeded)),
        (
            OperationKind::Backup,
            SucceededWithWarnings,
            Some(EventKind::BackupWarning),
        ),
        (OperationKind::Backup, Failed, Some(EventKind::BackupFailed)),
        (OperationKind::Forget, Failed, Some(EventKind::ForgetFailed)),
        (OperationKind::Prune, Failed, Some(EventKind::PruneFailed)),
        (OperationKind::Check, Failed, Some(EventKind::CheckFailed)),
        (OperationKind::Forget, Succeeded, None),
        (OperationKind::Prune, SucceededWithWarnings, None),
        (OperationKind::Check, Succeeded, None),
        (OperationKind::Adopt, Failed, None),
        (OperationKind::Init, Failed, None),
        (OperationKind::Identify, Failed, None),
    ];
    for (kind, outcome, event) in told {
        assert_eq!(EventKind::ended(kind, outcome), event, "{kind:?} {outcome:?}");
    }
}

#[test]
fn a_failed_operation_tells_its_stored_code_and_never_its_detail() {
    let mut failed = operation(OperationKind::Backup, Some("home"));
    failed
        .fail(
            FailureReason::EngineFailed {
                detail: "canary-detail-text".to_owned(),
            },
            at(),
        )
        .unwrap();
    let event = Event::ended(Subject::of(&failed), Outcome::Failed, at()).unwrap();
    assert_eq!(event.kind, EventKind::BackupFailed);
    assert_eq!(event.code, Some("operation-failed"));
    assert_eq!(event.plan, Some(plan("home")));
    assert_eq!(event.operation, failed.id());
    assert_eq!(event.at, at());
    assert!(!format!("{event:?}").contains("canary-detail-text"));
}

#[test]
fn a_started_backup_tells_its_plan_and_a_maintenance_start_is_no_event() {
    let backup = operation(OperationKind::Backup, Some("home"));
    let event = Event::started(Subject::of(&backup), at()).unwrap();
    assert_eq!(event.kind, EventKind::BackupStarted);
    assert_eq!(event.plan, Some(plan("home")));
    assert_eq!(event.code, None);
    let prune = operation(OperationKind::Prune, None)
        .with_repository_alias(RepositoryAlias::parse("nas").unwrap());
    assert_eq!(Event::started(Subject::of(&prune), at()), None);
}

#[test]
fn a_maintenance_failure_names_its_repository_by_alias() {
    let mut prune = operation(OperationKind::Prune, None)
        .with_repository_alias(RepositoryAlias::parse("nas").unwrap());
    prune.fail(FailureReason::DeadlinePassed, at()).unwrap();
    let event = Event::ended(Subject::of(&prune), Outcome::Failed, at()).unwrap();
    assert_eq!(event.kind, EventKind::PruneFailed);
    assert_eq!(event.repository, Some(RepositoryAlias::parse("nas").unwrap()));
    assert_eq!(event.code, Some("deadline-passed"));
}

#[test]
fn a_succeeded_ending_has_no_code() {
    let mut backup = operation(OperationKind::Backup, Some("home"));
    backup.succeed(at()).unwrap();
    let event = Event::ended(Subject::of(&backup), Outcome::Succeeded, at()).unwrap();
    assert_eq!(event.kind, EventKind::BackupSucceeded);
    assert_eq!(event.code, None);
}

#[test]
fn a_failure_the_log_has_not_recorded_has_no_code_and_keeps_the_repository_it_is_given() {
    // The run told of the failure, but the log lost its last write: the
    // operation is still running there, and its reason is unknown.
    let backup = operation(OperationKind::Backup, Some("home"));
    let nas = RepositoryAlias::parse("nas").unwrap();
    let subject = Subject {
        repository: Some(&nas),
        ..Subject::of(&backup)
    };
    let event = Event::ended(subject, Outcome::Failed, at()).unwrap();
    assert_eq!(event.kind, EventKind::BackupFailed);
    assert_eq!(event.code, None);
    assert_eq!(event.repository, Some(nas));
}

#[test]
fn names_are_slugs() {
    for good in ["ops", "home-check", "a1"] {
        assert_eq!(ChannelName::parse(good).unwrap().as_str(), good);
        assert_eq!(RuleName::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "Ops", "-ops", "ops.fail", &"a".repeat(65)] {
        assert_eq!(
            ChannelName::parse(bad).unwrap_err().kind,
            ValueKind::NotificationName,
            "{bad:?}"
        );
        assert_eq!(
            RuleName::parse(bad).unwrap_err().kind,
            ValueKind::NotificationName,
            "{bad:?}"
        );
    }
}

#[test]
fn channel_kinds_are_read_by_name_and_only_webhook_and_ntfy_sign_in() {
    for kind in ChannelKind::ALL {
        assert_eq!(ChannelKind::parse(kind.name()), Ok(kind));
    }
    assert_eq!(ChannelKind::Webhook.name(), "webhook");
    assert_eq!(ChannelKind::Ntfy.name(), "ntfy");
    assert_eq!(ChannelKind::Healthchecks.name(), "healthchecks");
    assert_eq!(
        ChannelKind::parse("gotify").unwrap_err().kind,
        ValueKind::ChannelKind
    );
    assert!(ChannelKind::Webhook.takes_auth());
    assert!(ChannelKind::Ntfy.takes_auth());
    assert!(!ChannelKind::Healthchecks.takes_auth());
}

fn basic(password: SecretRef) -> Auth {
    Auth::Basic {
        username: Username::parse("demo-user").unwrap(),
        password,
    }
}

#[test]
fn a_channel_never_holds_its_url_token_or_password_as_text() {
    let inline = || SecretRef::Inline(SecretValue::new("https://demo.example.com/hook"));
    assert_eq!(
        Channel::new(ChannelKind::Webhook, inline(), None),
        Err(ChannelError::InlineSecret)
    );
    assert_eq!(
        Channel::new(
            ChannelKind::Ntfy,
            env("DEMO_TOPIC_URL"),
            Some(Auth::Bearer(inline()))
        ),
        Err(ChannelError::InlineSecret)
    );
    assert_eq!(
        Channel::new(ChannelKind::Ntfy, env("DEMO_TOPIC_URL"), Some(basic(inline()))),
        Err(ChannelError::InlineSecret)
    );
    let ntfy = Channel::new(
        ChannelKind::Ntfy,
        env("DEMO_TOPIC_URL"),
        Some(Auth::Bearer(env("DEMO_TOKEN"))),
    )
    .unwrap();
    assert_eq!(ntfy.kind(), ChannelKind::Ntfy);
    assert_eq!(ntfy.url(), &env("DEMO_TOPIC_URL"));
    assert_eq!(ntfy.auth(), Some(&Auth::Bearer(env("DEMO_TOKEN"))));
    let signed_in = Channel::new(
        ChannelKind::Webhook,
        env("DEMO_WEBHOOK_URL"),
        Some(basic(env("DEMO_PASSWORD"))),
    )
    .unwrap();
    assert_eq!(signed_in.auth(), Some(&basic(env("DEMO_PASSWORD"))));
}

#[test]
fn a_ping_url_signs_in_with_nothing() {
    for auth in [Auth::Bearer(env("DEMO_TOKEN")), basic(env("DEMO_PASSWORD"))] {
        assert_eq!(
            Channel::new(ChannelKind::Healthchecks, env("DEMO_PING_URL"), Some(auth)),
            Err(ChannelError::AuthNotTaken)
        );
    }
    assert!(Channel::new(ChannelKind::Healthchecks, env("DEMO_PING_URL"), None).is_ok());
}

#[test]
fn a_username_is_short_plain_text_without_a_colon() {
    for good in ["demo-user", "Demo User", "ü", &"a".repeat(MAX_USERNAME_BYTES)] {
        assert_eq!(Username::parse(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        "demo:user",
        "demo\nuser",
        "demo\u{7f}",
        &"a".repeat(MAX_USERNAME_BYTES + 1),
    ] {
        let refused = Username::parse(bad).unwrap_err();
        assert_eq!(refused.kind, ValueKind::ChannelUsername, "{bad:?}");
    }
}

#[test]
fn a_rule_sends_at_least_one_event() {
    let refused = Rule::new(BTreeSet::new(), channel_name("ops"), BTreeSet::new(), None);
    assert_eq!(refused.unwrap_err().kind, ValueKind::NotificationEvent);
}

#[test]
fn a_rule_matches_the_events_it_names_for_the_plans_it_names() {
    let any_plan = rule(&[EventKind::BackupFailed], "ops", &[]);
    assert!(any_plan.matches(&backup_event(EventKind::BackupFailed, Some("home"))));
    assert!(any_plan.matches(&backup_event(EventKind::BackupFailed, Some("work"))));
    assert!(!any_plan.matches(&backup_event(EventKind::BackupSucceeded, Some("home"))));

    let home = rule(
        &[EventKind::BackupFailed, EventKind::BackupStarted],
        "ops",
        &["home"],
    );
    assert!(home.matches(&backup_event(EventKind::BackupFailed, Some("home"))));
    assert!(home.matches(&backup_event(EventKind::BackupStarted, Some("home"))));
    assert!(!home.matches(&backup_event(EventKind::BackupFailed, Some("work"))));
    // An event of no plan, a prune's, is not one of the plans named.
    assert!(!home.matches(&backup_event(EventKind::BackupFailed, None)));
    assert!(any_plan.matches(&backup_event(EventKind::BackupFailed, None)));
}

#[test]
fn a_rule_must_send_to_a_channel_the_configuration_has() {
    let violation = Notifications::new(
        BTreeMap::from([(channel_name("ops"), webhook())]),
        BTreeMap::from([
            (rule_name("a"), rule(&[EventKind::BackupFailed], "ops", &[])),
            (rule_name("b"), rule(&[EventKind::BackupFailed], "pager", &[])),
            (rule_name("c"), rule(&[EventKind::BackupFailed], "other", &[])),
        ]),
    )
    .unwrap_err();
    assert_eq!(
        violation,
        Violation::UnknownChannel {
            rule: rule_name("b"),
            channel: channel_name("pager"),
        }
    );
}

#[test]
fn matching_gives_each_rule_that_sends_the_event_in_name_order_with_its_channel() {
    let pager = Channel::new(ChannelKind::Ntfy, env("DEMO_TOPIC_URL"), None).unwrap();
    let notifications = Notifications::new(
        BTreeMap::from([(channel_name("ops"), webhook()), (channel_name("pager"), pager.clone())]),
        BTreeMap::from([
            (rule_name("z-failures"), rule(&[EventKind::BackupFailed], "pager", &[])),
            (rule_name("a-failures"), rule(&[EventKind::BackupFailed], "ops", &["home"])),
            (rule_name("starts"), rule(&[EventKind::BackupStarted], "ops", &[])),
        ]),
    )
    .unwrap();
    let event = backup_event(EventKind::BackupFailed, Some("home"));
    let matched: Vec<(&str, &str, &Channel)> = notifications
        .matching(&event)
        .map(|m| (m.rule.as_str(), m.channel_name.as_str(), m.channel))
        .collect();
    assert_eq!(
        matched,
        [("a-failures", "ops", &webhook()), ("z-failures", "pager", &pager)]
    );
    let other = backup_event(EventKind::BackupFailed, Some("work"));
    let names: Vec<&str> = notifications
        .matching(&other)
        .map(|m| m.rule.as_str())
        .collect();
    assert_eq!(names, ["z-failures"]);
}

#[test]
fn a_rule_must_be_for_plans_the_configuration_has() {
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(
            RepositoryAlias::parse("nas").unwrap(),
            crate::planning::RepositoryConfig {
                id: None,
                settings: crate::planning::RepositorySettings {
                    repository: "/srv/demo".to_owned(),
                    repo_hot: None,
                    password: env("DEMO_PASSWORD"),
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                },
                maintenance: crate::planning::Maintenance::None,
                group_by: Default::default(),
                hooks: Hooks::default(),
            },
        )]),
        vec![Plan::new(
            plan("home"),
            RepositoryAlias::parse("nas").unwrap(),
            vec![Source::parse("/home/demo").unwrap()],
        )],
    )
    .unwrap();
    let with = |plans: &[&str]| {
        Notifications::new(
            BTreeMap::from([(channel_name("ops"), webhook())]),
            BTreeMap::from([(rule_name("failures"), rule(&[EventKind::BackupFailed], "ops", plans))]),
        )
        .unwrap()
    };
    assert_eq!(
        config.clone().with_notifications(with(&["home", "work"])),
        Err(Violation::UnknownRulePlan {
            rule: rule_name("failures"),
            plan: plan("work"),
        })
    );
    let kept = config.with_notifications(with(&["home"])).unwrap();
    assert_eq!(kept.notifications(), &with(&["home"]));
}

mod templates {
    use super::*;

    fn failed_event() -> Event {
        Event {
            code: Some("operation-failed"),
            ..backup_event(EventKind::BackupFailed, Some("home"))
        }
    }

    #[test]
    fn each_placeholder_is_the_fact_it_names() {
        let template =
            Template::parse("{event} {plan} {repository} {operation} {code} {time}").unwrap();
        assert_eq!(
            template.render(&failed_event()),
            "backup.failed home nas 00000000-0000-0000-0000-000000000007 operation-failed 2026-11-12T09:30:00Z"
        );
    }

    #[test]
    fn text_around_placeholders_is_kept_as_written_and_doubled_braces_are_braces() {
        let template = Template::parse("Backup {{{plan}}} failed: {code}. }} done").unwrap();
        assert_eq!(
            template.render(&failed_event()),
            "Backup {home} failed: operation-failed. } done"
        );
        assert_eq!(template.as_str(), "Backup {{{plan}}} failed: {code}. }} done");
    }

    #[test]
    fn a_fact_the_event_lacks_is_left_empty() {
        let template = Template::parse("[{plan}] [{code}]").unwrap();
        let event = backup_event(EventKind::BackupSucceeded, None);
        assert_eq!(template.render(&event), "[] []");
    }

    #[test]
    fn a_template_can_name_no_secret_nor_any_other_unlisted_fact() {
        // I4: the list is closed, so no template can ask for a secret.
        for name in [
            "password", "token", "url", "secret", "address", "detail", "env", "PLAN", "plan ",
            "",
        ] {
            let refused = Template::parse(&format!("before {{{name}}} after")).unwrap_err();
            assert_eq!(refused.kind, ValueKind::NotificationTemplate, "{name:?}");
            assert!(refused.value.is_empty(), "the refusal keeps no text");
        }
    }

    #[test]
    fn an_unclosed_or_unopened_brace_is_refused() {
        for text in ["{plan", "plan}", "{", "}", "{{plan}", "a {plan} }"] {
            assert_eq!(
                Template::parse(text).unwrap_err().kind,
                ValueKind::NotificationTemplate,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_template_is_neither_empty_nor_longer_than_its_cap() {
        assert_eq!(MAX_TEMPLATE_BYTES, 2048);
        assert!(Template::parse("").is_err());
        assert!(Template::parse(&"a".repeat(MAX_TEMPLATE_BYTES)).is_ok());
        assert_eq!(
            Template::parse(&"a".repeat(MAX_TEMPLATE_BYTES + 1))
                .unwrap_err()
                .kind,
            ValueKind::NotificationTemplate
        );
        // Counted in bytes, not characters.
        assert!(Template::parse(&"é".repeat(MAX_TEMPLATE_BYTES / 2 + 1)).is_err());
    }

    #[test]
    fn multibyte_text_around_a_placeholder_is_kept_whole() {
        let template = Template::parse("Sicherung «{plan}» fehlgeschlagen ✗").unwrap();
        assert_eq!(
            template.render(&failed_event()),
            "Sicherung «home» fehlgeschlagen ✗"
        );
    }
}
