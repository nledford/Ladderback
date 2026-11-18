use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domain::fleet::{ConnectionCounter, HubPin, PublicKey, SeedFile, SigningSeed, TlsKey};

use crate::ports::{Attempt, Identity, IdentityError};

/// Key files kept in memory, as the data folder would keep them: a key once
/// kept is never replaced, and one attempt holds the counter at a time.
#[derive(Debug, Default)]
pub struct InMemoryIdentity {
    kept: Mutex<Kept>,
    /// Whether an attempt holds the counter's lock.
    attempting: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct Kept {
    seeds: HashMap<SeedFile, SigningSeed>,
    recovery_key: Option<PublicKey>,
    tls_key: Option<TlsKey>,
    pin: Option<HubPin>,
    counter: ConnectionCounter,
}

/// The counter's lock, let go when the attempt that holds it is dropped.
struct Held(Arc<AtomicBool>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl InMemoryIdentity {
    /// Key files whose counter stands at `counter`, as a data folder copied
    /// after that many attempts would.
    pub fn with_counter(counter: ConnectionCounter) -> Self {
        let _ = counter;
        todo!()
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl Identity for InMemoryIdentity {
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
        let _ = (file, self.kept());
        todo!()
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError> {
        let _ = (file, seed);
        todo!()
    }

    async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
        todo!()
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
        todo!()
    }

    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
        let _ = key;
        todo!()
    }

    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
        todo!()
    }

    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
        let _ = key;
        todo!()
    }

    async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
        todo!()
    }

    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
        let _ = pin;
        todo!()
    }

    async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
        let _ = (&self.attempting, Held, Attempt::new);
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_counter_at_the_most_it_holds_starts_no_attempt() {
        let identity = InMemoryIdentity::with_counter(ConnectionCounter::new(u64::MAX - 1));
        let last = identity.begin_attempt().await.unwrap();
        assert_eq!(last.counter(), ConnectionCounter::new(u64::MAX));
        drop(last);
        assert_eq!(
            identity.begin_attempt().await.unwrap_err(),
            IdentityError::CounterExhausted
        );
        assert_eq!(
            identity.begin_attempt().await.unwrap_err(),
            IdentityError::CounterExhausted,
            "a refused attempt holds no lock"
        );
    }
}
