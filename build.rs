//! Finds the build this program comes from (§16, decision 90) and hands it to
//! the program as `LADDERBACK_BUILT_COMMIT`, `LADDERBACK_BUILT_DIRTY` and
//! `LADDERBACK_BUILT_RELEASE` (`src/build_info.rs`). A packager names the
//! commit in `LADDERBACK_BUILD_COMMIT`; otherwise `git` is asked about the
//! package's work tree. No `git`, or no work tree of the package's own, is a
//! build whose commit is unknown, never a failed build. What the answers mean
//! is `src/build_info/probe.rs`, which the program's tests test.

use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "src/build_info/probe.rs"]
mod probe;

/// The variable a packager names the commit in.
const NAMED: &str = "LADDERBACK_BUILD_COMMIT";

fn main() {
    // At least one path, always: with none, Cargo reruns the script whenever
    // any file of the package changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/build_info/probe.rs");
    println!("cargo:rerun-if-env-changed={NAMED}");
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let named = env::var(NAMED).ok().filter(|named| !named.is_empty());
    if let Some(named) = named.as_deref().filter(|named| !probe::is_commit(named)) {
        println!(
            "cargo:warning={NAMED}={named:?} is not a commit hash (7 to 64 hex digits); it is ignored"
        );
    }
    let canonical = package.canonicalize().ok();
    // A packager's commit wins: `git` isn't asked.
    let answers = if named.as_deref().is_some_and(probe::is_commit) {
        None
    } else {
        canonical.as_deref().and_then(Answers::of)
    };
    let found = probe::found(
        env!("CARGO_PKG_VERSION"),
        canonical.as_deref().unwrap_or(package),
        named.as_deref(),
        answers.as_ref().map(Answers::checkout).as_ref(),
    );
    if let (Some(canonical), Some(_)) = (&canonical, &answers) {
        watch(canonical);
    }
    for line in probe::instructions(&found) {
        println!("{line}");
    }
}

/// What `git` answered about the work tree around `package`, its top folder
/// canonical.
struct Answers {
    top: String,
    head: String,
    tags: String,
    changes: String,
}

impl Answers {
    /// `None` when `git` couldn't answer: it isn't installed, or `package` is
    /// in no work tree.
    fn of(package: &Path) -> Option<Self> {
        let top = git(package, &["rev-parse", "--show-toplevel"])?;
        let top = Path::new(probe::line(&top)).canonicalize().ok()?;
        Some(Self {
            top: top.to_str()?.to_owned(),
            head: git(package, &["rev-parse", "HEAD"])?,
            tags: git(package, &["tag", "--points-at", "HEAD"])?,
            // `--no-optional-locks`: `status` would otherwise refresh the
            // index, a file this script watches, so it ran again every build.
            changes: git(
                package,
                &[
                    "--no-optional-locks",
                    "status",
                    "--porcelain",
                    "--untracked-files=no",
                ],
            )?,
        })
    }

    fn checkout(&self) -> probe::Checkout<'_> {
        probe::Checkout {
            top: &self.top,
            head: &self.head,
            tags: &self.tags,
            changes: &self.changes,
        }
    }
}

/// Has Cargo run the script again when the answers may have changed: a
/// commit, a checkout, a tag, the index, or a change to a tracked file (so
/// `.dirty` is right; an untracked file doesn't count). Only paths that
/// exist: Cargo reruns a script whose watched path is missing every build.
/// HEAD's log covers a branch whose ref is packed, which `git reset --soft`
/// or `git update-ref` moves without touching any other file watched here.
fn watch(package: &Path) {
    let mut paths = vec![
        "HEAD".to_owned(),
        "logs/HEAD".to_owned(),
        "index".to_owned(),
        "packed-refs".to_owned(),
        "refs/tags".to_owned(),
    ];
    if let Some(branch) = git(package, &["symbolic-ref", "-q", "HEAD"]) {
        paths.push(branch.trim().to_owned());
    }
    let mut asked = vec!["rev-parse".to_owned()];
    for path in &paths {
        asked.push("--git-path".to_owned());
        asked.push(path.clone());
    }
    let asked: Vec<&str> = asked.iter().map(String::as_str).collect();
    let in_git = git(package, &asked).unwrap_or_default();
    let tracked = git(package, &["ls-files", "-z"]).unwrap_or_default();
    let watched = in_git
        .lines()
        .chain(tracked.split('\0'))
        .filter(|path| !path.is_empty())
        .map(|path| package.join(path))
        .filter(|path| path.exists());
    for path in watched {
        // A path Cargo's instructions can't carry (not UTF-8, or a line break
        // in its name) is left unwatched.
        if let Some(path) = path.to_str().filter(|path| !path.contains('\n')) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// What `git` printed with `args` in `folder`, when it ran and succeeded. A
/// name that isn't UTF-8 (a tracked file's) is read with replacement
/// characters, so it names no path that exists, and the rest are still read.
fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(folder)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
