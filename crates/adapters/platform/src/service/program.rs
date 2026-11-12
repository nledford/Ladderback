//! Which of this program's paths the service file names (decision 64).

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// The path the service file names for this program, which the system gives
/// as `exe` (`std::env::current_exe`), run as `ran` (its first argument) with
/// `search` for its `PATH`.
///
/// A package manager such as Homebrew installs a program in a folder of its
/// version and puts a link to it in a folder on the `PATH`; an upgrade
/// removes the old version's folder and points the link at the new one. So
/// the path is the one the person ran, when it is the same file as `exe` and
/// a service file can name it: `ran` itself when it is absolute, or, when it
/// is a bare name, the first folder of `search` that holds it, as a shell
/// finds it. Otherwise it is `exe` with its links, `.` and `..` resolved, as
/// Linux gives it.
///
/// # Errors
///
/// `exe` can't be resolved: the program's file is gone.
pub fn program_path(
    exe: &Path,
    ran: Option<&OsStr>,
    search: Option<&OsStr>,
) -> io::Result<PathBuf> {
    let _ = (exe, ran, search);
    todo!()
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::program_path;

    /// A scratch folder laid out as Homebrew lays out its own: the program in
    /// a folder of its version, and a link to it in `bin`.
    struct Brew {
        scratch: tempfile::TempDir,
    }

    impl Brew {
        fn new() -> Self {
            let scratch = tempfile::tempdir().unwrap();
            let versioned = scratch.path().join("Cellar/ladderback/1.0/bin");
            std::fs::create_dir_all(&versioned).unwrap();
            std::fs::write(versioned.join("ladderback"), "the program").unwrap();
            std::fs::create_dir_all(scratch.path().join("bin")).unwrap();
            symlink(
                "../Cellar/ladderback/1.0/bin/ladderback",
                scratch.path().join("bin/ladderback"),
            )
            .unwrap();
            Self { scratch }
        }

        fn at(&self, path: &str) -> PathBuf {
            self.scratch.path().join(path)
        }

        /// A folder of the scratch, with a file `ladderback` that is another
        /// program.
        fn another_program_in(&self, folder: &str) {
            std::fs::create_dir_all(self.at(folder)).unwrap();
            std::fs::write(self.at(folder).join("ladderback"), "another").unwrap();
        }

        /// The program, as Linux gives it: its links resolved.
        fn resolved(&self) -> PathBuf {
            std::fs::canonicalize(self.at("Cellar/ladderback/1.0/bin/ladderback")).unwrap()
        }

        /// A `PATH` of the scratch's `folders`.
        fn search(&self, folders: &[&str]) -> OsString {
            std::env::join_paths(folders.iter().map(|folder| self.at(folder))).unwrap()
        }
    }

    fn bare(name: &str) -> Option<&OsStr> {
        Some(OsStr::new(name))
    }

    #[test]
    fn a_program_found_on_the_path_through_a_link_is_named_by_the_link() {
        let brew = Brew::new();
        let search = brew.search(&["missing", "bin"]);
        let named = program_path(&brew.resolved(), bare("ladderback"), Some(&search)).unwrap();
        assert_eq!(named, brew.at("bin/ladderback"));
    }

    #[test]
    fn a_program_run_by_the_absolute_path_of_a_link_is_named_by_it() {
        let brew = Brew::new();
        let link = brew.at("bin/ladderback");
        let named = program_path(&brew.resolved(), Some(link.as_os_str()), None).unwrap();
        assert_eq!(named, link);
    }

    #[test]
    fn another_program_of_the_same_name_on_the_path_is_passed_over() {
        let brew = Brew::new();
        brew.another_program_in("other");
        let search = brew.search(&["other", "bin"]);
        let named = program_path(&brew.resolved(), bare("ladderback"), Some(&search)).unwrap();
        assert_eq!(named, brew.at("bin/ladderback"));
    }

    #[test]
    fn an_absolute_path_run_that_is_another_program_is_passed_over() {
        let brew = Brew::new();
        brew.another_program_in("other");
        let other = brew.at("other/ladderback");
        let named = program_path(&brew.resolved(), Some(other.as_os_str()), None).unwrap();
        assert_eq!(named, brew.resolved());
    }

    #[test]
    fn a_folder_of_the_path_a_service_file_can_not_name_is_passed_over() {
        let brew = Brew::new();
        std::fs::create_dir_all(brew.at("other")).unwrap();
        let search = brew.search(&["other/../bin"]);
        let named = program_path(&brew.resolved(), bare("ladderback"), Some(&search)).unwrap();
        assert_eq!(named, brew.resolved());
    }

    #[test]
    fn a_program_run_by_a_relative_path_is_named_with_its_links_resolved() {
        // macOS gives the path as it was run, `..` and all.
        let brew = Brew::new();
        let exe = brew.at("bin/../Cellar/ladderback/1.0/bin/ladderback");
        let search = brew.search(&["bin"]);
        let named = program_path(&exe, bare("../bin/ladderback"), Some(&search)).unwrap();
        assert_eq!(named, brew.resolved());
    }

    #[test]
    fn a_program_with_no_name_it_was_run_by_is_named_as_the_system_gives_it() {
        let brew = Brew::new();
        let link = brew.at("bin/ladderback");
        assert_eq!(program_path(&link, None, None).unwrap(), link);
    }

    #[test]
    fn a_program_started_by_another_name_is_named_by_the_link_the_system_gives() {
        // `exec -a` or a launcher sets the first argument; macOS still gives
        // the link the program was started through.
        let brew = Brew::new();
        let search = brew.search(&["bin"]);
        let link = brew.at("bin/ladderback");
        let named = program_path(&link, bare("my-backups"), Some(&search)).unwrap();
        assert_eq!(named, link);
    }

    #[test]
    fn a_program_whose_file_is_gone_can_not_be_named() {
        let brew = Brew::new();
        assert!(program_path(&brew.at("gone/ladderback"), None, None).is_err());
    }
}
