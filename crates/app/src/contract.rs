//! Port contract suites (technical design §3.2). The fake in `fakes` runs
//! each suite in `app`'s tests, and each real adapter runs the same suite, so
//! a fake can't drift from reality. A suite panics on the first violation.
pub mod admin_password;
pub mod api_token_store;
pub mod change_codec;
pub mod clock;
pub mod config_drafts;
pub mod config_store;
pub mod engine;
pub mod event_bus;
pub mod full_disk_access;
pub mod hooks;
pub mod network;
pub mod network_link;
pub mod notifier;
pub mod operation_store;
pub mod power;
pub mod profiles;
pub mod proposal_store;
pub mod random;
pub mod recent_log;
pub mod release_feed;
pub mod schedule_store;
pub mod secrets;
pub mod service;
pub mod session_store;
pub mod stats_store;
