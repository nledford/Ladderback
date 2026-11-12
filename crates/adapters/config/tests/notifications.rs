//! The `[notifications]` tables of a config file (technical design §11): the
//! channels and rules they hold, what they refuse and where, and that a save
//! writes them and keeps them.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use app::ports::{ConfigStore, PasswordProblem, Problem};
use config_adapter::{parse, render};
use domain::PlanId;
use domain::notifications::{
    Auth, Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName, Template,
    Username,
};
use domain::planning::{AgentConfig, ValueKind, Violation};
use domain::secrets::{CommandLine, FileRead, KeyringEntry, SecretRef};
use support::*;

fn channel_name(name: &str) -> ChannelName {
    ChannelName::parse(name).expect("a channel name")
}

fn rule_name(name: &str) -> RuleName {
    RuleName::parse(name).expect("a rule name")
}

fn plan(id: &str) -> PlanId {
    PlanId::parse(id).expect("a plan ID")
}

/// [`BASE`], one plan `home` on the repository `nas`, with `tables` after it.
fn with(tables: &str) -> String {
    format!("{BASE}\n{tables}")
}

const CHANNELS: &str = "[notifications.channels.ops]
kind = \"webhook\"
url-env = \"DEMO_WEBHOOK_URL\"
token-file = \"/run/secrets/demo-token\"

[notifications.channels.pager]
kind = \"ntfy\"
url-keyring = \"ladderback/demo-topic\"

[notifications.channels.home-check]
kind = \"healthchecks\"
url-command = \"/usr/bin/demo-ping-url --plan home\"

[notifications.channels.relay]
kind = \"ntfy\"
url-env = \"DEMO_TOPIC_URL\"
username = \"demo-user\"
password-env = \"DEMO_TOPIC_PASSWORD\"

[notifications.channels.archive]
kind = \"webhook\"
url-file = \"/run/secrets/demo-archive-url\"
";

