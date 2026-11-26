use std::any::Any;
use std::fmt;

use async_trait::async_trait;
use domain::fleet::{ConnectionCounter, HubPin, KeyFile, PublicKey, SeedFile, SigningSeed, TlsKey};

/// The key files of an agent and its hub (§7.1; §16, decision 120, "The hub
/// key" and "A copied data folder"): kept in the data folder, never the
/// database, so `db rebuild` can't change them. A key, once kept, is never
/// replaced by this port but in two ways, each with an order of its own: the
/// hub key's rotation, and a new agent key when the machine takes a new
/// machine ID (decision 154). The seeds come from the `Random` port.
#[async_trait]
pub trait Identity: Send + Sync {
    /// The seed `file` holds, or `None` when there is no such file.
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError>;

    /// Keeps `seed` in `file` unless the file is there already, and answers
    /// the seed the file holds afterwards: `seed`, or the one found, which
    /// is never overwritten. Two callers at once agree on one seed.
    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError>;

    /// Deletes the recovery seed, once the person has confirmed the
    /// Recovery Kit. Done when there is none.
    async fn forget_recovery_seed(&self) -> Result<(), IdentityError>;

    /// Replaces the agent key's seed with `seed`, kept or not, as one
    /// write: a reader finds the old seed or the new one, never part of
    /// either, and a power loss can't roll it back once this has answered.
    /// Only a new machine ID replaces it (decision 154).
    async fn renew_agent_key(&self, seed: SigningSeed) -> Result<(), IdentityError>;

    /// The recovery key's public half, or `None` when there is none.
    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError>;

    /// Keeps `key` as the recovery key's public half unless one is there,
    /// and answers the one kept, as [`Identity::keep_seed`] does.
    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError>;

    /// The hub's TLS key, or `None` when there is none.
    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError>;

    /// Keeps `key` as the hub's TLS key unless one is there, and answers
    /// the one kept, as [`Identity::keep_seed`] does.
    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError>;

    /// The agent's pin of its hub, or `None` before it has paired.
    async fn pin(&self) -> Result<Option<HubPin>, IdentityError>;

    /// Replaces the pin with `pin` as one write: a reader finds the old pin
    /// or the new one, never part of either, and a power loss can't roll
    /// it back once this has answered.
    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError>;

    /// Deletes the pin, as a machine that leaves its hub does (decision
    /// 169). Done when there is none. The other key files stay, the agent
    /// key among them.
    async fn forget_pin(&self) -> Result<(), IdentityError>;

    /// Starts a connection attempt: takes the counter's lock, raises the
    /// counter by one and writes it, durably, before it answers. The
    /// attempt holds the lock until it is dropped, so a second attempt on
    /// the same data folder, in this process or another, is refused with
    /// [`IdentityError::AttemptInFlight`] meanwhile and never sends the
    /// same counter.
    async fn begin_attempt(&self) -> Result<Attempt, IdentityError>;
}

/// One connection attempt's counter, and the lock it holds until dropped.
pub struct Attempt {
    counter: ConnectionCounter,
    _held: Box<dyn Any + Send + Sync>,
}

impl Attempt {
    /// An attempt at `counter` that keeps `held`, the implementation's
    /// lock, until it is dropped.
    pub fn new(counter: ConnectionCounter, held: Box<dyn Any + Send + Sync>) -> Self {
        Self {
            counter,
            _held: held,
        }
    }

    /// The counter this attempt's proof carries.
    pub const fn counter(&self) -> ConnectionCounter {
        self.counter
    }
}

impl fmt::Debug for Attempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attempt")
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

/// Why a key file couldn't be read or written. None carries a key or any
/// of a file's text (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    /// The file is there but doesn't hold what its name says. It is left
    /// as it is, never replaced.
    Damaged { file: KeyFile },
    /// Other users than its owner can read or change the file: `mode` is its
    /// permission bits (`0o644`), without its type. Ladderback writes every
    /// key file `0600`, so another mode means it was changed after, and a
    /// key others can read or replace is no longer this machine's alone
    /// (#1137). It is refused, never used, and left as it is.
    Exposed { file: KeyFile, mode: u32 },
    /// Another attempt holds the counter's lock.
    AttemptInFlight,
    /// The counter is at the most it can hold.
    CounterExhausted,
    /// The operating system refused to read or write the file; `detail` is
    /// its own message.
    Unavailable { file: KeyFile, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An attempt's `Debug` form names its counter and nothing of the lock
    /// it holds.
    #[test]
    fn an_attempt_shows_its_counter_and_nothing_of_its_lock() {
        let attempt = Attempt::new(ConnectionCounter::new(7), Box::new("lock"));
        assert_eq!(
            format!("{attempt:?}"),
            "Attempt { counter: ConnectionCounter(7), .. }"
        );
    }
}
