use domain::notifications::{Event, EventKind, Ping};
use domain::planning::Locale;
use domain::{OperationId, PlanId, RepositoryAlias, Timestamp, Uuid};

use super::*;

const OPERATION: u128 = 0x0193_0000_0000_7000_8000_0000_0000_0001;

fn localizer() -> Localizer {
    Localizer::new(&["en-US".parse().expect("a locale")]).with_isolation(false)
}

fn event(kind: EventKind) -> Event {
    Event {
        kind,
        operation: OperationId::from_uuid(Uuid::from_u128(OPERATION)),
        plan: PlanId::parse("demo-plan").ok(),
        repository: RepositoryAlias::parse("demo-repo").ok(),
        at: Timestamp::from_second(1_800_000_000).expect("a time"),
        code: Some("operation-failed"),
    }
}

#[test]
fn a_server_on_the_local_network_over_plain_http_is_an_address() {
    for url in [
        "http://10.0.0.2:8080/demo-topic",
        "http://192.168.1.20/demo-topic",
        "http://ntfy.home.arpa/demo-topic",
        "http://127.0.0.1:2586/demo-topic",
        "https://ntfy.example.com/demo-topic",
    ] {
        assert_eq!(address(url).map(|url| url.to_string()), Ok(url.to_owned()));
    }
}

#[test]
fn a_url_that_is_not_http_with_a_host_is_no_address() {
    for url in [
        "ftp://127.0.0.1/demo",
        "file:///etc/hosts",
        "not a url",
        "",
        "http://",
    ] {
        assert_eq!(address(url), Err(NotifyError::InvalidAddress), "{url:?}");
    }
}

#[test]
fn a_url_longer_than_the_cap_is_no_address() {
    let base = "https://ntfy.example.com/";
    let longest = format!("{base}{}", "a".repeat(MAX_URL_BYTES - base.len()));
    assert_eq!(longest.len(), MAX_URL_BYTES);
    assert!(address(&longest).is_ok(), "a URL at the cap is taken");
    let longer = format!("{longest}a");
    assert_eq!(address(&longer), Err(NotifyError::InvalidAddress));
}

#[test]
fn only_a_success_status_is_a_delivery() {
    for code in [200, 201, 204, 299] {
        assert_eq!(status(code), Ok(()), "{code}");
    }
    for code in [100, 199, 300, 301, 307, 400, 404, 500] {
        assert_eq!(
            status(code),
            Err(NotifyError::Rejected { status: code }),
            "{code}"
        );
    }
}

#[test]
fn a_check_is_pinged_at_its_url_or_at_start_or_fail_of_it() {
    let check = |text: &str| Url::parse(text).expect("a URL");
    let pinged = |text: &str, ping| ping_url(check(text), ping).to_string();
    let base = "https://hc.example.com/ping/demo-check";
    assert_eq!(pinged(base, Ping::Success), base);
    assert_eq!(pinged(base, Ping::Start), format!("{base}/start"));
    assert_eq!(pinged(base, Ping::Fail), format!("{base}/fail"));
    assert_eq!(
        pinged(&format!("{base}/"), Ping::Fail),
        format!("{base}/fail")
    );
    assert_eq!(
        pinged(&format!("{base}?create=1"), Ping::Start),
        format!("{base}/start?create=1")
    );
}

#[test]
fn a_webhook_is_posted_every_fact_of_the_event() {
    let body = webhook_body(&event(EventKind::BackupFailed), "demo text");
    assert_eq!(
        body,
        serde_json::json!({
            "event": "backup.failed",
            "message": "demo text",
            "operation": "01930000-0000-7000-8000-000000000001",
            "plan": "demo-plan",
            "repository": "demo-repo",
            "code": "operation-failed",
            "time": "2027-01-15T08:00:00Z",
        })
    );
}

#[test]
fn a_fact_the_event_lacks_is_null_in_a_webhook_body() {
    let mut event = event(EventKind::BackupStarted);
    event.plan = None;
    event.repository = None;
    event.code = None;
    let body = webhook_body(&event, "");
    for key in ["plan", "repository", "code"] {
        assert_eq!(body[key], serde_json::Value::Null, "{key}");
    }
}

#[test]
fn a_default_message_names_the_plan_or_the_repository_and_the_code() {
    let localizer = localizer();
    let says = |kind| default_message(&localizer, &event(kind));
    assert_eq!(
        says(EventKind::BackupStarted),
        "The backup of demo-plan started."
    );
    assert_eq!(
        says(EventKind::BackupSucceeded),
        "The backup of demo-plan succeeded."
    );
    assert!(
        says(EventKind::BackupWarning)
            .starts_with("The backup of demo-plan succeeded with warnings.")
    );
    assert_eq!(
        says(EventKind::BackupFailed),
        "The backup of demo-plan failed (operation-failed)."
    );
    assert_eq!(
        says(EventKind::ForgetFailed),
        "Forgetting old snapshots in demo-repo failed (operation-failed)."
    );
    assert_eq!(
        says(EventKind::PruneFailed),
        "The prune of demo-repo failed (operation-failed)."
    );
    assert_eq!(
        says(EventKind::CheckFailed),
        "The check of demo-repo failed (operation-failed)."
    );
}

