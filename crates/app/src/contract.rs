//! Port contract suites (technical design §3.2). The fake in `fakes` runs
//! each suite in `app`'s tests, and each real adapter runs the same suite, so
//! a fake can't drift from reality. A suite panics on the first violation.

pub mod clock;
pub mod engine;
pub mod operation_store;
