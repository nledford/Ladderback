use std::sync::atomic::{AtomicU64, Ordering};

use domain::access::TOKEN_BYTES;
use domain::execution::next_run::RandomDraw;

use crate::ports::{Random, RandomUnavailable};

/// Randomness from a seed: each draw or token comes from the seed plus the
/// number of draws and tokens before it, so a scenario's draws and tokens
/// differ and repeat from run to run.
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

    fn token(&self) -> Result<[u8; TOKEN_BYTES], RandomUnavailable> {
        let draw = self.next.fetch_add(1, Ordering::Relaxed).to_le_bytes();
        let mut bytes = [0; TOKEN_BYTES];
        for (byte, value) in bytes.iter_mut().zip(draw.iter().cycle()) {
            *byte = *value;
        }
        Ok(bytes)
    }
}
