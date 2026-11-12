//! The `Random` port's contract (technical design §3.2): one case per
//! method, each run by [`run`].

use std::collections::HashSet;

use crate::ports::Random;

pub fn run(random: &dyn Random) {
    draws_differ(random);
    tokens_differ(random);
}

/// A hundred draws are a hundred different numbers: over every `u64`, a
/// working source repeats one with a chance of about 2^-57.
fn draws_differ(random: &dyn Random) {
    let mut seen = HashSet::new();
    for draw in 0..100 {
        assert!(
            seen.insert(random.draw()),
            "draw {draw} repeated an earlier one"
        );
    }
}

/// A hundred tokens are a hundred different tokens, each one the platform
/// gave.
fn tokens_differ(random: &dyn Random) {
    let mut seen = HashSet::new();
    for draw in 0..100 {
        let token = random.token();
        assert!(token.is_ok(), "token {draw} gave no bytes: {token:?}");
        let Ok(token) = token else { return };
        assert!(seen.insert(token), "token {draw} repeated an earlier one");
    }
}
