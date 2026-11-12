//! Randomness from the operating system (technical design §3.2).

use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};

use app::ports::{Random, RandomUnavailable};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use domain::access::TOKEN_BYTES;
use domain::execution::next_run::RandomDraw;

/// How many draws were made, so no two draws hash the same input.
static DRAWS: AtomicU64 = AtomicU64::new(0);

/// The operating system's randomness. A draw is the standard library's
/// randomly keyed hash of a count of draws: the keys come from the operating
/// system once per thread, and each new hasher's differ from the last, so
/// no two draws are the same hash of the same input. It spreads runs over
/// their jitter (§5), which needs numbers no two agents share, not secrets.
/// A token is the operating system's random bytes, for secrets (§10.1): a
/// platform that gives none is an error, never a panic.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsRandom;

impl Random for OsRandom {
    fn draw(&self) -> RandomDraw {
        let count = DRAWS.fetch_add(1, Ordering::Relaxed);
        RandomDraw::new(RandomState::new().hash_one(count))
    }

    fn token(&self) -> Result<[u8; TOKEN_BYTES], RandomUnavailable> {
        let mut bytes = [0; TOKEN_BYTES];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|error| RandomUnavailable(error.to_string()))?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_randomness_meets_the_random_contract() {
        app::contract::random::run(&OsRandom);
    }
}
