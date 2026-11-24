use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use api::sync::{AgentFrame, HubFrame, MAX_FRAME_BYTES};
use async_trait::async_trait;
use domain::fleet::{HubRoute, KEY_BYTES, RouteSession, SILENCE_LIMIT};
use domain::{AgentId, SignedDuration};

use crate::pairing::{HubPairing, HubSession, HubStep};
use crate::ports::{Clock, HubConnector, HubLink, LinkError};

/// The `tls-exporter` value both ends of a pinned link see.
pub const LOOPBACK_EXPORTER: [u8; KEY_BYTES] = [0xee; KEY_BYTES];

/// A connector to a hub in this process, as `/sync/v1` carries its frames:
/// each frame goes through its text to the hub's session and the answer
/// back, a frame over [`MAX_FRAME_BYTES`] ends the link, and a pinned route
/// has [`LOOPBACK_EXPORTER`] as its session's exporter. With no hub, nothing
/// answers. It keeps the routes it was asked for and the text of every frame
/// it carried to the hub, so a test can tell what left the machine.
pub struct LoopbackHubConnector {
    hub: Option<Arc<HubPairing>>,
    hub_binds: bool,
    /// The one host and port the hub answers on, when it answers on one.
    only: Option<(String, u16)>,
    tap: Arc<Tap>,
}

#[derive(Default)]
struct Tap {
    routes: Mutex<Vec<HubRoute>>,
    sent: Mutex<Vec<String>>,
    /// Whether nothing answers on any route for now, as a hub that is down.
    down: AtomicBool,
    /// How many of the hub's next acknowledgements are lost, each with its
    /// connection.
    losing_acks: AtomicUsize,
    /// Whether each refusal reaches the agent with a signature the hub
    /// didn't make.
    forging: AtomicBool,
    /// Every link the connector opened, in order, as the hub's transport
    /// holds it.
    links: Mutex<Vec<Arc<Held>>>,
    /// The hub's clock, by which a link that sent nothing for
    /// [`SILENCE_LIMIT`] is closed, when the connector watches for it.
    clock: Mutex<Option<Arc<dyn Clock>>>,
}

impl Tap {
    /// Now on the hub's clock, when the connector watches it.
    fn heard_now(&self) -> SignedDuration {
        self.clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or(SignedDuration::ZERO, |clock| clock.monotonic())
    }
}

/// One link as the hub's transport holds it (decision 159): its number,
/// the approved machine it syncs for once it does, whether the hub closed
/// it, and when on the hub's clock it last heard a frame.
struct Held {
    connection: u64,
    live: Mutex<Option<AgentId>>,
    closed: AtomicBool,
    heard: Mutex<SignedDuration>,
}

impl Held {
    /// Closes the link from the hub's side and tells the hub it went away,
    /// once.
    async fn ended(&self, hub: &HubPairing) {
        let live = *self.live.lock().unwrap_or_else(PoisonError::into_inner);
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(agent) = live {
            hub.went_away(agent, self.connection).await;
        }
    }
}

/// The number of the next link any connector opens, as the hub's
/// transport numbers its connections.
static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

impl LoopbackHubConnector {
    /// A connector whose every route reaches `hub`.
    pub fn to(hub: Arc<HubPairing>) -> Self {
        Self {
            hub: Some(hub),
            hub_binds: true,
            only: None,
            tap: Arc::default(),
        }
    }

    /// A connector on whose routes nothing answers.
    pub fn unanswered() -> Self {
        Self {
            hub: None,
            hub_binds: true,
            only: None,
            tap: Arc::default(),
        }
    }

    /// The hub sees no exporter on any link, as behind a proxy that ends
    /// TLS: the agent's pinned session ends at the proxy.
    #[must_use]
    pub fn behind_a_proxy(mut self) -> Self {
        self.hub_binds = false;
        self
    }

    /// The hub answers only on `host` and `port`, as a hub that moved to a
    /// new address no longer answers at its old one; nothing answers on
    /// any other route.
    #[must_use]
    pub fn only_at(mut self, host: &str, port: u16) -> Self {
        self.only = Some((host.to_owned(), port));
        self
    }

