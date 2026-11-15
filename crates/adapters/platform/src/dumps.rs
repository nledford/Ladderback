//! Core dumps and debugger attach, turned off for this process (technical
//! design §10.2, decision 98). The daemon holds resolved repository
//! passwords and its sign-in secrets, and a worker the secrets of its job: a
//! core dump would write them to disk, and a debugger of the same user could
//! read them, outside every rule of I4.

use std::io;

/// A protection [`refuse`] couldn't put in place. The process goes on
/// without it.
#[derive(Debug)]
pub enum Unprotected {
    /// The core-dump limit couldn't be set to 0, so a crash may write the
    /// process's memory to disk.
    CoreDumps(io::Error),
    /// Linux only: the process couldn't be made non-dumpable, so another
    /// process of the same user may attach a debugger to it or read its
    /// memory, and a crash may still be dumped through a pipe the kernel's
    /// `core_pattern` names, which ignores the limit.
    DebuggerAttach(io::Error),
}

/// Sets this process's core-dump limit to 0, both the soft and the hard one,
/// so that no program it starts can raise it again (Linux and macOS), and on
/// Linux clears its dumpable flag, which also refuses `ptrace` and reads of
/// `/proc/<pid>/mem` by other processes of the same user. Says what it
/// couldn't do.
///
/// The kernel sets the flag again on the `execve` of a program, so a worker,
/// which is one, calls this itself; the limit is inherited.
pub fn refuse() -> Vec<Unprotected> {
    [core_dumps(), debugger_attach()]
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(unix)]
fn core_dumps() -> Option<Unprotected> {
    use nix::sys::resource::{Resource, setrlimit};
    let refused = setrlimit(Resource::RLIMIT_CORE, 0, 0).err()?;
    Some(Unprotected::CoreDumps(refused.into()))
}

#[cfg(not(unix))]
fn core_dumps() -> Option<Unprotected> {
    None
}

#[cfg(target_os = "linux")]
fn debugger_attach() -> Option<Unprotected> {
    let refused = nix::sys::prctl::set_dumpable(false).err()?;
    Some(Unprotected::DebuggerAttach(refused.into()))
}

/// Only Linux has a dumpable flag.
#[cfg(not(target_os = "linux"))]
fn debugger_attach() -> Option<Unprotected> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use nix::sys::resource::{Resource, getrlimit};

    // Each test runs in a process of its own under nextest, and a test
    // process that keeps no core dump loses nothing.
    #[test]
    fn a_process_that_refuses_dumps_has_a_core_dump_limit_of_0() {
        let unprotected = super::refuse();

        assert!(unprotected.is_empty(), "{unprotected:?}");
        assert_eq!(
            getrlimit(Resource::RLIMIT_CORE).expect("the core-dump limit"),
            (0, 0)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_that_refuses_dumps_is_not_dumpable() {
        let unprotected = super::refuse();

        assert!(unprotected.is_empty(), "{unprotected:?}");
        assert!(!nix::sys::prctl::get_dumpable().expect("the dumpable flag"));
    }
}
