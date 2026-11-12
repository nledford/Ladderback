use domain::access::TOKEN_BYTES;
use domain::execution::next_run::RandomDraw;

/// Randomness (§3.2): the only source of it for `app`. So far it draws the
/// number that spreads a scheduled run over its jitter (§5, decision 60) and
/// the bytes of a session's tokens (§10.1).
pub trait Random: Send + Sync {
    /// A number drawn uniformly over every `u64`. Successive draws vary: a
    /// source that always drew the same number would put every run at the
    /// same point of its jitter. Not for secrets: a draw may be seeded and
    /// guessed; what no one may guess comes from [`Random::token`].
    fn draw(&self) -> RandomDraw;

    /// Bytes no one can guess, or why the platform gave none.
    fn token(&self) -> Result<[u8; TOKEN_BYTES], RandomUnavailable>;
}

/// The platform gave no random bytes; the text is its own message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomUnavailable(pub String);
