//! The notifier hears the daemon's news on the event bus (technical design
//! §4.4, §11). A subscriber's queue holds at most `EVENT_BUFFER` events and,
//! once it holds no progress, loses its oldest to make room, so a notifier
//! that stopped reading while a channel was slow would lose the start or
//! the end of another run to a stream of warnings. It reads on while it
//! waits, and tells every start and end in the order they came.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use api::{UiEvent, WarningCode};
use app::announce::{end_of, start_of};
use app::event_bus::BroadcastEventBus;
use app::fakes::{FakeSecrets, InMemoryConfigStore, InMemoryOperationStore, RecordingNotifier};
use app::notifications::Notify;
use app::ports::{
    ConfigStore, EVENT_BUFFER, EventBus, Notification, Notifier, NotifyError, OperationStore,
};
use async_trait::async_trait;
use domain::execution::{FailureReason, Operation, OperationKind, Target, Trigger};
use domain::notifications::{
    Auth, Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName,
};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, Source,
};
use domain::secrets::{FileRead, SecretRef, SecretWarning};
use domain::{OperationId, PlanId, RepositoryAlias, Timestamp, Uuid};
use tokio::sync::watch;

/// A channel that holds each delivery until it is opened, counting the
/// deliveries that have arrived.
struct Slow {
    inner: RecordingNotifier,
    arrived: AtomicUsize,
    open: watch::Sender<bool>,
}

#[async_trait]
impl Notifier for Slow {
    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let mut open = self.open.subscribe();
        open.wait_for(|open| *open)
            .await
            .expect("the test holds the sender");
        self.inner.deliver(notification).await
    }
}

/// Lets the runtime run until `done`, or for a few turns. Everything the
/// fakes do is ready at once, so a few turns are all a step takes, and a
/// step that never comes fails the test at once instead of hanging it.
async fn turns_until(done: impl Fn() -> bool) {
    for _ in 0..100 {
        if done() {
            return;
        }
        tokio::task::yield_now().await;
    }
}

fn t(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).expect("a time")
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("nas").expect("an alias")
}

fn plan() -> PlanId {
    PlanId::parse("home").expect("a plan ID")
}

/// The configuration: the plan "home", and a rule "ops" that sends its
/// backups' starts and failures to the webhook "ops".
fn config() -> AgentConfig {
    config_sending_to(
        Channel::new(
            ChannelKind::Webhook,
            SecretRef::Env("DEMO_OPS_URL".to_owned()),
            None,
        )
        .expect("a channel"),
    )
}

/// [`config`], its channel "ops" being `ops`.
fn config_sending_to(ops: Channel) -> AgentConfig {
    let repository = RepositoryConfig {
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
        maintenance: Maintenance::default(),
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let source = Source::parse("/home/demo/home").expect("a source");
    let channel = ChannelName::parse("ops").expect("a channel name");
    let notifications = Notifications::new(
        BTreeMap::from([(channel.clone(), ops)]),
        BTreeMap::from([(
            RuleName::parse("ops").expect("a rule name"),
            Rule::new(
                BTreeSet::from([EventKind::BackupStarted, EventKind::BackupFailed]),
                channel,
                BTreeSet::new(),
                None,
            )
            .expect("a rule"),
        )]),
    )
    .expect("the rule sends to its channel");
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(alias(), repository)]),
        vec![Plan::new(plan(), alias(), vec![source])],
    )
    .and_then(|config| config.with_notifications(notifications))
    .expect("a valid configuration")
}

/// A backup of "home" that started at `at`.
fn backup(number: u128, at: Timestamp) -> Operation {
    let mut operation = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(number)),
        OperationKind::Backup,
        Trigger::Manual,
        Target {
            plan: Some(plan()),
            repository: None,
        },
        at,
    );
    operation.start(at).expect("a queued backup starts");
    operation
}

