use async_trait::async_trait;
use domain::Timestamp;
use domain::access::PasswordFingerprint;
use domain::access::api_token::{ApiToken, ApiTokenId, TokenPermissions};

use crate::ports::StoreError;

/// Whether [`ApiTokenStore::add`] kept the token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    Kept,
    /// As many tokens as the limit allows are kept already; nothing changed.
    Full,
}

/// What [`ApiTokenStore::set_permissions`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionsSet {
    /// The token held other permissions and now holds the ones given.
    Changed,
    /// The token held the ones given already; nothing was written.
    Unchanged,
    /// No token of that ID is kept; nothing changed.
    NotKept,
}

/// Keeps the API tokens (technical design §10.1, §9 `api_tokens`; decision
/// 93): each one's ID, label, permissions, the SHA-256 of its secret, the admin
/// password it was created under, and when it was created and last used.
/// Never a secret: the store is given none.
///
/// Contract (checked by `contract::api_token_store`):
/// - [`add`](Self::add) keeps a token unless `at_most` are kept already,
///   deciding and keeping in one step, so of any number of adds racing for
///   the last place exactly one is kept.
/// - [`get`](Self::get) returns a token as it was kept, its times cut to the
///   millisecond; an ID never kept, or revoked, is none.
/// - [`all`](Self::all) returns every token kept, oldest first.
/// - [`revoke`](Self::revoke) forgets a token and says whether there was one.
/// - [`set_permissions`](Self::set_permissions) gives a kept token other
///   permissions, the rest of it as it was, and says whether there was one
///   and whether its permissions changed; the ones it holds already change
///   nothing, and it never brings a revoked token back.
/// - [`revoke_all_but`](Self::revoke_all_but) forgets every token created
///   under another admin password than `password`.
/// - [`record_use`](Self::record_use) keeps a token's last use; a token not
///   kept is no error and is not brought back.
/// - What is kept outlives the handle: a store opened again on the same
///   place returns it.
#[async_trait]
pub trait ApiTokenStore: Send + Sync {
    /// Keeps `token` unless `at_most` tokens are kept already.
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError>;

    /// The token `id`, if it is kept.
    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError>;

    /// Every token kept, oldest first.
    async fn all(&self) -> Result<Vec<ApiToken>, StoreError>;

    /// Forgets the token `id`; whether one was kept.
    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError>;

    /// Gives the token `id` the `permissions`; whether one was kept, and
    /// whether it held other permissions.
    async fn set_permissions(
        &self,
        id: &ApiTokenId,
        permissions: TokenPermissions,
    ) -> Result<PermissionsSet, StoreError>;

    /// Forgets every token created under another admin password.
    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError>;

    /// Keeps `at` as the token's last use.
    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError>;
}
