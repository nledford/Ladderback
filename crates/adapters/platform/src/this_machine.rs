//! The machine identifier (technical design §7.1; §16, decision 144; #296):
//! the one the operating system gives this computer, `/etc/machine-id` on
//! Linux and `IOPlatformUUID` on macOS, and the one recorded in the data
//! folder beside the agent ID, which a copy of the folder brings with it.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use app::ports::{MachineError, ThisMachine};
use domain::fleet::{MAX_MACHINE_IDENTIFIER_BYTES, MachineIdentifier};
use domain::{AgentId, Uuid};

use crate::agent_id::{self, AGENT_ID_FILE, sync_dir};

/// The file in the data folder holding the recorded machine identifier.
pub const MACHINE_IDENTIFIER_FILE: &str = "machine-identifier";

/// Where this computer's identifier is read from.
#[derive(Clone, Debug)]
enum Source {
    /// The operating system's own.
    System,
    /// A file holding it, as `/etc/machine-id` does.
    File(PathBuf),
}

/// This machine as its operating system and its data folder know it.
#[derive(Clone, Debug)]
pub struct SystemMachine {
    data_dir: PathBuf,
    source: Source,
}

impl SystemMachine {
    /// The machine whose data folder is `data_dir`, its identifier read
    /// from the operating system.
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            source: Source::System,
        }
    }

    /// The machine whose data folder is `data_dir`, its identifier read
    /// from `file`, as `/etc/machine-id` holds Linux's.
    pub fn reading(data_dir: &Path, file: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            source: Source::File(file.to_path_buf()),
        }
    }
}

impl ThisMachine for SystemMachine {
    fn identifier(&self) -> Result<MachineIdentifier, MachineError> {
        let text = match &self.source {
            Source::File(file) => read_bounded(file).map_err(|error| failed(file, error))?,
            Source::System => system_identifier()?,
        };
        MachineIdentifier::parse(&text).map_err(|invalid| MachineError {
            detail: format!("the system's machine identifier is not one: {invalid:?}"),
        })
    }

    fn recorded(&self) -> Result<Option<MachineIdentifier>, MachineError> {
        let path = self.data_dir.join(MACHINE_IDENTIFIER_FILE);
        let text = match read_bounded(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failed(&path, error)),
        };
        MachineIdentifier::parse(&text)
            .map(Some)
            .map_err(|invalid| MachineError {
                detail: format!(
                    "{} holds no machine identifier: {invalid:?}",
                    path.display()
                ),
            })
    }

    fn record(&self, identifier: &MachineIdentifier) -> Result<(), MachineError> {
        replace(&self.data_dir, MACHINE_IDENTIFIER_FILE, identifier.as_str())
    }

    /// Writes the new machine ID first, then this computer's identifier: a
    /// crash between the two leaves a new ID beside the other computer's
    /// identifier, which the next start finds a copy again, never the old
    /// ID beside this computer's, which it would take for its own.
    fn renew(&self, identifier: &MachineIdentifier) -> Result<AgentId, MachineError> {
        let id = Uuid::now_v7();
        replace(&self.data_dir, AGENT_ID_FILE, &id.hyphenated().to_string())?;
        self.record(identifier)?;
        agent_id::load(&self.data_dir)
            .map_err(|error| failed(&self.data_dir.join(AGENT_ID_FILE), error))?
            .ok_or_else(|| MachineError {
                detail: "the new machine ID was written and is gone".to_owned(),
            })
    }
}

/// More than an identifier and its line break: a longer file is never one,
/// and is never read whole (technical design §13.5).
const MAX_FILE_BYTES: u64 = MAX_MACHINE_IDENTIFIER_BYTES as u64 + 2;

/// The most of `ioreg`'s report read: a few kilobytes on every Mac seen.
#[cfg(target_os = "macos")]
const MAX_IOREG_BYTES: u64 = 256 * 1024;

