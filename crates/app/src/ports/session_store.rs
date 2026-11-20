use async_trait::async_trait;
use domain::Timestamp;
use domain::access::{KeptSession, SessionDigest, SessionLength};

use crate::ports::StoreError;

/// Keeps the admin's sessions, so they outlive a restart (technical design
/// §10.1, §9 `sessions`; decision 70): each one's token digest, the admin
/// password it started under, and when it started and was last used. Never a
/// token: the store is given none, and no CSRF token either. It keeps the
/// session length the admin set too (`sign_in_settings`).
///
/// Contract (checked by `contract::session_store`):
/// - [`add`](Self::add) keeps a session and forgets the sessions `forget`
///   names in one step, so a crash keeps both or neither.
/// - [`all`](Self::all) returns every session kept as it was kept, its times
///   cut to the millisecond, in no particular order.
/// - [`forget_unreadable`](Self::forget_unreadable) forgets the sessions
///   kept that can't be read back, and only those, and says how many; once
///   it has, [`all`](Self::all) reads every one left.
/// - [`remove`](Self::remove) forgets the sessions named, and says whether
///   it kept any of them; one not kept is no error.
/// - [`remove_all`](Self::remove_all) forgets every session, and says
///   whether it kept any.
/// - [`record_use`](Self::record_use) keeps a session's last use; a session
///   not kept is no error and is not brought back.
/// - [`length`](Self::length) is `None` until
///   [`set_length`](Self::set_length) has kept one, then the one kept last;
///   forgetting sessions never touches it. `set_length` says whether the
///   length kept changed.
/// - What is kept outlives the handle: a store opened again on the same
///   place returns it.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Keeps `session`, and forgets the sessions of `forget`.
    async fn add(&self, session: &KeptSession, forget: &[SessionDigest]) -> Result<(), StoreError>;

    /// Every session kept.
    async fn all(&self) -> Result<Vec<KeptSession>, StoreError>;

    /// Forgets the sessions kept that can't be read back, as damage or
    /// another build could leave them, and says how many there were.
    async fn forget_unreadable(&self) -> Result<usize, StoreError>;

    /// Forgets the sessions of `digests`; whether any of them was kept.
    async fn remove(&self, digests: &[SessionDigest]) -> Result<bool, StoreError>;

    /// Forgets every session; whether any was kept.
    async fn remove_all(&self) -> Result<bool, StoreError>;

    /// Keeps `at` as the session's last use.
    async fn record_use(&self, digest: &SessionDigest, at: Timestamp) -> Result<(), StoreError>;

    /// The session length kept, or `None` while none has been set.
    async fn length(&self) -> Result<Option<SessionLength>, StoreError>;

    /// Keeps `length` as the session length; whether it differs from the
    /// one kept before.
    async fn set_length(&self, length: SessionLength) -> Result<bool, StoreError>;
}
