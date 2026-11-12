//! Trunk's pre-build hook (`crates/web/Trunk.toml`), run as trunk runs it: the
//! command and arguments read from the file, in the web crate's folder of a
//! scratch checkout, with a fake `npm` and `npx` first on `PATH` that log
//! their calls. The hook installs the npm packages Tailwind's CLI needs, so a
//! build must install them again when `package-lock.json` changed since they
//! were installed (after a pull, say), and must not when it didn't: the build
//! then needs no network.
//!
//! Unix only, as the hook is: it runs `sh`, and Windows isn't a build platform.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The web crate's folder, where `Trunk.toml` lives.
const PACKAGE: &str = env!("CARGO_MANIFEST_DIR");

const LOCKFILE: &str = "{\"lockfileVersion\": 3, \"packages\": {\"node_modules/@tailwindcss/cli\": {\"version\": \"4.1.0\"}}}\n";
const BUMPED_LOCKFILE: &str = "{\"lockfileVersion\": 3, \"packages\": {\"node_modules/@tailwindcss/cli\": {\"version\": \"4.1.1\"}}}\n";

/// Logs its arguments. `npm ci` replaces node_modules with the packages, as
/// npm does; with `FAKE_NPM_FAILS` set it fails having changed node_modules
/// without replacing it, as an install that stops part way does.
const FAKE_NPM: &str = r#"#!/bin/sh
echo "npm $*" >> "$FAKE_LOG"
[ "$1" = ci ] || exit 2
if [ -n "${FAKE_NPM_FAILS:-}" ]; then
  mkdir -p node_modules && echo partial > node_modules/partial
  exit 1
fi
rm -rf node_modules
mkdir -p node_modules/@tailwindcss/cli
"#;

const FAKE_NPX: &str = r#"#!/bin/sh
echo "npx $*" >> "$FAKE_LOG"
"#;

/// The pre-build hook's command and its arguments, as trunk reads them.
fn hook() -> (String, Vec<String>) {
    let text = fs::read_to_string(Path::new(PACKAGE).join("Trunk.toml")).unwrap();
    let config: toml::Table = toml::from_str(&text).unwrap();
    let hooks = config["hooks"].as_array().unwrap();
    let [hook] = hooks.as_slice() else {
        panic!("Trunk.toml has {} hooks; this test runs one", hooks.len());
    };
    assert_eq!(hook["stage"].as_str(), Some("pre_build"));
    let command = hook["command"].as_str().unwrap().to_owned();
    let arguments = hook["command_arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap().to_owned())
        .collect();
    (command, arguments)
}

/// The first `name` on this process's `PATH`.
fn on_path(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{name} is not on PATH"))
}

fn write_executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A scratch checkout: `crates/web` with a lockfile, the repository's
/// `scripts` folder, and a `bin` folder with the fake `npm` and `npx`.
struct Checkout {
    root: tempfile::TempDir,
}

impl Checkout {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("crates/web")).unwrap();
        symlink(
            Path::new(PACKAGE).join("../../scripts"),
            root.path().join("scripts"),
        )
        .unwrap();
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        write_executable(&bin.join("npm"), FAKE_NPM);
        write_executable(&bin.join("npx"), FAKE_NPX);
        let checkout = Self { root };
        checkout.set_lockfile(LOCKFILE);
        checkout
    }

    fn web(&self) -> PathBuf {
        self.root.path().join("crates/web")
    }

    fn log(&self) -> PathBuf {
        self.root.path().join("calls.log")
    }

    fn set_lockfile(&self, text: &str) {
        fs::write(self.web().join("package-lock.json"), text).unwrap();
    }

    /// This process's `PATH` with the fake `npm` and `npx` first.
    fn fake_path(&self) -> OsString {
        let path = std::env::var_os("PATH").unwrap();
        let dirs =
            std::iter::once(self.root.path().join("bin")).chain(std::env::split_paths(&path));
        std::env::join_paths(dirs).unwrap()
    }

    /// Runs the hook with the fake `npm` and `npx` first on `PATH`.
    fn build(&self) -> Output {
        self.build_with(self.fake_path(), false)
    }

    /// Runs the hook with an `npm ci` that fails part way.
    fn failing_build(&self) -> Output {
        self.build_with(self.fake_path(), true)
    }

    fn build_with(&self, path: OsString, npm_fails: bool) -> Output {
        let (command, arguments) = hook();
        let mut hook = Command::new(command);
        hook.args(arguments)
            .current_dir(self.web())
            .env("PATH", path)
            .env("FAKE_LOG", self.log())
            .env_remove("FAKE_NPM_FAILS");
        if npm_fails {
            hook.env("FAKE_NPM_FAILS", "1");
        }
        hook.output().unwrap()
    }

    /// The calls the fake `npm` and `npx` logged, each as its first two words.
    fn calls(&self) -> Vec<String> {
        let log = fs::read_to_string(self.log()).unwrap_or_default();
        log.lines()
            .map(|line| line.split(' ').take(2).collect::<Vec<_>>().join(" "))
            .collect()
    }

    fn installs(&self) -> usize {
        self.calls().iter().filter(|call| *call == "npm ci").count()
    }
}

#[track_caller]
fn assert_succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "the hook failed: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_build_after_the_lockfile_changed_installs_the_packages_again() {
    let checkout = Checkout::new();
    assert_succeeded(&checkout.build());

    checkout.set_lockfile(BUMPED_LOCKFILE);
    assert_succeeded(&checkout.build());
    assert_eq!(checkout.installs(), 2, "calls: {:?}", checkout.calls());

    assert_succeeded(&checkout.build());
    assert_eq!(checkout.installs(), 2, "calls: {:?}", checkout.calls());
}

#[test]
fn packages_installed_without_a_record_of_their_lockfile_are_installed_again() {
    let checkout = Checkout::new();
    // What an install made before this hook kept its record leaves behind.
    fs::create_dir_all(checkout.web().join("node_modules/@tailwindcss/cli")).unwrap();
    fs::write(
        checkout.web().join("node_modules/.package-lock.json"),
        LOCKFILE,
    )
    .unwrap();

    assert_succeeded(&checkout.build());
    assert_succeeded(&checkout.build());

    assert_eq!(checkout.installs(), 1, "calls: {:?}", checkout.calls());
}

#[test]
fn an_install_that_failed_part_way_is_redone_with_the_old_lockfile_back() {
    let checkout = Checkout::new();
    assert_succeeded(&checkout.build());

    checkout.set_lockfile(BUMPED_LOCKFILE);
    let failed = checkout.failing_build();
    assert!(!failed.status.success(), "the hook ignored npm's failure");
    assert_eq!(
        checkout.calls(),
        ["npm ci", "npx --no-install", "npm ci"],
        "tailwind ran after a failed install"
    );

    checkout.set_lockfile(LOCKFILE);
    assert_succeeded(&checkout.build());
    assert_eq!(checkout.installs(), 3, "calls: {:?}", checkout.calls());
}

#[test]
fn a_build_without_npm_stops_and_says_npm_is_missing() {
    let checkout = Checkout::new();
    let tools = checkout.root.path().join("tools");
    fs::create_dir(&tools).unwrap();
    for tool in ["sh", "cmp", "cp", "rm"] {
        symlink(on_path(tool), tools.join(tool)).unwrap();
    }

    let output = checkout.build_with(tools.into_os_string(), false);

    assert!(!output.status.success(), "the hook succeeded without npm");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("npm is not installed"), "stderr: {stderr}");
    assert!(checkout.calls().is_empty(), "calls: {:?}", checkout.calls());
}
