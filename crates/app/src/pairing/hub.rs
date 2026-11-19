use std::fmt;
use std::sync::Arc;

use domain::fleet::{
    Binding, BindingKind, Challenge, Connect, EpochSeconds, HANDSHAKES, Holder, HubAddress,
    KEY_BYTES, KeyFile, Nonce, Pair, PairAnswer, PairRefusal, PairRequest, Paired, PairingToken,
    PublicKey, Refused, SeedFile, SignedMessage, SigningSeed, TokenId, TokenRecord, TokenTls,
    TokenUse, UNUSED_TOKEN_LIMIT, token_life,
};
use domain::{AgentId, SignedDuration};

use crate::ports::{
    Added, AgentRegistry, Clock, Identity, IdentityError, Random, RandomUnavailable, Signing,
    SigningError, StoreError,
};

/// The hub's side of pairing: its keys, its tokens, and its answers to an
/// agent's `Connect` and `Pair`.
pub struct HubPairing {
    registry: Arc<dyn AgentRegistry>,
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
}

/// The hub key, with its seed, and the recovery key's public half, read
/// once at the hub's start.
pub struct HubKeys {
    seed: SigningSeed,
    key: PublicKey,
    recovery_key: PublicKey,
}

impl HubKeys {
    /// The hub key every agent pins.
    pub const fn key(&self) -> &PublicKey {
        &self.key
    }

    /// The recovery key's public half, which `Paired` gives the agent.
    pub const fn recovery_key(&self) -> &PublicKey {
        &self.recovery_key
    }
}

impl fmt::Debug for HubKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubKeys")
            .field("key", &self.key)
            .field("recovery_key", &self.recovery_key)
            .finish_non_exhaustive()
    }
}

/// What the hub keeps of one connection between its `Challenge` and the
/// agent's next message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubHandshake {
    agent_id: AgentId,
    agent_nonce: Nonce,
    hub_nonce: Nonce,
    kind: BindingKind,
    binding: Binding,
}

/// Why the hub couldn't take a step. None carries a key, a seed or a
/// token's text (I4); the transport drops the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubPairingError {
    /// A key file is gone while agents are registered: a new key would
    /// lock out every agent that pinned the old one, so none is made.
    KeyMissing {
        file: KeyFile,
    },
    /// A token's life outside 5 minutes to 24 hours was asked for.
    TokenLife,
    /// As many tokens as the limit allows are unused; one must be used,
    /// lapse or be revoked first.
    TooManyUnusedTokens,
    /// The agent named binding kind 1 on a session that has no exporter.
    BindingUnavailable,
    Identity(IdentityError),
    Registry(StoreError),
    Signing(SigningError),
    Random(RandomUnavailable),
}

impl From<IdentityError> for HubPairingError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<StoreError> for HubPairingError {
    fn from(error: StoreError) -> Self {
        Self::Registry(error)
    }
}

impl From<SigningError> for HubPairingError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

impl From<RandomUnavailable> for HubPairingError {
    fn from(error: RandomUnavailable) -> Self {
        Self::Random(error)
    }
}

impl HubPairing {
    pub fn new(
        registry: Arc<dyn AgentRegistry>,
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            identity,
            signing,
            random,
            clock,
        }
    }

    /// The hub's keys: the hub key and the recovery key, made while no
    /// agent is registered and read from the data folder after that (§16,
    /// decision 120, "The key's life").
    pub async fn keys(&self) -> Result<HubKeys, HubPairingError> {
        todo!()
    }

    /// A pairing token for `address`, of `tls`'s kind, living `life` or 15
    /// minutes.
    pub async fn make_token(
        &self,
        _keys: &HubKeys,
        _address: HubAddress,
        _tls: TokenTls,
        _life: Option<SignedDuration>,
    ) -> Result<PairingToken, HubPairingError> {
        todo!()
    }

    /// Revokes the unused token `id`, and answers whether there was one.
    pub async fn revoke_token(&self, _id: TokenId) -> Result<bool, HubPairingError> {
        todo!()
    }

    /// The hub's answer to `connect` on a session whose `tls-exporter`
    /// value is `exporter`: a fresh nonce and its proof over the
    /// connection.
    pub fn challenge(
        &self,
        _keys: &HubKeys,
        _connect: &Connect,
        _exporter: Option<[u8; KEY_BYTES]>,
    ) -> Result<(Challenge, HubHandshake), HubPairingError> {
        todo!()
    }

    /// The hub's answer to `pair`.
    pub async fn pair(
        &self,
        _keys: &HubKeys,
        _handshake: &HubHandshake,
        _pair: &Pair,
    ) -> Result<PairAnswer, HubPairingError> {
        todo!()
    }
}
