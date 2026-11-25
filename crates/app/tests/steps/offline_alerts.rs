//! Steps for `features/fleet/offline_alerts.feature`: the hub watching its
//! machines every 30 seconds on its clock and telling its notification
//! rules when one has stayed offline 15 minutes, and when one it alerted
//! came back (technical design §7.2, §11; §16, decision 168). The machine
//! goes offline as `staying_connected.feature`'s does: it stops sending,
//! and the hub closes its connection 90 seconds later.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use app::fakes::{FakeSecrets, InMemoryConfigStore, InMemoryOperationStore, RecordingNotifier};
use app::notifications::Notify;
use app::pairing::{MACHINES_WATCHED_EVERY, WatchMachines};
use app::ports::{Clock as _, ConfigStore, Notification};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::notifications::{
    Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName,
};
use domain::planning::{AgentConfig, AgentSettings, Hooks};
use domain::secrets::SecretRef;

use super::Agent;
use super::staying_connected::is_connected;
use super::sync_replay::{first, keeps_syncing};

/// The webhook the hub's rule sends to.
const OPS_URL: &str = "https://ops.example/hook";

/// The hub's notification rules and what they were sent.
pub(super) struct Alerting {
    notify: Arc<Notify>,
    notifier: Arc<RecordingNotifier>,
}

fn alerting(agent: &Agent) -> &Alerting {
    agent
        .pairing
        .alerts
        .as_ref()
        .expect("the hub has a notification rule")
}

/// What the hub's rules were sent, in order.
fn sent(agent: &Agent) -> Vec<Notification> {
    alerting(agent).notifier.delivered()
}

#[given(regex = r#"^the hub has a rule that sends "([^"]+)" to the webhook channel "([^"]+)"$"#)]
async fn hub_rule(agent: &mut Agent, events: String, channel: String) {
    let kinds: BTreeSet<EventKind> = events
        .split(", ")
        .map(|name| EventKind::parse(name).expect("an event"))
        .collect();
    let channel = ChannelName::parse(&channel).unwrap();
    let secrets = Arc::new(FakeSecrets::new());
    secrets.set_env("DEMO_OPS_URL", OPS_URL);
    let notifications = Notifications::new(
        BTreeMap::from([(
            channel.clone(),
            Channel::new(
                ChannelKind::Webhook,
                SecretRef::Env("DEMO_OPS_URL".to_owned()),
                None,
            )
            .unwrap(),
        )]),
        BTreeMap::from([(
            RuleName::parse("machines").unwrap(),
            Rule::new(kinds, channel, BTreeSet::new(), None).unwrap(),
        )]),
    )
    .unwrap();
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::new(),
        vec![],
    )
    .unwrap()
    .with_notifications(notifications)
    .unwrap();
    let store = InMemoryConfigStore::new();
    store.save(&config).await.unwrap();
    let notifier = Arc::new(RecordingNotifier::new());
    let notify = Arc::new(Notify::new(
        Arc::new(store),
        Arc::new(InMemoryOperationStore::new()),
        secrets,
        notifier.clone(),
    ));
    agent.pairing.alerts = Some(Alerting { notify, notifier });
}

/// The hub watches its machines once, as its daemon does every 30
/// seconds, and nothing was left to report.
async fn watch(agent: &Agent) {
    let watch = WatchMachines::new(
        agent.pairing.hub().pairing.clone(),
        alerting(agent).notify.clone(),
    );
    assert_eq!(watch.watch().await, vec![]);
}

#[when("the hub watches its machines")]
async fn watches(agent: &mut Agent) {
    watch(agent).await;
}

#[when(regex = r"^the hub watches its machines for (\d+) (?:more )?minutes?$")]
async fn watches_for(agent: &mut Agent, minutes: i64) {
    let watches = SignedDuration::from_mins(minutes).as_secs() / MACHINES_WATCHED_EVERY.as_secs();
    for _ in 0..watches {
        agent.pairing.hub().clock.advance(MACHINES_WATCHED_EVERY);
        first(agent).connector.close_silent().await;
        watch(agent).await;
    }
}

#[when("the agent starts again and connects to its hub")]
async fn starts_again(agent: &mut Agent) {
    keeps_syncing(agent).await;
    is_connected(agent).await;
}

#[when("the hub's daemon restarts")]
async fn hub_restarts(agent: &mut Agent) {
    let restarted = agent.pairing.hub().restarted().await;
    agent.pairing.hub = Some(restarted);
}

#[then(regex = r#"^"([^"]+)" was sent nothing$"#)]
fn sent_nothing(agent: &mut Agent, _channel: String) {
    let sent = sent(agent);
    assert!(sent.is_empty(), "{sent:?}");
}

/// Each notification was sent to the one webhook of the hub's rule.
fn to_ops(sent: &[Notification]) {
    for notification in sent {
        assert_eq!(notification.destination.url.expose(), OPS_URL);
    }
}

#[then(regex = r#"^"([^"]+)" was sent "([^"]+)" for "([^"]+)", offline since (\d+) minutes ago$"#)]
fn sent_offline(
    agent: &mut Agent,
    _channel: String,
    event: String,
    hostname: String,
    minutes: i64,
) {
    let sent = sent(agent);
    to_ops(&sent);
    let [notification] = sent.as_slice() else {
        panic!("one notification: {sent:?}");
    };
    let since = agent
        .pairing
        .hub()
        .clock
        .now()
        .checked_sub(SignedDuration::from_mins(minutes))
        .unwrap();
    assert_eq!(notification.event.kind.name(), event);
    assert_eq!(
        notification
            .event
            .machine
            .as_ref()
            .map(|machine| machine.as_str()),
        Some(hostname.as_str())
    );
    assert_eq!(notification.event.at, since);
}

#[then(regex = r#"^"([^"]+)" was sent only "([^"]+)"$"#)]
fn sent_only(agent: &mut Agent, _channel: String, event: String) {
    let sent = sent(agent);
    let kinds: Vec<&str> = sent.iter().map(|sent| sent.event.kind.name()).collect();
    assert_eq!(kinds, [event.as_str()]);
}

#[then(regex = r#"^"([^"]+)" was sent "([^"]+)", then "([^"]+)" for "([^"]+)", back online now$"#)]
fn sent_offline_then_online(
    agent: &mut Agent,
    _channel: String,
    offline: String,
    online: String,
    hostname: String,
) {
    let sent = sent(agent);
    to_ops(&sent);
    let kinds: Vec<&str> = sent.iter().map(|sent| sent.event.kind.name()).collect();
    assert_eq!(kinds, [offline.as_str(), online.as_str()]);
    let back = &sent[1].event;
    assert_eq!(
        back.machine.as_ref().map(|machine| machine.as_str()),
        Some(hostname.as_str())
    );
    assert_eq!(back.at, agent.pairing.hub().clock.now());
}