fn failed(path: &Path, error: impl std::fmt::Display) -> MachineError {
    MachineError {
        detail: format!("{}: {error}", path.display()),
    }
}

/// The text of `path`, refused unread past [`MAX_FILE_BYTES`].
fn read_bounded(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("longer than {MAX_FILE_BYTES} bytes"),
        ));
    }
    Ok(text)
}

/// Writes `text` as `name` in `data_dir` whole or not at all: written and
/// synced under a private name, renamed into place, and the folder synced.
fn replace(data_dir: &Path, name: &str, text: &str) -> Result<(), MachineError> {
    let path = data_dir.join(name);
    let staged = data_dir.join(format!("{name}.{}.tmp", Uuid::now_v7().simple()));
    let written = (|| {
        fs::create_dir_all(data_dir)?;
        let mut file = File::create_new(&staged)?;
        writeln!(file, "{text}")?;
        file.sync_all()?;
        fs::rename(&staged, &path)?;
        sync_dir(data_dir)
    })();
    if written.is_err() {
        // Nothing to keep of a write that failed; a leftover is harmless.
        let _ = fs::remove_file(&staged);
    }
    written.map_err(|error| failed(&path, error))
}

/// The identifier the operating system gives this computer.
#[cfg(target_os = "linux")]
fn system_identifier() -> Result<String, MachineError> {
    // systemd's, and D-Bus's where systemd isn't (machine-id(5)).
    let primary = Path::new("/etc/machine-id");
    let text = read_bounded(primary)
        .or_else(|_| read_bounded(Path::new("/var/lib/dbus/machine-id")))
        .map_err(|error| failed(primary, error))?;
    machine_id(&text).map(str::to_owned).ok_or_else(|| {
        failed(
            primary,
            "it holds no machine ID of 32 lowercase hexadecimal digits, as on a first boot",
        )
    })
}

/// Linux's machine ID, which machine-id(5) says is 32 lowercase hexadecimal
/// digits on one line. Anything else, as the `uninitialized` systemd writes
/// on a first boot or an empty file, isn't this computer's lasting
/// identifier: compared at the next start, it would make the computer look
/// like a copy of itself.
#[cfg_attr(
    all(not(target_os = "linux"), not(test)),
    expect(dead_code, reason = "Linux's file, tested on every system")
)]
fn machine_id(text: &str) -> Option<&str> {
    let id = text.strip_suffix('\n').unwrap_or(text);
    (id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(id)
}

/// The identifier the operating system gives this computer: the platform
/// UUID `ioreg -rd1 -c IOPlatformExpertDevice` prints.
#[cfg(target_os = "macos")]
fn system_identifier() -> Result<String, MachineError> {
    use std::process::{Command, Stdio};
    let ioreg = Path::new("/usr/sbin/ioreg");
    let mut command = Command::new(ioreg);
    command
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The spawn gate keeps another start from inheriting the pipe.
    let mut child = spawn_gate::hold(|| command.spawn()).map_err(|error| failed(ioreg, error))?;
    let mut report = String::new();
    let read = match child.stdout.take() {
        Some(stdout) => stdout
            .take(MAX_IOREG_BYTES)
            .read_to_string(&mut report)
            .map(|_| ()),
        None => Err(io::Error::other("its output is not piped")),
    };
    let _ = child.kill();
    let status = child.wait().map_err(|error| failed(ioreg, error))?;
    read.map_err(|error| failed(ioreg, error))?;
    if !status.success() {
        return Err(failed(ioreg, format!("it ended with {status}")));
    }
    platform_uuid(&report).ok_or_else(|| failed(ioreg, "it names no IOPlatformUUID"))
}

/// No identifier on a system that gives none this adapter reads.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn system_identifier() -> Result<String, MachineError> {
    Err(MachineError {
        detail: "this system gives no machine identifier Ladderback reads".to_owned(),
    })
}

