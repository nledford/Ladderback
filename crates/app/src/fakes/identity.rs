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
    /// Why every read of a seed fails, for a data folder that can't be
    /// read.
    unreadable: Option<IdentityError>,
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
        let identity = Self::default();
        identity.kept().counter = counter;
        identity
    }

    /// Key files that can't be read: every seed's read fails with `error`.
    pub fn unreadable(error: IdentityError) -> Self {
        Self {
            unreadable: Some(error),
            ..Self::default()
        }
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl Identity for InMemoryIdentity {
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
        if let Some(error) = &self.unreadable {
            return Err(error.clone());
        }
        Ok(self.kept().seeds.get(&file).cloned())
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError> {
        Ok(self.kept().seeds.entry(file).or_insert(seed).clone())
    }

    async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
        self.kept().seeds.remove(&SeedFile::RecoverySeed);
        Ok(())
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
        Ok(self.kept().recovery_key)
    }

    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
        Ok(*self.kept().recovery_key.get_or_insert(key))
    }

    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
        Ok(self.kept().tls_key.clone())
    }

    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
        Ok(self.kept().tls_key.get_or_insert(key).clone())
    }

    async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
        if let Some(error) = &self.unreadable {
            return Err(error.clone());
        }
        Ok(self.kept().pin.clone())
    }

    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
        self.kept().pin = Some(pin.clone());
        Ok(())
    }

    async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
        if self.attempting.swap(true, Ordering::SeqCst) {
            return Err(IdentityError::AttemptInFlight);
        }
        let held = Held(Arc::clone(&self.attempting));
        let mut kept = self.kept();
        let counter = kept.counter.next().ok_or(IdentityError::CounterExhausted)?;
        kept.counter = counter;
        Ok(Attempt::new(counter, Box::new(held)))
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
