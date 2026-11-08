//! The API's error and warning codes, and the catalog message each one is
//! shown as (technical design §8.4). A code is an enum variant on the wire; the
//! message is chosen here, so a new variant is not complete until it has a
//! message.

use api::{ErrorCode, WarningCode};

use crate::MessageId;

/// Every error code with its message.
pub const ERRORS: &[(ErrorCode, MessageId)] = &[];

/// Every warning code with its message.
pub const WARNINGS: &[(WarningCode, MessageId)] = &[];

/// The message `code` is shown as.
pub fn error_message(code: ErrorCode) -> MessageId {
    let _ = code;
    todo!()
}

/// The message `code` is shown as.
pub fn warning_message(code: WarningCode) -> MessageId {
    let _ = code;
    todo!()
}