/// The value of `"IOPlatformUUID" = "…"` in an `ioreg` report. Built on
/// macOS, which reads it, and in tests everywhere, which check the parsing.
#[cfg(any(target_os = "macos", test))]
fn platform_uuid(report: &str) -> Option<String> {
    report.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "\"IOPlatformUUID\"").then(|| value.trim().trim_matches('"').to_owned())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use app::contract;
    use app::contract::this_machine::ThisMachineHarness;
    use app::ports::ThisMachine;

    use super::SystemMachine;
    use crate::agent_id;

    /// Each machine on a data folder of its own, its identifier in a file
    /// beside it.
    struct Folders(tempfile::TempDir);

    impl ThisMachineHarness for Folders {
        fn on(&self, identifier: Option<&str>) -> Arc<dyn ThisMachine> {
            let root = tempfile::tempdir_in(self.0.path()).unwrap().keep();
            let data_dir = root.join("data");
            std::fs::create_dir(&data_dir).unwrap();
            let file = root.join("machine-id");
            if let Some(identifier) = identifier {
                std::fs::write(&file, format!("{identifier}\n")).unwrap();
            }
            Arc::new(SystemMachine::reading(&data_dir, &file))
        }
    }

    #[test]
    fn the_system_machine_meets_the_this_machine_contract() {
        contract::this_machine::run(&Folders(tempfile::tempdir().unwrap()));
    }

    #[test]
    fn a_new_machine_id_replaces_the_one_in_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("machine-id");
        std::fs::write(&file, "here\n").unwrap();
        let old = agent_id::load_or_create(dir.path()).unwrap();
        let machine = SystemMachine::reading(dir.path(), &file);
        let new = machine.renew(&machine.identifier().unwrap()).unwrap();
        assert_ne!(new, old);
        assert_eq!(agent_id::load(dir.path()).unwrap(), Some(new));
    }

    #[test]
    fn a_record_that_holds_no_identifier_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(super::MACHINE_IDENTIFIER_FILE), " \n").unwrap();
        let machine = SystemMachine::reading(dir.path(), &dir.path().join("machine-id"));
        assert!(machine.recorded().is_err());
    }

    #[test]
    fn this_computer_s_own_identifier_reads_the_same_each_time() {
        let dir = tempfile::tempdir().unwrap();
        let machine = SystemMachine::in_data_dir(dir.path());
        let Ok(first) = machine.identifier() else {
            testkit::skip!(
                "this computer gives no machine identifier: no /etc/machine-id, or not Linux or macOS"
            );
        };
        assert_eq!(machine.identifier().unwrap(), first);
    }

    #[test]
    fn the_platform_uuid_is_read_from_an_ioreg_report() {
        let report = "+-o J316sAP  <class IOPlatformExpertDevice>\n    {\n      \"IOPlatformSerialNumber\" = \"DEMO\"\n      \"IOPlatformUUID\" = \"00000000-0000-4000-8000-000000000000\"\n    }\n";
        assert_eq!(
            super::platform_uuid(report).as_deref(),
            Some("00000000-0000-4000-8000-000000000000")
        );
        assert_eq!(
            super::platform_uuid("\"IOPlatformSerialNumber\" = \"DEMO\""),
            None
        );
    }

    #[test]
    fn a_linux_machine_id_is_32_lowercase_hexadecimal_digits() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(super::machine_id(&format!("{id}\n")), Some(id));
        assert_eq!(super::machine_id(id), Some(id));
        for not_one in [
            "uninitialized\n",
            "",
            "\n",
            "0123456789ABCDEF0123456789ABCDEF\n",
            "0123456789abcdef0123456789abcde\n",
            "0123456789abcdef0123456789abcdef0\n",
            "0123456789abcdef0123456789abcdeg\n",
        ] {
            assert_eq!(super::machine_id(not_one), None, "{not_one:?}");
        }
    }
}
