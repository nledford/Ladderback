//! Finds the build this program comes from (§16, decision 90) and hands it to
//! the program as `LADDERBACK_BUILT_COMMIT`, `LADDERBACK_BUILT_DIRTY` and
//! `LADDERBACK_BUILT_RELEASE` (`src/build_info.rs`). A packager names the
//! commit in `LADDERBACK_BUILD_COMMIT`; otherwise `git` is asked about the
//! package's work tree. No `git`, or no work tree of the package's own, is a
//! build whose commit is unknown, never a failed build. What the answers mean
//! is `src/build_info/probe.rs`, which the program's tests test.

use std::env;
use std::path::{Path, PathBuf};
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
    let asked = if named.as_deref().is_some_and(probe::is_commit) {
        None
    } else {
        canonical.as_deref().and_then(|package| {
            // Found before `git` answers, so a change made while it answers
            // lands in a file watched and the next build runs the script.
            let watch = Watch::of(package);
            Answers::of(package).map(|answers| (answers, watch))
        })
    };
    let found = probe::found(
        env!("CARGO_PKG_VERSION"),
        canonical.as_deref().unwrap_or(package),
        named.as_deref(),
        asked
            .as_ref()
            .map(|(answers, _)| answers.checkout())
            .as_ref(),
    );
    if let Some((_, watch)) = &asked {
        watch.instruct(found.release);
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

/// What has Cargo run the script again when the answers may have changed: a
/// commit, a checkout, the release tag, the index, or a change to a tracked
/// file (so `.dirty` is right; an untracked file doesn't count). Which files
/// in the git folder, and why no other ref's, is `probe::watched`.
struct Watch {
    /// `git symbolic-ref -q HEAD`: none when HEAD is detached.
    branch: Option<String>,
    /// Whether the release tag exists.
    tag: bool,
    /// The files either answer may watch that exist, each by the name
    /// `probe::watched` gives it and where it is.
    existing: Vec<(String, PathBuf)>,
    /// The tracked files that exist.
    tracked: Vec<PathBuf>,
}

impl Watch {
    /// The files around `package` that may be watched, as they are now.
    fn of(package: &Path) -> Self {
        let version = env!("CARGO_PKG_VERSION");
        let branch =
            git(package, &["symbolic-ref", "-q", "HEAD"]).map(|branch| branch.trim().to_owned());
        let tag = git(
            package,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &probe::release_ref(version),
            ],
        )
        .is_some();
        // Whether the build is the release is `git`'s answer, still to come.
        let mut names: Vec<String> = [false, true]
            .into_iter()
            .flat_map(|is_release| {
                probe::watched(
                    version,
                    branch.as_deref(),
                    probe::ReleaseTag::of(is_release, tag),
                )
            })
            .flatten()
            .collect();
        names.sort();
        names.dedup();
        let mut asked = vec!["rev-parse"];
        for name in &names {
            asked.push("--git-path");
            asked.push(name);
        }
        let in_git = git(package, &asked).unwrap_or_default();
        // `--git-path` prints one line for each path asked, in order.
        let existing = names
            .into_iter()
            .zip(in_git.lines())
            .map(|(name, path)| (name, package.join(path)))
            .filter(|(_, path)| path.exists())
            .collect();
        let tracked = git(package, &["ls-files", "-z"])
            .unwrap_or_default()
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| package.join(path))
            .filter(|path| path.exists())
            .collect();
        Self {
            branch,
            tag,
            existing,
            tracked,
        }
    }

    /// Has Cargo watch the files that bear on a build that `is_release` or
    /// isn't. Of each of `probe::watched`'s groups, the first file that
    /// existed is watched: Cargo reruns a script on every build while a path
    /// it watches is missing, and only once for one deleted since.
    fn instruct(&self, is_release: bool) {
        let groups = probe::watched(
            env!("CARGO_PKG_VERSION"),
            self.branch.as_deref(),
            probe::ReleaseTag::of(is_release, self.tag),
        );
        let path = |name: &str| {
            self.existing
                .iter()
                .find(|(known, _)| known == name)
                .map(|(_, path)| path)
        };
        let chosen = probe::chosen(&groups, |name| path(name).is_some());
        let in_git = chosen.into_iter().filter_map(path);
        for path in in_git.chain(&self.tracked) {
            // A path Cargo's instructions can't carry (not UTF-8, or a line
            // break in its name) is left unwatched.
            if let Some(path) = path.to_str().filter(|path| !path.contains('\n')) {
                println!("cargo:rerun-if-changed={path}");
            }
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
