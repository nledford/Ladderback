use domain::execution::next_run::RandomDraw;

/// Randomness (§3.2): the only source of it for `app`. So far it draws the
/// number that spreads a scheduled run over its jitter (§5, decision 60).
pub trait Random: Send + Sync {
    /// A number drawn uniformly over every `u64`. Successive draws vary: a
    /// source that always drew the same number would put every run at the
    /// same point of its jitter.
    fn draw(&self) -> RandomDraw;
}
