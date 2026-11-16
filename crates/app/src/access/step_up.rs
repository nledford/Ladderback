//! Step-up confirmation (§10.4), as a restore and a download ask for it.

use std::net::IpAddr;

use domain::SnapshotId;
use domain::access::Password;
use domain::access::step_up::confirms_snapshot;

use super::{Access, PasswordRefused};

/// Why a step-up confirmation wasn't taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StepUpRefused {
    /// It wasn't the admin password, or the snapshot's ID.
    Failed,
    /// The address's wait after failed sign-ins has `seconds` left; the
    /// password wasn't checked.
    Wait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own text.
    Unavailable(String),
}

/// Checks `confirmation` for something done to `snapshot`: the admin
/// password, in `address`'s sign-in waits, when `access` says sign-in is on;
/// the snapshot's ID otherwise.
pub(crate) async fn confirm_step_up(
    access: Option<&Access>,
    address: IpAddr,
    confirmation: &str,
    snapshot: &SnapshotId,
) -> Result<(), StepUpRefused> {
    let Some(access) = access else {
        return if confirms_snapshot(confirmation, snapshot) {
            Ok(())
        } else {
            Err(StepUpRefused::Failed)
        };
    };
    // Nothing typed, or more than any password, is no attempt at it.
    let attempt = Password::parse(confirmation.to_owned()).map_err(|_| StepUpRefused::Failed)?;
    access
        .step_up(address, &attempt)
        .await
        .map(|_| ())
        .map_err(|refused| match refused {
            PasswordRefused::WrongPassword => StepUpRefused::Failed,
            PasswordRefused::Wait { seconds } => StepUpRefused::Wait { seconds },
            PasswordRefused::Unavailable(text) => StepUpRefused::Unavailable(text),
        })
}
