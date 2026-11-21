use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use api::sync::{AgentFrame, HubFrame, MAX_FRAME_BYTES};
use async_trait::async_trait;
use domain::fleet::{HubRoute, KEY_BYTES, RouteSession};

use crate::pairing::{HubPairing, HubSession, HubStep};
use crate::ports::{HubConnector, HubLink, LinkError};

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
}

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
}

#[async_trait]
impl HubConnector for LoopbackHubConnector {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        self.tap
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(route.clone());
        let answers = self
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
        Ok(Box::new(LoopLink {
            hub,
            exporter,
            tap: self.tap.clone(),
            session,
            inbox: VecDeque::new(),
            closed: false,
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
    closed: bool,
    reason: Option<String>,
}

impl LoopLink {
    fn close(&mut self, reason: Option<&str>) {
        self.closed = true;
        self.reason = reason.map(str::to_owned);
    }
}

#[async_trait]
impl HubLink for LoopLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.exporter
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        if self.closed {
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
        let Some(session) = self.session.as_mut() else {
            self.close(None);
            return Ok(());
        };
        let read = match AgentFrame::from_text(&text) {
            Ok(read) if text.len() <= MAX_FRAME_BYTES => read,
            _ => {
                self.close(None);
                return Ok(());
            }
        };
        match self.hub.step(session, &read).await {
            HubStep::Reply(answer) => self.inbox.push_back(answer.to_text()),
            HubStep::Wait => {}
            HubStep::Last(answer) => {
                self.inbox.push_back(answer.to_text());
                self.closed = true;
            }
            HubStep::Close(reason) => self.close(Some(reason)),
            HubStep::Fail(_) => self.close(None),
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
