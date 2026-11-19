use std::fmt;
use std::sync::Arc;

use domain::AgentId;
use domain::fleet::{
    Binding, BindingKind, Challenge, Connect, HANDSHAKES, Hostname, HubPin, KEY_BYTES, Nonce, Pair,
    PairAnswer, PairingToken, PublicKey, Refused, SeedFile, SignedMessage, SigningSeed,
};

use crate::ports::{
    Attempt, Identity, IdentityError, Random, RandomUnavailable, Signing, SigningError,
};

/// The agent's side of pairing: it checks the hub against the token before
/// it sends anything of the token, and pins the hub once it is paired.
pub struct AgentPairing {
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
}

/// What the agent keeps of a pairing between its `Connect` and the hub's
/// `Challenge`: the token, its ID and hostname, and its nonce.
pub struct AgentHandshake {
    token: PairingToken,
    agent_id: AgentId,
    hostname: Hostname,
    agent_nonce: Nonce,
    kind: BindingKind,
}

impl fmt::Debug for AgentHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentHandshake")
            .field("agent_id", &self.agent_id)
            .field("hostname", &self.hostname)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// What the agent keeps between its `Pair` and the hub's answer, with the
/// connection attempt whose counter `Pair` carries.
#[derive(Debug)]
pub struct AwaitingPaired {
    handshake: AgentHandshake,
    agent_key: PublicKey,
    hub_nonce: Nonce,
    binding: Binding,
    attempt: Attempt,
}

/// A pairing the hub accepted: the pin the agent keeps, and its own key,
/// whose fingerprint it shows beside the hub's so the person can compare
/// them before approving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedHub {
    pub pin: HubPin,
    pub agent_key: PublicKey,
}

/// Why the agent stopped a pairing. None carries a seed or the token's text
/// (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentPairingError {
    /// The agent pins another hub; replacing the pin needs its person to
    /// confirm, naming both keys (decision 120, "Pairing").
    PinnedToAnotherHub {
        pinned: PublicKey,
        token: PublicKey,
    },
    /// The hub that answered has another key than the token's: nothing of
    /// the token was sent.
    HubKeyMismatch,
    /// The hub's proof isn't the token's hub key's signature over this
    /// connection: nothing of the token was sent.
    HubProofInvalid,
    /// The hub speaks no handshake this build speaks.
    NoCommonHandshake,
    /// The token's address is the hub's own TLS, and this session has no
    /// exporter to bind to.
    BindingUnavailable,
    /// An answer the hub didn't sign: treated as a dropped connection, and
    /// nothing in it is acted on.
    Unsigned,
    /// The hub refused the pairing, signed.
    Refused(Refused),
    Identity(IdentityError),
    Signing(SigningError),
    Random(RandomUnavailable),
}

impl From<IdentityError> for AgentPairingError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<SigningError> for AgentPairingError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

impl From<RandomUnavailable> for AgentPairingError {
    fn from(error: RandomUnavailable) -> Self {
        Self::Random(error)
    }
}

impl AgentPairing {
    pub fn new(
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            identity,
            signing,
            random,
        }
    }

    /// The agent's `Connect` for a pairing with `token` as `agent_id` under
    /// `hostname`. It names the binding kind of the token's address, and
    /// carries nothing of the token.
    pub async fn connect(
        &self,
        _token: PairingToken,
        _agent_id: AgentId,
        _hostname: Hostname,
    ) -> Result<(Connect, AgentHandshake), AgentPairingError> {
        todo!()
    }

    /// The agent's `Pair`, once the hub's `Challenge` proves it holds the
    /// token's hub key on this connection. Nothing signed with the token's
    /// pairing key leaves the agent before that.
    pub async fn answer_challenge(
        &self,
        _handshake: AgentHandshake,
        _challenge: &Challenge,
        _exporter: Option<[u8; KEY_BYTES]>,
    ) -> Result<(Pair, AwaitingPaired), AgentPairingError> {
        todo!()
    }

    /// The hub's answer, checked against the token's hub key. A signed
    /// `Paired` pins the hub, unless the agent pins it already, whose pin
    /// is kept as it is; a signed refusal is the hub's; anything else is a
    /// dropped connection.
    pub async fn take_answer(
        &self,
        _awaiting: AwaitingPaired,
        _answer: &PairAnswer,
    ) -> Result<PairedHub, AgentPairingError> {
        todo!()
    }
}
