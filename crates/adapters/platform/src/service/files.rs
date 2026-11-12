//! The service files: a systemd unit and a launchd plist, each a fixed
//! template with the paths quoted by the file's own rules.

use domain::service::ServicePath;

/// The systemd user unit that runs `program serve`. Its cache folder is the
/// one systemd makes for it, private to the user, which the agent is pointed
/// at (decision 59).
pub fn systemd_unit(program: &ServicePath) -> String {
    let _ = program;
    todo!()
}

/// The launchd agent labelled `label` that runs `program serve` from login
/// on, again after a crash, and writes its log to `log`.
pub fn launchd_plist(label: &str, program: &ServicePath, log: &ServicePath) -> String {
    let _ = (label, program, log);
    todo!()
}
