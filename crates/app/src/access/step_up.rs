//! Step-up confirmation (§10.4), as a restore, a download and the approval
//! of a proposal ask for it.

use std::net::IpAddr;

use domain::access::step_up::{StepUpConfirmation, StepUpFor};

use super::{Access, PasswordRefused};

/// Why a step-up confirmation wasn't taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StepUpRefused {
    /// It wasn't the admin password, or the ID of what it is for.
    Failed,
    /// The address's wait after failed sign-ins has `seconds` left; the
    /// password wasn't checked.
    Wait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own text.
    Unavailable(String),
}

/// Checks `confirmation` for `what`: the admin password, in `address`'s
/// sign-in waits, when `access` says sign-in is on; the ID of `what`
/// otherwise.
pub(crate) async fn confirm_step_up(
    access: Option<&Access>,
    address: IpAddr,
    confirmation: &StepUpConfirmation,
    what: StepUpFor<'_>,
) -> Result<(), StepUpRefused> {
    let Some(access) = access else {
        return if confirmation.names(what) {
            Ok(())
        } else {
            Err(StepUpRefused::Failed)
        };
    };
    access
        .step_up(address, confirmation.as_password())
        .await
        .map(|_| ())
        .map_err(|refused| match refused {
            PasswordRefused::WrongPassword => StepUpRefused::Failed,
            PasswordRefused::Wait { seconds } => StepUpRefused::Wait { seconds },
            PasswordRefused::Unavailable(text) => StepUpRefused::Unavailable(text),
        })
}
