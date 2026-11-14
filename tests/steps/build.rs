//! Steps for `features/cli/build.feature`: the build the real `ladderback`
//! names, against what `git` says of this checkout now. The test binary is
//! built from the same work tree just before, so the two agree unless the
//! build named the wrong commit or missed a change.

use std::path::Path;
use std::process::{Command, Stdio};

use api::BuildDto;
use cucumber::then;
use serde_json::Value;

use super::interop_steps::ran;
use super::{Agent, blocking};

/// What `git` says of this checkout: the build the binary should name;
/// `None` when it can't say (no `git`, no work tree of this package's own)
/// or a packager named the commit.
fn this_checkout() -> Option<BuildDto> {
    if std::env::var_os("LADDERBACK_BUILD_COMMIT").is_some_and(|named| !named.is_empty()) {
        return None;
    }
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let top = git(package, &["rev-parse", "--show-toplevel"])?;
    let same = Path::new(top.trim()).canonicalize().ok()? == package.canonicalize().ok()?;
    if !same {
        return None;
    }
    let version = env!("CARGO_PKG_VERSION");
    let commit = git(package, &["rev-parse", "HEAD"])?.trim().to_owned();
    let tags = git(package, &["tag", "--points-at", "HEAD"])?;
    let changes = git(package, &["status", "--porcelain", "--untracked-files=no"])?;
    Some(BuildDto {
        version: version.to_owned(),
        commit: Some(commit),
        dirty: !changes.trim().is_empty(),
        release: tags.lines().any(|tag| tag.trim() == format!("v{version}")),
    })
}

/// What `git` printed with `args` in `folder`, when it ran and succeeded.
fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = spawn_gate::hold(|| command.spawn()).ok()?;
    let output = child.wait_with_output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

async fn expected() -> Option<BuildDto> {
    blocking(this_checkout).await
}

#[then("standard output names this checkout's build")]
async fn output_names_the_build(agent: &mut Agent) {
    let Some(build) = expected().await else {
        testkit::skip!("git can't say which build this checkout is");
    };
    assert_eq!(
        ran(agent).stdout,
        format!("ladderback {}\n", build.display())
    );
}

#[then("the daemon's log names this checkout's build as it starts")]
async fn log_names_the_build(agent: &mut Agent) {
    let Some(build) = expected().await else {
        testkit::skip!("git can't say which build this checkout is");
    };
    let line = format!("ladderback {} starting", build.display());
    let log = &ran(agent).stderr;
    assert!(log.contains(&line), "no {line:?} in: {log}");
}

/// The answer's body, as JSON.
fn answer(agent: &Agent) -> Value {
    let body = &agent.web.response().body;
    serde_json::from_str(body).unwrap_or_else(|error| panic!("not JSON ({error}): {body}"))
}

#[then(
    "the answer names this checkout's build: its version, its commit and whether it had changes"
)]
async fn answer_names_the_build(agent: &mut Agent) {
    let Some(build) = expected().await else {
        testkit::skip!("git can't say which build this checkout is");
    };
    let answer = answer(agent);
    assert_eq!(answer["version"], build.version.as_str(), "{answer}");
    assert_eq!(
        answer["commit"],
        build.commit.as_deref().unwrap_or_default(),
        "{answer}"
    );
    assert_eq!(answer["dirty"], build.dirty, "{answer}");
    assert_eq!(answer["release"], build.release, "{answer}");
}

#[then("the answer doesn't name the build")]
fn answer_names_no_build(agent: &mut Agent) {
    let answer = answer(agent);
    for field in ["version", "commit", "dirty", "release"] {
        assert!(answer.get(field).is_none(), "{field} in {answer}");
    }
}