#[tokio::test]
async fn an_end_that_comes_while_a_channel_is_slow_is_told_after_any_number_of_warnings() {
    let store = InMemoryConfigStore::new();
    store.save(&config()).await.expect("saved");
    let secrets = FakeSecrets::new();
    secrets.set_env("DEMO_OPS_URL", "http://127.0.0.1/ops");
    let operations = Arc::new(InMemoryOperationStore::new());
    let slow = Arc::new(Slow {
        inner: RecordingNotifier::new(),
        arrived: AtomicUsize::new(0),
        open: watch::channel(false).0,
    });
    let first = backup(1, t(0));
    let mut second = backup(2, t(1));
    second
        .fail(
            FailureReason::EngineFailed {
                detail: "repository locked".to_owned(),
            },
            t(2),
        )
        .expect("a running backup fails");
    for operation in [&first, &second] {
        operations
            .save(operation, None)
            .await
            .expect("the log takes it");
    }
    let notify = Notify::new(Arc::new(store), operations, Arc::new(secrets), slow.clone());
    let bus = BroadcastEventBus::new();
    let news = bus.subscribe().await;
    tokio::spawn(async move { notify.run(news, |_| {}).await });

    // The first backup's start is held at its channel.
    bus.publish(start_of(&first, None).expect("news of a start"))
        .await;
    turns_until(|| slow.arrived.load(Ordering::SeqCst) == 1).await;
    assert_eq!(
        slow.arrived.load(Ordering::SeqCst),
        1,
        "the first start reached its channel"
    );
    // The second backup fails meanwhile, and the first warns more often than
    // a queue holds events.
    bus.publish(end_of(&second).expect("news of an end")).await;
    tokio::task::yield_now().await;
    for _ in 0..EVENT_BUFFER {
        bus.publish(UiEvent::OperationWarning {
            operation: first.id().as_uuid().hyphenated().to_string(),
            code: WarningCode::SourceUnreadable,
            args: Vec::new(),
        })
        .await;
    }
    slow.open.send_replace(true);
    turns_until(|| slow.inner.delivered().len() >= 2).await;

    let told: Vec<(EventKind, OperationId)> = slow
        .inner
        .delivered()
        .iter()
        .map(|notification| (notification.event.kind, notification.event.operation))
        .collect();
    assert_eq!(
        told,
        [
            (EventKind::BackupStarted, first.id()),
            (EventKind::BackupFailed, second.id()),
        ]
    );
}

/// A channel's secret read from a file other users can read is told by the
/// file's path, masked of every secret the delivery read (I4, #386), and
/// told too when a later secret of the channel can't be read.
#[tokio::test]
async fn a_channels_readable_secret_file_is_told_by_its_masked_path() {
    let told = Arc::new(Mutex::new(Vec::new()));
    let notify = |channel: Channel, secrets: FakeSecrets| {
        let store = InMemoryConfigStore::new();
        let heard = Arc::clone(&told);
        async move {
            store
                .save(&config_sending_to(channel))
                .await
                .expect("saved");
            let operations = Arc::new(InMemoryOperationStore::new());
            operations
                .save(&backup(1, t(0)), None)
                .await
                .expect("the log takes it");
            Notify::new(
                Arc::new(store),
                operations,
                Arc::new(secrets),
                Arc::new(RecordingNotifier::new()),
            )
            .telling(Arc::new(move |warning| {
                heard.lock().expect("the list").push(warning.clone());
            }))
        }
    };
    let readable = |path: &str| SecretWarning::FileReadableByOthers {
        path: path.to_owned(),
        mode: 0o644,
    };
    let file = |path: &str| SecretRef::File {
        path: path.to_owned(),
        read: FileRead::FirstLine,
    };

    // The token's file is named as the token is, by chance.
    let secrets = FakeSecrets::new();
    secrets.set_env("DEMO_OPS_URL", "http://127.0.0.1/ops");
    secrets.write_file("/home/demo/tokens/demo-token", b"demo-token\n", 0o644);
    let channel = Channel::new(
        ChannelKind::Webhook,
        SecretRef::Env("DEMO_OPS_URL".to_owned()),
        Some(Auth::Bearer(file("/home/demo/tokens/demo-token"))),
    )
    .expect("a channel");
    let reports = notify(channel, secrets)
        .await
        .tell(start_of(&backup(1, t(0)), None).expect("news of a start"))
        .await;
    assert_eq!(reports, []);
    assert_eq!(
        *told.lock().expect("the list"),
        [readable("/home/demo/tokens/***")]
    );

    // The URL's file is read, and the token can't be.
    told.lock().expect("the list").clear();
    let secrets = FakeSecrets::new();
    secrets.write_file("/home/demo/ops-url", b"http://127.0.0.1/ops\n", 0o644);
    let channel = Channel::new(
        ChannelKind::Webhook,
        file("/home/demo/ops-url"),
        Some(Auth::Bearer(SecretRef::Env(
            "DEMO_MISSING_TOKEN".to_owned(),
        ))),
    )
    .expect("a channel");
    let reports = notify(channel, secrets)
        .await
        .tell(start_of(&backup(1, t(0)), None).expect("news of a start"))
        .await;
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(
        *told.lock().expect("the list"),
        [readable("/home/demo/ops-url")]
    );
}