#[test]
fn a_default_message_without_a_plan_repository_or_code_names_the_operation_and_none() {
    let localizer = localizer();
    let mut backup = event(EventKind::BackupFailed);
    backup.plan = None;
    backup.code = None;
    assert_eq!(
        default_message(&localizer, &backup),
        "The backup of 01930000-0000-7000-8000-000000000001 failed (none)."
    );
    let mut check = event(EventKind::CheckFailed);
    check.repository = None;
    assert_eq!(
        default_message(&localizer, &check),
        "The check of 01930000-0000-7000-8000-000000000001 failed (operation-failed)."
    );
}

#[test]
fn the_notifier_writes_default_messages_without_isolation_marks() {
    let notifier = HttpNotifier::new(
        Localizer::new(&["en-US".parse().expect("a locale")]),
        Duration::from_secs(1),
    )
    .expect("a client");
    let notification = Notification {
        destination: app::ports::Destination {
            kind: ChannelKind::Webhook,
            url: domain::secrets::SecretValue::new("http://127.0.0.1:9/demo"),
            auth: None,
        },
        event: event(EventKind::BackupStarted),
        message: Message::Default,
        locale: None,
    };
    assert_eq!(
        notifier.text(&notification),
        "The backup of demo-plan started."
    );
}

/// The default message of a backup's start, in the agent's `locale`, if any.
fn default_for(locale: Option<&str>) -> Notification {
    Notification {
        destination: app::ports::Destination {
            kind: ChannelKind::Webhook,
            url: domain::secrets::SecretValue::new("http://127.0.0.1:9/demo"),
            auth: None,
        },
        event: event(EventKind::BackupStarted),
        message: Message::Default,
        locale: locale.map(|locale| Locale::parse(locale).expect("a locale")),
    }
}

#[test]
fn a_default_message_is_written_in_the_agents_locale_and_else_in_the_systems() {
    let notifier = HttpNotifier::new(localizer(), Duration::from_secs(1)).expect("a client");
    let english = "The backup of demo-plan started.";
    let pseudo = default_message(
        &Localizer::new(&["en-XA".parse().expect("a locale")]),
        &event(EventKind::BackupStarted),
    );
    assert_ne!(pseudo, english);
    assert_eq!(notifier.text(&default_for(Some("en-XA"))), pseudo);
    assert_eq!(notifier.text(&default_for(None)), english);
    assert_eq!(notifier.text(&default_for(Some("en-XA"))), pseudo);
    // A locale without a catalog falls to the system's.
    assert_eq!(notifier.text(&default_for(Some("de-DE"))), english);
}

#[test]
fn an_agents_locale_without_a_catalog_falls_back_to_the_systems() {
    let system = Localizer::new(&["en-XA".parse().expect("a locale")]);
    let pseudo = default_message(&system, &event(EventKind::BackupStarted));
    let notifier = HttpNotifier::new(system, Duration::from_secs(1)).expect("a client");
    assert_eq!(notifier.text(&default_for(Some("de-DE"))), pseudo);
}

#[test]
fn a_custom_message_is_sent_as_it_is_up_to_the_cap() {
    let notifier = HttpNotifier::new(localizer(), Duration::from_secs(1)).expect("a client");
    let notification = |text: String| Notification {
        destination: app::ports::Destination {
            kind: ChannelKind::Ntfy,
            url: domain::secrets::SecretValue::new("http://127.0.0.1:9/demo"),
            auth: None,
        },
        event: event(EventKind::BackupStarted),
        message: Message::Custom(text),
        locale: None,
    };
    let text = "a".repeat(MAX_MESSAGE_BYTES);
    assert_eq!(notifier.text(&notification(text.clone())), text);
    assert_eq!(notifier.text(&notification(format!("{text}b"))), text);
}

#[test]
fn a_long_message_is_cut_at_a_character_boundary() {
    assert_eq!(capped("demo".to_owned()), "demo");
    // A two-byte character straddles the cap: it is left out whole.
    let text = format!("{}é", "a".repeat(MAX_MESSAGE_BYTES - 1));
    let cut = capped(text);
    assert_eq!(cut.len(), MAX_MESSAGE_BYTES - 1);
    assert!(cut.chars().all(|c| c == 'a'));
}

#[test]
fn a_client_error_says_nothing_but_that_the_client_failed() {
    assert_eq!(
        ClientError.to_string(),
        "the notifications client couldn't be built"
    );
}
