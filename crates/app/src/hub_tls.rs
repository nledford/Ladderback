//! The key the hub serves its built-in TLS with (§16, decision 120, "The hub
//! key"). The hub makes it on its first start and keeps it in `hub-tls-key`;
//! every agent pins its fingerprint at pairing. The hub never makes a
//! replacement by itself: a new key would lock out every agent that pinned
//! the old one, so a later start that finds the key gone, or another key in
//! its place, is refused and changes nothing.

use std::sync::Arc;

use domain::fleet::{Fingerprint, TlsKey};

use crate::ports::{AgentRegistry, Identity, IdentityError, StoreError, TlsKeyError, TlsKeys};

/// Takes the hub's TLS key at its start.
pub struct HubTlsKey {
    identity: Arc<dyn Identity>,
    keys: Arc<dyn TlsKeys>,
}

/// The key the hub serves its built-in TLS with, and the fingerprint agents
/// pin. Its `Debug` form shows the fingerprint and none of the key (I4).
#[derive(Clone, Debug)]
pub struct ServedTlsKey {
    pub key: TlsKey,
    pub fingerprint: Fingerprint,
}

/// Why the hub can't serve its built-in TLS. None carries any of the key
/// (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsKeyRefusal {
    /// The hub recorded its key at its first start, and `hub-tls-key` is
    /// gone. It makes none in its place.
    Missing,
    /// `hub-tls-key` holds another key than the one the hub recorded. It is
    /// left as it is.
    AnotherKey,
    /// `hub-tls-key` holds no key the hub can serve. It is left as it is.
    NotAKey,
    /// The library couldn't make a key; `detail` is its own message.
    Unavailable { detail: String },
    /// The key file couldn't be read or written.
    Identity(IdentityError),
    /// The registry couldn't say whether an agent may have pinned a key.
    Store(StoreError),
}

impl HubTlsKey {
    pub fn new(identity: Arc<dyn Identity>, keys: Arc<dyn TlsKeys>) -> Self {
        Self { identity, keys }
    }

    /// The key to serve. `recorded` is the fingerprint the hub recorded
    /// when it made its key, `None` before its first start: then a missing
    /// key is made and kept, two first starts at once keep one, and the
    /// caller records the fingerprint answered.
    pub async fn take(&self, recorded: Option<Fingerprint>) -> Result<ServedTlsKey, TlsKeyRefusal> {
        let key = match (self.identity.tls_key().await?, recorded) {
            (Some(key), _) => key,
            (None, Some(_)) => return Err(TlsKeyRefusal::Missing),
            (None, None) => {
                let made = self.keys.generate()?;
                self.identity.keep_tls_key(made).await?
            }
        };
        let fingerprint = self.keys.fingerprint(&key)?;
        match recorded {
            Some(recorded) if recorded != fingerprint => Err(TlsKeyRefusal::AnotherKey),
            _ => Ok(ServedTlsKey { key, fingerprint }),
        }
    }

    /// The key to serve at a start of the hub, checked against the
    /// fingerprint `registry` recorded when the key was made. With no key
    /// in `hub-tls-key` and none recorded, one is made only while
    /// `registry` holds no agent, since an agent may have pinned a key
    /// whose record went with an earlier database. A key made, or found
    /// with none recorded, is recorded (§16, decision 120, "The hub key").
    /// No unused token is voided: with no key recorded, none can pin one.
    pub async fn start(&self, registry: &dyn AgentRegistry) -> Result<ServedTlsKey, TlsKeyRefusal> {
        let recorded = registry.tls_key().await?;
        if recorded.is_none()
            && self.identity.tls_key().await?.is_none()
            && !registry.agents().await?.is_empty()
        {
            return Err(TlsKeyRefusal::Missing);
        }
        let served = self.take(recorded).await?;
        if recorded.is_none() {
            registry.record_tls_key(served.fingerprint).await?;
        }
        Ok(served)
    }
}

impl From<TlsKeyError> for TlsKeyRefusal {
    fn from(error: TlsKeyError) -> Self {
        match error {
            TlsKeyError::NotAKey => Self::NotAKey,
            TlsKeyError::Unavailable { detail } => Self::Unavailable { detail },
        }
    }
}

impl From<IdentityError> for TlsKeyRefusal {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<StoreError> for TlsKeyRefusal {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use domain::fleet::{
        AgentRecord, AgentStatus, Hostname, HubPin, KEY_BYTES, PublicKey, SeedFile, SigningSeed,
        TlsKey,
    };
    use domain::{AgentId, Timestamp, Uuid};

    use super::{HubTlsKey, TlsKeyRefusal};
    use crate::fakes::{FakeTlsKeys, InMemoryAgentRegistry, InMemoryIdentity};
    use crate::ports::{AgentRegistry, Attempt, Identity, IdentityError, TlsKeys};

    /// The key files of a hub whose other first start keeps `theirs` after
    /// this start found no key and before it keeps its own.
    struct RacedBy {
        files: InMemoryIdentity,
        theirs: TlsKey,
    }

    #[async_trait]
    impl Identity for RacedBy {
        async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
            self.files.seed(file).await
        }

        async fn keep_seed(
            &self,
            file: SeedFile,
            seed: SigningSeed,
        ) -> Result<SigningSeed, IdentityError> {
            self.files.keep_seed(file, seed).await
        }

