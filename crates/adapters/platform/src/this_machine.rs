//! The machine identifier (technical design §7.1; §16, decision NN; #296):
//! the one the operating system gives this computer, `/etc/machine-id` on
//! Linux and `IOPlatformUUID` on macOS, and the one recorded in the data
//! folder beside the agent ID, which a copy of the folder brings with it.

use std::path::{Path, PathBuf};

use app::ports::{MachineError, ThisMachine};
use domain::AgentId;
use domain::fleet::MachineIdentifier;

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
        let _ = &self.source;
        todo!()
    }

    fn recorded(&self) -> Result<Option<MachineIdentifier>, MachineError> {
        let _ = &self.data_dir;
        todo!()
    }

    fn record(&self, identifier: &MachineIdentifier) -> Result<(), MachineError> {
        let _ = identifier;
        todo!()
    }

    fn renew(&self, identifier: &MachineIdentifier) -> Result<AgentId, MachineError> {
        let _ = identifier;
        todo!()
    }
}

/// The value of `"IOPlatformUUID" = "…"` in an `ioreg` report.
#[cfg_attr(not(test), expect(dead_code, reason = "a stub its tests specify"))]
fn platform_uuid(report: &str) -> Option<String> {
    let _ = report;
    todo!()
}

/// Linux's machine ID, which machine-id(5) says is 32 lowercase hexadecimal
/// digits on one line.
#[cfg_attr(not(test), expect(dead_code, reason = "a stub its tests specify"))]
fn machine_id(text: &str) -> Option<&str> {
    let _ = text;
    todo!()
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