const RULES: &str = "
[notifications.rules.failures]
events = [\"backup.failed\", \"prune.failed\"]
channel = \"pager\"

[notifications.rules.home]
events = [\"backup.started\", \"backup.succeeded\", \"backup.warning\", \"backup.failed\"]
channel = \"home-check\"
plans = [\"home\"]

[notifications.rules.everything]
events = [\"backup.succeeded\"]
channel = \"ops\"
template = \"{event}: {plan} on {repository} at {time}\"
";

fn expected() -> Notifications {
    let channels = BTreeMap::from([
        (
            channel_name("ops"),
            Channel::new(
                ChannelKind::Webhook,
                SecretRef::Env("DEMO_WEBHOOK_URL".to_owned()),
                Some(Auth::Bearer(SecretRef::File {
                    path: "/run/secrets/demo-token".to_owned(),
                    read: FileRead::FirstLine,
                })),
            )
            .expect("a channel"),
        ),
        (
            channel_name("pager"),
            Channel::new(
                ChannelKind::Ntfy,
                SecretRef::Keyring(KeyringEntry::parse("ladderback/demo-topic").expect("an entry")),
                None,
            )
            .expect("a channel"),
        ),
        (
            channel_name("home-check"),
            Channel::new(
                ChannelKind::Healthchecks,
                SecretRef::Command(
                    CommandLine::parse("/usr/bin/demo-ping-url --plan home").expect("a command"),
                ),
                None,
            )
            .expect("a channel"),
        ),
        (
            channel_name("relay"),
            Channel::new(
                ChannelKind::Ntfy,
                SecretRef::Env("DEMO_TOPIC_URL".to_owned()),
                Some(Auth::Basic {
                    username: Username::parse("demo-user").expect("a username"),
                    password: SecretRef::Env("DEMO_TOPIC_PASSWORD".to_owned()),
                }),
            )
            .expect("a channel"),
        ),
        (
            channel_name("archive"),
            Channel::new(
                ChannelKind::Webhook,
                SecretRef::File {
                    path: "/run/secrets/demo-archive-url".to_owned(),
                    read: FileRead::FirstLine,
                },
                None,
            )
            .expect("a channel"),
        ),
    ]);
    let rules = BTreeMap::from([
        (
            rule_name("failures"),
            Rule::new(
                BTreeSet::from([EventKind::BackupFailed, EventKind::PruneFailed]),
                channel_name("pager"),
                BTreeSet::new(),
                None,
            )
            .expect("a rule"),
        ),
        (
            rule_name("home"),
            Rule::new(
                BTreeSet::from([
                    EventKind::BackupStarted,
                    EventKind::BackupSucceeded,
                    EventKind::BackupWarning,
                    EventKind::BackupFailed,
                ]),
                channel_name("home-check"),
                BTreeSet::from([plan("home")]),
                None,
            )
            .expect("a rule"),
        ),
        (
            rule_name("everything"),
            Rule::new(
                BTreeSet::from([EventKind::BackupSucceeded]),
                channel_name("ops"),
                BTreeSet::new(),
                Some(
                    Template::parse("{event}: {plan} on {repository} at {time}")
                        .expect("a template"),
                ),
            )
            .expect("a rule"),
        ),
    ]);
    Notifications::new(channels, rules).expect("rules that name their channels")
}

#[test]
fn channels_and_rules_are_read_from_their_named_tables() {
    let parsed = parsed(&with(&format!("{CHANNELS}{RULES}"))).expect("a valid file");
    assert_eq!(parsed.config.notifications(), &expected());
}

#[test]
fn a_rule_may_send_the_retries_of_a_failed_backup_and_is_written_back_with_them() {
    let text = with(
        "[notifications.channels.ops]\nkind = \"webhook\"\nurl-env = \"DEMO_WEBHOOK_URL\"\n\n[notifications.rules.retries]\nevents = [\"backup.failed\", \"backup.retrying\"]\nchannel = \"ops\"\n",
    );
    let config = parsed(&text).expect("a valid file").config;
    let rule = config
        .notifications()
        .rules()
        .get(&rule_name("retries"))
        .expect("the rule");
    assert_eq!(
        rule.events(),
        &BTreeSet::from([EventKind::BackupFailed, EventKind::BackupRetrying])
    );
    let written = render(Path::new(FILE), &config).expect("renders");
    assert!(
        written.contains("\"backup.retrying\""),
        "the rule's events in:\n{written}"
    );
    let read = parse(
        Path::new(FILE),
        &written,
        &config_adapter::Schemas::production(),
    )
    .expect("the written file reads");
    assert_eq!(read.config, config);
}

#[test]
fn a_file_without_notifications_or_with_an_empty_table_has_none() {
    for text in [BASE.to_owned(), with("[notifications]\n")] {
        let parsed = parsed(&text).expect("a valid file");
        assert!(parsed.config.notifications().is_empty(), "{text}");
    }
}

#[test]
fn a_channels_url_is_never_written_in_the_file() {
    // I4: the URL of a webhook or a topic is a credential, so the file names
    // where it is kept, never the URL itself.
    let text = with(
        "[notifications.channels.ops]\nkind = \"webhook\"\nurl = \"https://demo.example.com/hook\"\n",
    );
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::UnknownKey);
    assert_eq!(error.key.as_deref(), Some("notifications.channels.ops.url"));
    assert!(!format!("{error:?}").contains("demo.example.com"));

    let text = with(
        "[notifications.channels.ops]\nkind = \"ntfy\"\nurl-env = \"DEMO\"\ntoken = \"demo\"\n",
    );
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::UnknownKey);
    assert_eq!(
        error.key.as_deref(),
        Some("notifications.channels.ops.token")
    );
}

#[test]
fn a_channel_needs_exactly_one_url_source_and_at_most_one_token_source() {
    let none = invalid(parsed(&with(
        "[notifications.channels.ops]\nkind = \"webhook\"\n",
    )));
    assert_eq!(none.problem, Problem::Secret(PasswordProblem::Missing));
    assert_eq!(none.key.as_deref(), Some("notifications.channels.ops"));

    let two = invalid(parsed(&with(
        "[notifications.channels.ops]\nkind = \"webhook\"\nurl-env = \"A\"\nurl-file = \"/run/a\"\n",
    )));
    assert_eq!(two.problem, Problem::Secret(PasswordProblem::Several));
    assert_eq!(two.key.as_deref(), Some("notifications.channels.ops"));

    let tokens = invalid(parsed(&with(
        "[notifications.channels.ops]\nkind = \"webhook\"\nurl-env = \"A\"\n\
         token-env = \"B\"\ntoken-file = \"/run/b\"\n",
    )));
    assert_eq!(tokens.problem, Problem::Secret(PasswordProblem::Several));

    let text =
        with("[notifications.channels.ops]\nkind = \"webhook\"\nurl-file = \"relative/url\"\n");
    let relative = invalid(parsed(&text));
    assert_eq!(
        relative.problem,
        Problem::Secret(PasswordProblem::RelativeFile)
    );
    assert_eq!(
        relative.key.as_deref(),
        Some("notifications.channels.ops.url-file")
    );
    assert_eq!(relative.at, Some(position_of(&text, "\"relative/url\"")));
}

// The command runs in the daemon's working directory, as a repository's
// `password-command` does.
#[test]
fn a_channel_command_that_starts_a_program_by_a_relative_path_is_refused_where_it_is_written() {
    let tables = [
        ("url-command", "kind = \"webhook\"\n"),
        ("token-command", "kind = \"ntfy\"\nurl-env = \"A\"\n"),
        (
            "password-command",
            "kind = \"webhook\"\nurl-env = \"A\"\nusername = \"demo-user\"\n",
        ),
    ];
    for (key, table) in tables {
        let command = "bin/get-secret --vault CANARY-vault";
        let text = with(&format!(
            "[notifications.channels.ops]\n{table}{key} = \"{command}\"\n"
        ));
        let relative = invalid(parsed(&text));
        assert_eq!(
            relative.problem,
            Problem::Secret(PasswordProblem::RelativeProgram),
            "{key}"
        );
        assert_eq!(
            relative.key.as_deref(),
            Some(format!("notifications.channels.ops.{key}").as_str())
        );
        assert_eq!(
            relative.at,
            Some(position_of(&text, &format!("\"{command}\"")))
        );
        assert!(!format!("{relative:?}").contains("CANARY"), "{relative:?}");
        let text = with(&format!(
            "[notifications.channels.ops]\n{table}{key} = \"get-secret --vault demo\"\n"
        ));
        assert!(parsed(&text).is_ok(), "{key}");
    }
}

#[test]
fn a_ping_url_signs_in_with_nothing() {
    for key in [
        "token-env = \"B\"",
        "username = \"demo-user\"",
        "password-env = \"B\"",
    ] {
        let text = with(&format!(
            "[notifications.channels.check]\nkind = \"healthchecks\"\nurl-env = \"A\"\n{key}\n"
        ));
        let error = invalid(parsed(&text));
        assert_eq!(error.problem, Problem::UnknownKey, "{key}");
        let name = key.split(' ').next().unwrap_or_default();
        assert_eq!(
            error.key.as_deref(),
            Some(format!("notifications.channels.check.{name}").as_str())
        );
    }
}

#[test]
fn a_channel_signs_in_with_a_token_or_a_name_and_password_never_both() {
    let channel = "[notifications.channels.ops]\nkind = \"ntfy\"\nurl-env = \"A\"\n";

    let both = invalid(parsed(&with(&format!(
        "{channel}token-env = \"B\"\nusername = \"demo-user\"\npassword-env = \"C\"\n"
    ))));
    assert_eq!(both.problem, Problem::Secret(PasswordProblem::Several));
    assert_eq!(both.key.as_deref(), Some("notifications.channels.ops"));

    let no_password = invalid(parsed(&with(&format!(
        "{channel}username = \"demo-user\"\n"
    ))));
    assert_eq!(
        no_password.problem,
        Problem::Secret(PasswordProblem::Missing)
    );
    assert_eq!(
        no_password.key.as_deref(),
        Some("notifications.channels.ops")
    );

    let no_name = invalid(parsed(&with(&format!("{channel}password-env = \"C\"\n"))));
    assert_eq!(no_name.problem, Problem::MissingKey);
    assert_eq!(
        no_name.key.as_deref(),
        Some("notifications.channels.ops.username")
    );

    let passwords = invalid(parsed(&with(&format!(
        "{channel}username = \"demo-user\"\npassword-env = \"C\"\npassword-file = \"/run/c\"\n"
    ))));
    assert_eq!(passwords.problem, Problem::Secret(PasswordProblem::Several));

    let text = with(&format!(
        "{channel}username = \"demo:user\"\npassword-env = \"C\"\n"
    ));
    let bad_name = invalid(parsed(&text));
    assert_eq!(bad_name.problem, Problem::Value(ValueKind::ChannelUsername));
    assert_eq!(
        bad_name.key.as_deref(),
        Some("notifications.channels.ops.username")
    );
    assert_eq!(bad_name.at, Some(position_of(&text, "\"demo:user\"")));
}

#[test]
fn a_value_a_channel_or_rule_refuses_is_reported_where_it_is_written() {
    let channel = "[notifications.channels.ops]\nkind = \"webhook\"\nurl-env = \"A\"\n";
    for (text, key, kind, value) in [
        (
            with("[notifications.channels.ops]\nkind = \"gotify\"\nurl-env = \"A\"\n"),
            "notifications.channels.ops.kind",
            ValueKind::ChannelKind,
            "\"gotify\"",
        ),
        (
            with(&format!(
                "{channel}\n[notifications.rules.failures]\nevents = [\"backup.failed\", \"backup.missed\"]\nchannel = \"ops\"\n"
            )),
            "notifications.rules.failures.events[1]",
            ValueKind::NotificationEvent,
            "\"backup.missed\"",
        ),
        (
            with(&format!(
                "{channel}\n[notifications.rules.failures]\nevents = []\nchannel = \"ops\"\n"
            )),
            "notifications.rules.failures.events",
            ValueKind::NotificationEvent,
            "[]",
        ),
        (
            with(&format!(
                "{channel}\n[notifications.rules.failures]\nevents = [\"backup.failed\"]\nchannel = \"ops\"\ntemplate = \"{{password}}\"\n"
            )),
            "notifications.rules.failures.template",
            ValueKind::NotificationTemplate,
            "\"{password}\"",
        ),
    ] {
        let error = invalid(parsed(&text));
        assert_eq!(error.problem, Problem::Value(kind), "{key}");
        assert_eq!(error.key.as_deref(), Some(key));
        assert_eq!(error.at, Some(position_of(&text, value)), "{key}");
    }
}

#[test]
fn a_name_that_is_not_a_slug_is_refused_at_its_table() {
    let text = with("[notifications.channels.Ops]\nkind = \"webhook\"\nurl-env = \"A\"\n");
    let error = invalid(parsed(&text));
    assert_eq!(error.problem, Problem::Value(ValueKind::NotificationName));
    assert_eq!(error.key.as_deref(), Some("notifications.channels.Ops"));
}

#[test]
fn a_rule_that_names_no_channel_or_plan_the_file_has_is_refused_where_it_names_it() {
    let channel = "[notifications.channels.ops]\nkind = \"webhook\"\nurl-env = \"A\"\n";
    let text = with(&format!(
        "{channel}\n[notifications.rules.failures]\nevents = [\"backup.failed\"]\nchannel = \"pager\"\n"
    ));
    let error = invalid(parsed(&text));
    assert_eq!(
        error.problem,
        Problem::Violation(Violation::UnknownChannel {
            rule: rule_name("failures"),
            channel: channel_name("pager"),
        })
    );
    assert_eq!(
        error.key.as_deref(),
        Some("notifications.rules.failures.channel")
    );
    assert_eq!(error.at, Some(position_of(&text, "\"pager\"")));

    let text = with(&format!(
        "{channel}\n[notifications.rules.failures]\nevents = [\"backup.failed\"]\nchannel = \"ops\"\nplans = [\"work\"]\n"
    ));
    let error = invalid(parsed(&text));
    assert_eq!(
        error.problem,
        Problem::Violation(Violation::UnknownRulePlan {
            rule: rule_name("failures"),
            plan: plan("work"),
        })
    );
    assert_eq!(
        error.key.as_deref(),
        Some("notifications.rules.failures.plans")
    );
}

#[test]
fn notifications_are_written_and_read_back_as_they_were() {
    let config = parsed(BASE)
        .expect("a valid file")
        .config
        .with_notifications(expected())
        .expect("rules for the plans the file has");
    let text = render(Path::new(FILE), &config).expect("renders");
    for header in [
        "[notifications.channels.archive]",
        "[notifications.channels.home-check]",
        "[notifications.channels.ops]",
        "[notifications.rules.everything]",
        "[notifications.rules.home]",
    ] {
        assert!(text.contains(header), "{header} in:\n{text}");
    }
    let read = parse(
        Path::new(FILE),
        &text,
        &config_adapter::Schemas::production(),
    )
    .expect("the written file reads");
    assert_eq!(read.config, config);
}

#[tokio::test]
async fn a_save_keeps_the_notifications_and_the_words_around_them() {
    let scratch = Scratch::new();
    let text = with(&format!(
        "# Where the pager goes.\n{CHANNELS}# Who hears of what.\n{RULES}"
    ));
    scratch.write(&text);
    let store = scratch.store();
    let loaded = store.load().await.expect("the file loads");
    assert_eq!(loaded.notifications(), &expected());

    // A change elsewhere in the file: the notifications and their comments
    // stay as they were written.
    let mut plans = loaded.plans().to_vec();
    plans[0].label = Some("demo".to_owned());
    let changed = AgentConfig::new(
        loaded.agent().clone(),
        loaded.hooks().clone(),
        loaded.repositories().clone(),
        plans,
    )
    .and_then(|config| config.with_notifications(loaded.notifications().clone()))
    .expect("a valid configuration");
    store.save(&changed).await.expect("saved");
    let written = scratch.read();
    assert!(written.contains("# Where the pager goes.\n"), "{written}");
    assert!(written.contains("# Who hears of what.\n"), "{written}");
    assert!(written.contains(RULES), "{written}");
    assert_eq!(store.load().await.expect("loads"), changed);
}

#[tokio::test]
async fn a_save_adds_rules_to_a_file_that_had_none() {
    let scratch = Scratch::new();
    let text = format!("# Written by hand.\n{BASE}");
    scratch.write(&text);
    let store = scratch.store();
    let loaded = store.load().await.expect("the file loads");
    let with_rules = loaded
        .with_notifications(expected())
        .expect("rules for the plans the file has");
    store.save(&with_rules).await.expect("saved");
    let written = scratch.read();
    assert!(written.starts_with("# Written by hand.\n"), "{written}");
    assert_eq!(store.load().await.expect("loads"), with_rules);
}