        async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
            self.files.forget_recovery_seed().await
        }

        async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
            self.files.recovery_key().await
        }

        async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
            self.files.keep_recovery_key(key).await
        }

        async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
            self.files.tls_key().await
        }

        async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
            self.files.keep_tls_key(self.theirs.clone()).await?;
            self.files.keep_tls_key(key).await
        }

        async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
            self.files.pin().await
        }

        async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
            self.files.write_pin(pin).await
        }

        async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
            self.files.begin_attempt().await
        }
    }

    /// Two first starts at once serve one key, the one kept first: the
    /// start that lost serves the other's key, never the one it made.
    #[tokio::test]
    async fn a_first_start_another_beat_to_the_key_serves_the_others_key() {
        let keys = Arc::new(FakeTlsKeys::default());
        let theirs = keys.generate().unwrap();
        let identity = Arc::new(RacedBy {
            files: InMemoryIdentity::default(),
            theirs: theirs.clone(),
        });
        let served = HubTlsKey::new(identity.clone(), keys.clone())
            .take(None)
            .await
            .unwrap();
        assert_eq!(keys.made(), 2, "this start made a key of its own");
        assert_eq!(served.key.der(), theirs.der());
        assert_eq!(served.fingerprint, keys.fingerprint(&theirs).unwrap());
        let kept = identity.files.tls_key().await.unwrap().unwrap();
        assert_eq!(kept.der(), theirs.der());
    }

    /// A hub's first start makes its key and keeps it; the next start
    /// serves that key, under the same fingerprint, and makes none.
    #[tokio::test]
    async fn a_start_makes_the_key_once_and_a_restart_serves_it() {
        let keys = Arc::new(FakeTlsKeys::default());
        let identity = Arc::new(InMemoryIdentity::default());
        let registry = InMemoryAgentRegistry::new();
        let use_case = HubTlsKey::new(identity.clone(), keys.clone());
        let first = use_case.start(&registry).await.unwrap();
        let kept = identity.tls_key().await.unwrap().expect("a key kept");
        assert_eq!(first.key.der(), kept.der());
        let again = use_case.start(&registry).await.unwrap();
        assert_eq!(again.fingerprint, first.fingerprint);
        assert_eq!(keys.made(), 1, "the restart made a key");
    }

    /// With an agent registered, a gone key isn't replaced: the agent may
    /// have pinned it.
    #[tokio::test]
    async fn a_start_with_a_registered_agent_makes_no_key_in_place_of_a_gone_one() {
        let keys = Arc::new(FakeTlsKeys::default());
        let identity = Arc::new(InMemoryIdentity::default());
        let registry = InMemoryAgentRegistry::new();
        registry.hold(laptop());
        let refused = HubTlsKey::new(identity.clone(), keys.clone())
            .start(&registry)
            .await
            .unwrap_err();
        assert_eq!(refused, TlsKeyRefusal::Missing);
        assert_eq!(keys.made(), 0);
        assert!(identity.tls_key().await.unwrap().is_none());
    }

    fn laptop() -> AgentRecord {
        let since = Timestamp::from_second(1_800_000_000).unwrap();
        AgentRecord {
            agent_id: AgentId::from_uuid(Uuid::from_u128(7)),
            key: PublicKey::from_bytes([7; KEY_BYTES]).unwrap(),
            hostname: Hostname::parse("laptop").unwrap(),
            status: AgentStatus::Pending { since },
            counter: 0,
            last_seen: since,
        }
    }

    /// The key made is recorded, so a restart once an agent has paired
    /// serves it, though no key could be made then.
    #[tokio::test]
    async fn a_start_records_the_key_it_makes_and_serves_it_once_an_agent_pairs() {
        let keys = Arc::new(FakeTlsKeys::default());
        let registry = InMemoryAgentRegistry::new();
        let use_case = HubTlsKey::new(Arc::new(InMemoryIdentity::default()), keys.clone());
        let first = use_case.start(&registry).await.unwrap();
        assert_eq!(registry.tls_key().await.unwrap(), Some(first.fingerprint));
        registry.hold(laptop());
        let again = use_case.start(&registry).await.unwrap();
        assert_eq!(again.fingerprint, first.fingerprint);
    }

    /// Once a key is recorded, another key in the file is refused, and so
    /// is no key, with no agent registered too: the hub never makes a
    /// replacement by itself.
    #[tokio::test]
    async fn a_start_serves_only_the_recorded_key() {
        let keys = Arc::new(FakeTlsKeys::default());
        let recorded = keys.generate().unwrap();
        let registry = InMemoryAgentRegistry::new();
        registry
            .record_tls_key(keys.fingerprint(&recorded).unwrap())
            .await
            .unwrap();
        let identity = Arc::new(InMemoryIdentity::default());
        let use_case = HubTlsKey::new(identity.clone(), keys.clone());
        assert_eq!(
            use_case.start(&registry).await.unwrap_err(),
            TlsKeyRefusal::Missing
        );
        assert_eq!(
            keys.made(),
            1,
            "a key was made in place of the recorded one"
        );
        identity
            .keep_tls_key(keys.generate().unwrap())
            .await
            .unwrap();
        assert_eq!(
            use_case.start(&registry).await.unwrap_err(),
            TlsKeyRefusal::AnotherKey
        );
    }
}
