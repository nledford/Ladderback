//! The line the hub logs when a machine pairs (#1307).

use domain::fleet::{PairRequest, Pairing};

/// What a person at the hub's terminal or in its log matches against the
/// machine's own screen: the machine's hostname, escaped as the log escapes
/// outside text, its agent ID and key fingerprint, and whether it waits for
/// approval. The token, the pairing key and every seed stay out (I4).
pub(super) fn paired(request: &PairRequest, outcome: &Pairing) -> String {
    todo!("#1307")
}

#[cfg(test)]
mod tests;
