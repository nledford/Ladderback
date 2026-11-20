//! What a fingerprint draws (§16, decision 121, "The fingerprint"), decided
//! on the host: the groups of four its characters stand in.

/// How many characters a group holds.
const GROUP: usize = 4;

/// The groups `fingerprint` is drawn in, four characters each, every
/// character kept, so no fingerprint is ever shown cut short. Its hyphens
/// are where the groups part, never characters of their own; one written
/// without them is grouped alike, and a last group short of four holds what
/// is left.
pub(crate) fn groups(fingerprint: &str) -> Vec<String> {
    let _ = (fingerprint, GROUP);
    todo!()
}

#[cfg(test)]
mod tests;
