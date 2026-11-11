//! The daemon's own folders, seen from the tests that start it: no test target
//! that runs the `ladderback` binary may touch the platform's real data or
//! config folder (`~/Library/Application Support/ladderback` on macOS, the XDG
//! folders on Linux). A daemon that did would share the developer's operation
//! log and its migrations with every other daemon on the machine: three
//! daemons applying a new migration at once raced, a branch older than the
//! real database failed with `NewerSchema`, and the tests wrote to the
//! developer's real operation log.
//!
//! Each test here runs one of those targets' compiled test binaries, with
//! `HOME`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` pointing into a scratch
//! folder, so the folder the platform would pick for the daemon is a scratch
//! one, and fails when the run left anything there. Where the platform keeps
//! the config, the folder holds a `config.toml` from a newer schema too: a
//! daemon that reads the platform's config instead of its own folder's, which
//! leaves no file behind, refuses to start and fails its test. The binaries are run as
//! they are, not through `cargo test`: changing `HOME` under Cargo and the
//! toolchain's own wrappers rebuilds and writes to that folder as well.
//!
//! The `@ui` journeys start the daemon from Node, with scratch folders of
//! their own (`ui-tests/support/world.ts`); they are not run here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The root package's directory: where `cargo test` runs the test binaries.
const PACKAGE: &str = env!("CARGO_MANIFEST_DIR");

/// The compiled test binary of the integration-test target `target`, built if
/// it isn't yet. The flags are the ones `just test` gives every Cargo command,
/// so that Cargo reuses the build it has.
fn test_binary(target: &str) -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["test", "--workspace", "--no-run", "--message-format=json"])
        .args(["--test", target])
        .current_dir(PACKAGE)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .expect("cargo ran");
    assert!(
        output.status.success(),
        "cargo could not build the `{target}` test binary"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == target)
        .filter(|message| message["target"]["kind"] == serde_json::json!(["test"]))
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| panic!("cargo built no `{target}` test binary"))
}

/// Where the daemon would read its `config.toml` if no override named a
/// folder, under the home folder `home` that the platform's folders point into:
/// the Mac's application-support folder, and the XDG config folder.
const PLATFORM_CONFIGS: [&str; 2] = [
    "Library/Application Support/ladderback/config.toml",
    ".config/ladderback/config.toml",
];

/// A configuration no daemon starts on.
const NEWER_SCHEMA: &str = "schema = 999999\n";

/// Every file and folder under `dir`, as paths relative to it, sorted.
fn contents(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(relative) = path.strip_prefix(base) {
                found.push(relative.display().to_string());
            }
            if path.is_dir() {
                walk(&path, base, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.sort();
    found
}

/// The test targets that run the `ladderback` binary: the daemon, or its
/// worker (which the daemon's folders don't concern, but a test that started
/// to resolve them would show here).
const TARGETS: [&str; 4] = ["logging", "cli_locale", "engine_contract", "e2e"];

/// Runs the test binary of `target` to completion with the platform's folders
/// pointing into a scratch folder, and returns what it left there.
fn left_in_the_platforms_folders_by(target: &str) -> Vec<String> {
    let binary = test_binary(target);
    let home = tempfile::tempdir().expect("a scratch home");
    for config in PLATFORM_CONFIGS {
        let path = home.path().join(config);
        std::fs::create_dir_all(path.parent().expect("a folder holds the file"))
            .expect("the platform's config folder");
        std::fs::write(&path, NEWER_SCHEMA).expect("the config file");
    }
    let output = Command::new(&binary)
        .current_dir(PACKAGE)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        // These name the folders before the platform's do: a caller that has
        // them set would hide a daemon that ignores them.
        .env_remove("LADDERBACK_CONFIG_DIR")
        .env_remove("LADDERBACK_DATA_DIR")
        .stdin(Stdio::null())
        .output()
        .expect("the test binary ran");
    // The planted files, and the folders that hold only them, are not a
    // daemon's doing.
    let planted: Vec<String> = PLATFORM_CONFIGS
        .iter()
        .flat_map(|config| {
            let mut path = PathBuf::new();
            Path::new(config).iter().map(move |part| {
                path.push(part);
                path.display().to_string()
            })
        })
        .collect();
    let left: Vec<String> = contents(home.path())
        .into_iter()
        .filter(|path| !planted.contains(path))
        .collect();
    // Daemons that share a folder fail each other's tests: say what they
    // left before what they printed.
    if !left.is_empty() {
        return left;
    }
    assert!(
        output.status.success(),
        "the `{target}` tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    left
}

#[test]
fn no_test_that_runs_the_daemon_touches_the_platforms_folders() {
    let mut offenders = Vec::new();
    for target in TARGETS {
        let left = left_in_the_platforms_folders_by(target);
        if !left.is_empty() {
            offenders.push((target, left));
        }
    }
    assert!(
        offenders.is_empty(),
        "these test targets wrote to the platform's folders, which a developer's \
         real daemon uses; every daemon a test starts needs folders of its own \
         (LADDERBACK_DATA_DIR, LADDERBACK_CONFIG_DIR): {offenders:#?}"
    );
}
