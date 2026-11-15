//! The tools that act (§4.6): `run_plan` starts a backup, `wait_operation`
//! waits for an operation to end, `cancel_operation` stops a backup.

use std::time::Duration;

use api::ErrorDto;

/// The refusal of a cancel of anything but a backup, alike in both
/// backends (decision 93).
pub fn not_cancellable() -> ErrorDto {
    todo!()
}

/// `timeout`, in seconds, as how long a wait lasts.
pub fn wait_length(_timeout: Option<u64>) -> Duration {
    todo!()
}
