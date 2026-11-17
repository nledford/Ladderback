//! What the Repositories page notes above its list, of the machine rather
//! than of one repository (security §10.2): on Linux, a keyring service that
//! runs now but that nothing can start on demand (#923). Asked only when the
//! page asks, never before a run.

use api::WarningDto;

use crate::ports::Keyring;

/// The page's notes: `keyring-not-activatable` while `keyring` says its
/// service runs but can't be started on demand, and nothing otherwise. A
/// keyring started on demand, none at all, or an answer that couldn't be
/// had is noted nothing: a false alarm would send a person to repair what
/// isn't broken.
pub async fn page_notes(_keyring: &dyn Keyring) -> Vec<WarningDto> {
    todo!()
}
