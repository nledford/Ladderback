//! Runs the manager's program: `systemctl` or `launchctl`, found on the
//! `PATH` it is given, with argv only and an environment of its own.

use std::ffi::OsString;

/// The variables the manager's program is run with: what it needs to find
/// the user's manager (`XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`), to be
/// found itself (`PATH`), and to speak the user's language. No other
/// variable of the environment reaches it, so no secret does (I4).
#[derive(Clone, Debug, Default)]
pub struct Environment {
    kept: Vec<(OsString, OsString)>,
}

impl Environment {
    /// The variables of `vars` the manager is run with.
    pub fn keeping(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let _ = vars.into_iter().count();
        todo!()
    }

    /// The variables kept.
    pub fn kept(&self) -> &[(OsString, OsString)] {
        &self.kept
    }
}
