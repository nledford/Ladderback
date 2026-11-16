//! Steps for `features/cli/translations.feature`: `scripts/translations.ts`
//! run by Node as `just translations` and `just accept-translations` run it,
//! through `interop_steps::run` (a scratch home, nothing of the developer's
//! environment), over catalogs in a scratch folder that a git repository of
//! its own keeps, with `git` alone on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cucumber::gherkin::Step;
use cucumber::{given, then, when};

use super::interop_steps::{ran, run};
use super::{Agent, blocking};

/// The script under test, as the repository holds it.
const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/translations.ts");

/// The language the examples translate into.
const GERMAN: &str = "de";

/// The repository the catalogs are kept in.
fn repository(agent: &Agent) -> PathBuf {
    agent.scratch().join("catalogs")
}

/// The folder of catalogs, as `crates/l10n/locales` is in the workspace.
fn locales(agent: &Agent) -> PathBuf {
    repository(agent).join("locales")
}

/// Writes the one file of the catalog of `tag` from the step's text, which
/// cucumber hands over without the newline a file ends with.
fn write_catalog(agent: &Agent, tag: &str, step: &Step) {
    let text = step.docstring.as_deref().expect("the step has a text");
    let dir = locales(agent).join(tag);
    fs::create_dir_all(&dir).expect("the catalog's folder");
    fs::write(
        dir.join("main.ftl"),
        format!("{}\n", text.trim_start_matches('\n')),
    )
    .expect("the catalog");
}

/// The first `name` on the test's own `PATH`.
fn on_path(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("a PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{name} is on the PATH"))
}

/// Runs the script with Node, `git` alone on its `PATH` and no git
/// configuration but the repository's own.
async fn run_script(agent: &mut Agent, args: &[&str]) {
    let git = on_path("git");
    let git_dir = git.parent().expect("git is in a folder");
    agent.interop.env = vec![
        ("PATH".to_owned(), git_dir.display().to_string()),
        ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()),
    ];
    let locales = locales(agent).display().to_string();
    let mut all = vec![SCRIPT];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--locales", &locales]);
    agent.interop.program = Some(on_path("node").display().to_string());
    run(agent, &all).await;
    agent.interop.program = None;
    agent.interop.env.clear();
}

async fn accept(agent: &mut Agent, ids: &[&str]) {
    let mut args = vec!["accept", GERMAN];
    args.extend_from_slice(ids);
    run_script(agent, &args).await;
}

/// Runs `git` in `dir` with no configuration but the repository's own.
async fn git(agent: &Agent, dir: &Path, args: &[&str]) {
    let mut command = Command::new(on_path("git"));
    command
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", agent.scratch().join("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let shown = format!("git {}", args.join(" "));
    let output = blocking(move || {
        spawn_gate::hold(|| command.spawn())
            .expect("git runs")
            .wait_with_output()
            .expect("git ran")
    })
    .await;
    assert!(
        output.status.success(),
        "{shown} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[given("the English catalog holds:")]
fn english_holds(agent: &mut Agent, step: &Step) {
    write_catalog(agent, "en-US", step);
}

#[given("the German catalog holds:")]
fn german_holds(agent: &mut Agent, step: &Step) {
    write_catalog(agent, GERMAN, step);
}

#[given(expr = "the owner has accepted the German translations of {string} and {string}")]
async fn has_accepted(agent: &mut Agent, first: String, second: String) {
    accept(agent, &[&first, &second]).await;
    let ran = ran(agent);
    assert!(
        ran.success,
        "the translations weren't accepted: {}",
        ran.stderr
    );
}

#[given("the catalogs are committed")]
async fn committed(agent: &mut Agent) {
    let dir = repository(agent);
    fs::create_dir_all(agent.scratch().join("home")).expect("the home folder");
    git(agent, &dir, &["init", "--quiet"]).await;
    git(agent, &dir, &["add", "--all"]).await;
    git(
        agent,
        &dir,
        &[
            "-c",
            "user.name=Owner",
            "-c",
            "user.email=owner@example.com",
            "commit",
            "--quiet",
            "--message",
            "Catalogs",
        ],
    )
    .await;
}

#[when(expr = "the owner accepts the German translation of {string}")]
async fn accepts(agent: &mut Agent, id: String) {
    accept(agent, &[&id]).await;
}

#[when("the owner lists the translations")]
async fn lists(agent: &mut Agent) {
    run_script(agent, &["list"]).await;
}

#[then("the translations listed are:")]
fn listed(agent: &mut Agent, step: &Step) {
    let expected = step.docstring.as_deref().expect("the step has a text");
    let expected = format!("{}\n", expected.trim_matches('\n'));
    let ran = ran(agent);
    assert_eq!(ran.stdout, expected, "standard error: {}", ran.stderr);
}