    /// The routes connected to, in order.
    pub fn routes(&self) -> Vec<HubRoute> {
        self.tap
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The text of every frame sent to the hub, in order.
    pub fn sent(&self) -> Vec<String> {
        self.tap
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Nothing answers on any route while `down`, as a hub that is down or
    /// out of reach, until it is set back.
    pub fn set_down(&self, down: bool) {
        self.tap.down.store(down, Ordering::SeqCst);
    }

    /// The hub's next `count` acknowledgements are lost, each with the
    /// connection it was sent on, as when the link drops just after the hub
    /// took a report.
    pub fn lose_acks(&self, count: usize) {
        self.tap.losing_acks.store(count, Ordering::SeqCst);
    }

    /// Each refusal from now on reaches the agent with a signature the hub
    /// didn't make, as one from anyone but the hub it paired with.
    pub fn forge_refusals(&self) {
        self.tap.forging.store(true, Ordering::SeqCst);
    }

    /// The connector closing each link that sent nothing for
    /// [`SILENCE_LIMIT`] by `clock`, the hub's, when asked to
    /// ([`Self::close_silent`]), as the hub's transport does.
    #[must_use]
    pub fn watching(self, clock: Arc<dyn Clock>) -> Self {
        *self
            .tap
            .clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(clock);
        self
    }

    /// The hub dropping every open link, as a network that went away or a
    /// hub that stopped: the agent finds each closed at its next frame.
    pub async fn drop_links(&self) {
        let Some(hub) = &self.hub else {
            return;
        };
        for held in self.held() {
            held.ended(hub).await;
        }
    }

    /// The hub closing each open link that sent nothing for
    /// [`SILENCE_LIMIT`] by the clock it watches ([`Self::watching`]).
    pub async fn close_silent(&self) {
        let clock = self
            .tap
            .clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let (Some(hub), Some(clock)) = (&self.hub, clock) else {
            return;
        };
        let now = clock.monotonic();
        for held in self.held() {
            let heard = *held.heard.lock().unwrap_or_else(PoisonError::into_inner);
            if now - heard >= SILENCE_LIMIT {
                held.ended(hub).await;
            }
        }
    }

    /// How many links the hub holds open.
    pub fn open_links(&self) -> usize {
        self.held()
            .iter()
            .filter(|held| !held.closed.load(Ordering::SeqCst))
            .count()
    }

    fn held(&self) -> Vec<Arc<Held>> {
        self.tap
            .links
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl HubConnector for LoopbackHubConnector {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        self.tap
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(route.clone());
        let answers = !self.tap.down.load(Ordering::SeqCst)
            && self
                .only
                .as_ref()
                .is_none_or(|(host, port)| *host == route.host && *port == route.port);
        let Some(hub) = self.hub.clone().filter(|_| answers) else {
            return Err(LinkError::Unreachable {
                detail: "nothing answers on this route".to_owned(),
            });
        };
        let exporter =
            matches!(route.session, RouteSession::Pinned(_)).then_some(LOOPBACK_EXPORTER);
        let session = hub.session(exporter.filter(|_| self.hub_binds)).await.ok();
        let held = Arc::new(Held {
            connection: NEXT_CONNECTION.fetch_add(1, Ordering::SeqCst),
            live: Mutex::new(None),
            closed: AtomicBool::new(false),
            heard: Mutex::new(self.tap.heard_now()),
        });
        self.tap
            .links
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(held.clone());
        Ok(Box::new(LoopLink {
            hub,
            exporter,
            tap: self.tap.clone(),
            session,
            inbox: VecDeque::new(),
            held,
            reason: None,
        }))
    }
}

struct LoopLink {
    hub: Arc<HubPairing>,
    exporter: Option<[u8; KEY_BYTES]>,
    tap: Arc<Tap>,
    /// `None` once the hub couldn't open one.
    session: Option<HubSession>,
    /// What the hub sent that the agent hasn't read, in the order sent.
    inbox: VecDeque<String>,
    /// The link as the hub holds it: its `closed` is the hub's side.
    held: Arc<Held>,
    reason: Option<String>,
}

impl LoopLink {
    /// The hub closes the link, `reason` its close reason.
    async fn close(&mut self, reason: Option<&str>) {
        self.reason = reason.map(str::to_owned);
        self.held.ended(&self.hub).await;
    }

    fn closed(&self) -> bool {
        self.held.closed.load(Ordering::SeqCst)
    }

    /// `answer` as it reaches the agent: a refusal with a signature the hub
    /// didn't make, while the connector forges them.
    fn delivered(&self, answer: HubFrame) -> String {
        match answer {
            HubFrame::Refused { code, holder, .. } if self.tap.forging.load(Ordering::SeqCst) => {
                HubFrame::Refused {
                    code,
                    holder,
                    signature: "A".repeat(86),
                }
                .to_text()
            }
            answer => answer.to_text(),
        }
    }
}

#[async_trait]
impl HubLink for LoopLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.exporter
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        if self.closed() {
            return Err(LinkError::Closed {
                reason: self.reason.clone(),
            });
        }
        let text = frame.to_text();
        self.tap
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(text.clone());
        *self
            .held
            .heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = self.tap.heard_now();
        let Some(session) = self.session.as_mut() else {
            self.close(None).await;
            return Ok(());
        };
        let read = match AgentFrame::from_text(&text) {
            Ok(read) if text.len() <= MAX_FRAME_BYTES => read,
            _ => {
                self.close(None).await;
                return Ok(());
            }
        };
        let step = self.hub.step(session, &read).await;
        let went_live = session.syncing_agent().filter(|_| {
            let mut live = self
                .held
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let first = live.is_none();
            if first {
                *live = session.syncing_agent();
            }
            first
        });
        if let Some(agent) = went_live {
            let held = self.held.clone();
            let close = Box::new(move || held.closed.store(true, Ordering::SeqCst));
            self.hub.went_live(agent, self.held.connection, close).await;
        }
        match step {
            HubStep::Reply(answer) => {
                let lost = matches!(&answer, HubFrame::Signed { kind, .. } if kind == "Ack")
                    && self
                        .tap
                        .losing_acks
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                            left.checked_sub(1)
                        })
                        .is_ok();
                if lost {
                    self.close(None).await;
                } else {
                    let delivered = self.delivered(answer);
                    self.inbox.push_back(delivered);
                }
            }
            HubStep::Wait => {}
            HubStep::Last(answer) => {
                let delivered = self.delivered(answer);
                self.inbox.push_back(delivered);
                self.close(None).await;
            }
            HubStep::Close(reason) => self.close(Some(reason)).await,
            HubStep::Fail(_) => self.close(None).await,
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        match self.inbox.pop_front() {
            Some(text) if text.len() <= MAX_FRAME_BYTES => {
                HubFrame::from_text(&text).map_err(|_| LinkError::Unreadable)
            }
            Some(_) => Err(LinkError::Unreadable),
            None => Err(LinkError::Closed {
                reason: self.reason.clone(),
            }),
        }
    }
}
