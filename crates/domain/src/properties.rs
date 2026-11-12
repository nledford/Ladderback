//! What every property test of the domain runs with (technical design
//! §13.3).

use proptest::test_runner::{Config, RngSeed};

/// The seed every property draws its cases from.
const SEED: u64 = 266;

/// proptest's 256 cases, drawn from one fixed seed rather than a random one:
/// every run of `just test`, here or in CI, tries the same cases, so a
/// property fails on every run or on none. A failure is still saved under
/// the crate's `proptest-regressions/` and tried first from then on.
pub(crate) fn config() -> Config {
    Config {
        rng_seed: RngSeed::Fixed(SEED),
        ..Config::default()
    }
}
