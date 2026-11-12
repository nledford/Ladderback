use std::sync::atomic::{AtomicU64, Ordering};

use domain::execution::next_run::RandomDraw;

use crate::ports::Random;

/// Randomness from a seed: each draw is the seed plus the number of draws
/// before it, so a scenario's draws differ and repeat from run to run.
pub struct SeededRandom {
    next: AtomicU64,
}

impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self {
            next: AtomicU64::new(seed),
        }
    }
}

impl Random for SeededRandom {
    fn draw(&self) -> RandomDraw {
        RandomDraw::new(self.next.fetch_add(1, Ordering::Relaxed))
    }
}
