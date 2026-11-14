//! Steps for the `@e2e` scenarios of `features/interop/*.feature`: the real
//! `ladderback import-profile` and `export-profile` run on real files in the
//! scenario's scratch folder, and what they wrote is read back as TOML. The
//! golden scenario also runs the stock `rustic` CLI on the exported profile
//! and compares its forget with Ladderback's own. The scenarios of
//! `features/cli/output.feature` run commands the same way, through [`run`]
//! (see `cli_output.rs`).
//!
//! The scratch folder holds `work/` (the folder the command runs in, where
//! profiles are written and where rustic looks for the profiles one uses
//! last), `home/` (the home folder, which holds rustic's config folder),
//! `config/` and `data/` (Ladderback's own, as `LADDERBACK_CONFIG_DIR` and
//! `LADDERBACK_DATA_DIR` name them).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use domain::SnapshotId;

use super::Agent;
use super::blocking;
use super::forget_steps::snapshots_of;

/// What the scenario set up, and what the last command did.
#[derive(Default)]
pub struct InteropE2e {
    /// The environment variables the commands run with: rustic's, and the
    /// ones a scenario sets to choose colour or language.
    pub(super) env: Vec<(String, String)>,
    /// Whether the commands run with a standard output nobody reads: a pipe
    /// whose reader has gone, so that a write to it fails.
    pub(super) stdout_unread: bool,
    /// The same for standard error.
    pub(super) stderr_unread: bool,
    /// Whether the commands run with no descriptor for standard output at
    /// all, as a shell starts one with `>&-`.
    pub(super) stdout_not_open: bool,
    /// The name the commands are run by, which the scenario's `PATH` finds
    /// as a shell would; the built program's path when unset.
    pub(super) program: Option<String>,
    pub(super) ran: Option<Ran>,
    /// The profile the last export wrote on stdout.
    exported: Option<String>,
    /// The golden scenario: rustic is not installed, so the scenario stops
    /// at the step that needed it.
    rustic_missing: bool,
    /// Every snapshot, before rustic and Ladderback forgot.
    before: Vec<SnapshotId>,
    /// What rustic's dry run would have removed.
    rustic_removes: BTreeSet<SnapshotId>,
}

/// What one run of `ladderback` printed, and how it ended.
pub(super) struct Ran {
    pub(super) success: bool,
    /// `1` is a refusal the command reports; a panic ends with `101`.
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

/// `{scratch}` in a step's text stands for the scenario's scratch folder,
/// `{agent}` for the agent's ID, and the placeholders `long_texts` knows for
/// what they say.
fn expand(agent: &Agent, text: &str) -> String {
    let text = text
        .replace("{scratch}", &agent.scratch().display().to_string())
        .replace(
            "{agent}",
            &agent.agent_id.as_uuid().hyphenated().to_string(),
        );
    super::config_steps::long_texts(&text)
}

fn work_dir(agent: &Agent) -> PathBuf {
    agent.scratch().join("work")
}

fn home_dir(agent: &Agent) -> PathBuf {
    agent.scratch().join("home")
}

/// Where rustic keeps its profiles: its config folder under the home folder,
/// the platform's way.
fn rustic_config_dir(agent: &Agent) -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir(agent).join("Library/Application Support/rustic")
    } else {
        home_dir(agent).join(".config/rustic")
    }
}

fn config_path(agent: &Agent) -> PathBuf {
    agent.scratch().join("config").join("config.toml")
}

fn write_profile(dir: PathBuf, name: &str, text: &str) {
    fs::create_dir_all(&dir).expect("a profile folder");
    fs::write(dir.join(name), text).expect("a profile");
}

/// The text of a step's docstring as the file it stands for: cucumber hands
/// it over with the newline after its opening quotes, and without the one a
/// file ends with.
fn file_text(agent: &Agent, step: &Step) -> String {
    let text = step.docstring.as_deref().expect("the step has a text");
    format!("{}\n", expand(agent, text.trim_start_matches('\n')))
}

/// The write end of a pipe whose reader has gone: a stream nothing can be
/// written to, as the standard output of a command in a pipeline whose next
/// command has ended. Made through the spawn gate, so no child another
/// scenario starts meanwhile inherits the read end and keeps the pipe
/// readable (#255, decision 58).
pub(super) fn pipe_without_reader() -> std::io::PipeWriter {
    let (reader, writer) = spawn_gate::hold(std::io::pipe).expect("a pipe");
    drop(reader);
    writer
}

/// Runs `ladderback` with `args` in the working folder, with an environment
/// of its own: the scratch folders, English, and the variables the scenario
/// set. Nothing of the developer's environment reaches it.
pub(super) async fn run(agent: &mut Agent, args: &[&str]) {
    fs::create_dir_all(work_dir(agent)).expect("the working folder");
    fs::create_dir_all(home_dir(agent)).expect("the home folder");
    let program = agent
        .interop
        .program
        .as_deref()
        .unwrap_or(super::LADDERBACK);
    // Only a shell can start a program with a descriptor closed.
    let mut command = if agent.interop.stdout_not_open {
        let mut shell = Command::new("/bin/sh");
        shell.args(["-c", r#"exec "$0" "$@" >&-"#, program]);
        shell
    } else {
        Command::new(program)
    };
    command
        .args(args)
        .current_dir(work_dir(agent))
        .env_clear()
        .env("HOME", home_dir(agent))
        .env("LADDERBACK_CONFIG_DIR", agent.scratch().join("config"))
        .env("LADDERBACK_DATA_DIR", agent.scratch().join("data"))
        .env("LANGUAGE", "en-US");
    for (name, value) in &agent.interop.env {
        command.env(name, value);
    }
    // What `output` would do, but started through the spawn gate.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if agent.interop.stdout_unread {
        command.stdout(pipe_without_reader());
    }
    if agent.interop.stderr_unread {
        command.stderr(pipe_without_reader());
    }
    let output = blocking(move || {
        spawn_gate::hold(|| command.spawn())
            .expect("ladderback runs")
            .wait_with_output()
            .expect("ladderback ran")
    })
    .await;
    agent.interop.ran = Some(Ran {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy_owned(output.stdout),
        stderr: String::from_utf8_lossy_owned(output.stderr),
    });
}

pub(super) fn ran(agent: &Agent) -> &Ran {
    agent.interop.ran.as_ref().expect("a command ran")
}

/// Whatever the last command said.
pub(super) fn said(agent: &Agent) -> String {
    let ran = ran(agent);
    format!("{}\n{}", ran.stdout, ran.stderr)
}

fn assert_succeeded(agent: &Agent) {
    let ran = ran(agent);
    assert!(ran.success, "the command was refused: {}", ran.stderr);
}

fn assert_refused_naming(agent: &Agent, text: &str) {
    let ran = ran(agent);
    assert_eq!(
        ran.code,
        Some(1),
        "the command should be refused: {}{}",
        ran.stdout,
        ran.stderr
    );
    assert!(
        ran.stderr.contains(text),
        "the refusal should name {text:?}: {}",
        ran.stderr
    );
}

fn assert_stderr_names(agent: &Agent, text: &str) {
    assert_succeeded(agent);
    let ran = ran(agent);
    assert!(
        ran.stderr.contains(text),
        "the notices should name {text:?}: {}",
        ran.stderr
    );
}

/// The value at `path` of a parsed file. A segment names a key; in an array
/// it is the position of an item, or the `id` of the item that has it.
fn lookup<'a>(root: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    let mut found = root;
    for segment in path.split('.') {
        found = match found {
            toml::Value::Table(table) => table.get(segment)?,
            toml::Value::Array(items) => match segment.parse::<usize>() {
                Ok(position) => items.get(position)?,
                Err(_) => items
                    .iter()
                    .find(|item| item.get("id").and_then(toml::Value::as_str) == Some(segment))?,
            },
            _ => return None,
        };
    }
    Some(found)
}

fn parse(text: &str) -> toml::Value {
    toml::Value::Table(text.parse().expect("the file is TOML"))
}

fn saved_config(agent: &Agent) -> toml::Value {
    parse(&fs::read_to_string(config_path(agent)).expect("the config file"))
}

fn exported_profile(agent: &Agent) -> toml::Value {
    parse(
        agent
            .interop
            .exported
            .as_deref()
            .expect("a plan was exported"),
    )
}

/// Checks each row of `| key | value |`: the value is TOML.
fn assert_has(agent: &Agent, step: &Step, found: &toml::Value) {
    let table = step.table.as_ref().expect("the step has a table");
    for row in table.rows.iter().skip(1) {
        let [key, value] = row.as_slice() else {
            panic!("a row of a key and a value: {row:?}")
        };
        let expected: toml::Table = format!("v = {}", expand(agent, value))
            .parse()
            .unwrap_or_else(|error| panic!("{value:?} is not a TOML value: {error}"));
        assert_eq!(
            lookup(found, key),
            expected.get("v"),
            "the value at {key:?} in {found:#?}"
        );
    }
}

#[given(expr = "a rustic profile {string} holding:")]
fn a_profile(agent: &mut Agent, name: String, step: &Step) {
    let text = file_text(agent, step);
    write_profile(work_dir(agent), &name, &text);
}

#[given(expr = "a rustic profile {string} in rustic's config folder holding:")]
fn a_profile_in_rustics_config_folder(agent: &mut Agent, name: String, step: &Step) {
    let text = file_text(agent, step);
    write_profile(rustic_config_dir(agent), &name, &text);
}

#[given(expr = "the source folder {string} is a link to the folder {string}")]
fn a_linked_source(agent: &mut Agent, link: String, real: String) {
    let real = expand(agent, &real);
    std::fs::create_dir_all(&real).expect("the folder is made");
    std::os::unix::fs::symlink(&real, expand(agent, &link)).expect("the link is made");
}

/// `folders` is a list, `, ` between two.
#[given(expr = "the folders {string} exist")]
fn folders_exist(agent: &mut Agent, folders: String) {
    for folder in folders.split(", ") {
        std::fs::create_dir_all(expand(agent, folder)).expect("the folder is made");
    }
}

#[given(expr = "a rustic profile {string} larger than 1 MiB")]
fn a_huge_profile(agent: &mut Agent, name: String) {
    // One byte past the cap, all of it a comment, so that it is TOML.
    let text = format!("#{}\n", "x".repeat(1 << 20));
    write_profile(work_dir(agent), &name, &text);
}

#[given(expr = "rustic's environment sets {string} to {string}")]
fn rustics_environment(agent: &mut Agent, name: String, value: String) {
    agent.interop.env.push((name, value));
}

#[given(expr = "the repository's password is in the file {string}")]
fn password_in_file(agent: &mut Agent, path: String) {
    fs::write(expand(agent, &path), crate::backup_steps::PASSWORD).expect("the password file");
}

#[given(expr = "the repository holds {int} snapshots of another agent's plan {string}")]
async fn another_agents_snapshots(agent: &mut Agent, count: usize, plan: String) {
    // Another agent: its own ID, which is never this agent's.
    let other = domain::AgentId::from_uuid(domain::Uuid::from_u128(0x07e5));
    let tags = [
        format!("ladderback-agent:{}", other.as_uuid().hyphenated()),
        format!("ladderback-plan:{plan}"),
    ];
    for _ in 0..count {
        crate::adopt_steps::back_up_with_tags(agent, &tags).await;
    }
}

#[when(expr = "the owner imports the profile {string}")]
async fn imports(agent: &mut Agent, name: String) {
    let file = work_dir(agent).join(name);
    let file = file.to_str().expect("a UTF-8 path").to_owned();
    run(agent, &["import-profile", &file]).await;
}

#[when(expr = "the owner imports the profile {string} as the repository {string}")]
async fn imports_as(agent: &mut Agent, name: String, alias: String) {
    let file = work_dir(agent).join(name);
    let file = file.to_str().expect("a UTF-8 path").to_owned();
    run(agent, &["import-profile", &file, "--repo-name", &alias]).await;
}

#[when(expr = "the owner exports the plan {string}")]
async fn exports(agent: &mut Agent, plan: String) {
    run(agent, &["export-profile", "--plan", &plan]).await;
    let ran = ran(agent);
    agent.interop.exported = ran.success.then(|| ran.stdout.clone());
}

#[when(expr = "the export is kept as the profile {string}")]
fn keeps_export(agent: &mut Agent, name: String) {
    let text = agent.interop.exported.clone().expect("a plan was exported");
    write_profile(work_dir(agent), &name, &text);
}

#[when("the config file is removed")]
fn removes_config(agent: &mut Agent) {
    fs::remove_file(config_path(agent)).expect("a config file to remove");
}

#[when("rustic is asked which snapshots its forget would remove, from the exported profile")]
async fn rustic_dry_run(agent: &mut Agent) {
    // Placed right before rustic is run, so that a machine without it says
    // so, and everything before still ran.
    let installed = blocking(|| {
        let mut version = Command::new("rustic");
        version
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_gate::hold(|| version.spawn()).is_ok_and(|mut child| child.wait().is_ok())
    })
    .await;
    if !installed {
        agent.interop.rustic_missing = true;
        testkit::skip!("rustic is not on the PATH");
    }
    let profile = agent.scratch().join("exported-profile.toml");
    fs::write(
        &profile,
        agent
            .interop
            .exported
            .as_deref()
            .expect("a plan was exported"),
    )
    .expect("the profile");
    agent.interop.before = snapshots_of(agent)
        .await
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let work = work_dir(agent);
    let home = home_dir(agent);
    let output = blocking(move || {
        let mut rustic = Command::new("rustic");
        rustic
            .args(["-P"])
            .arg(&profile)
            .args(["forget", "--dry-run", "--json"])
            .current_dir(work)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("RUSTIC_NO_CACHE", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        spawn_gate::hold(|| rustic.spawn())
            .expect("rustic runs")
            .wait_with_output()
            .expect("rustic ran")
    })
    .await;
    assert!(
        output.status.success(),
        "rustic refused the exported profile: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Stdout is the JSON alone: rustic's log lines go to stderr.
    let groups: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("rustic's forget prints JSON");
    let mut removes = BTreeSet::new();
    for group in groups.as_array().expect("a list of groups") {
        for item in group["items"].as_array().expect("a list of items") {
            if item["keep"] == serde_json::Value::Bool(false) {
                let id = item["snapshot"]["id"].as_str().expect("a snapshot ID");
                removes.insert(SnapshotId::parse(id).expect("a snapshot ID"));
            }
        }
    }
    agent.interop.rustic_removes = removes;
}

#[then("the import succeeds")]
fn import_succeeds(agent: &mut Agent) {
    assert_succeeded(agent);
}

#[then(expr = "the import is refused naming {string}")]
fn import_refused(agent: &mut Agent, text: String) {
    assert_refused_naming(agent, &text);
}

#[then(expr = "the refusal points at line {int} of the profile")]
fn refusal_line(agent: &mut Agent, line: usize) {
    let stderr = &ran(agent).stderr;
    assert!(
        stderr.contains(&format!("line {line}")),
        "the refusal should point at line {line}: {stderr}"
    );
}

#[then("no config file was written")]
fn no_config_file(agent: &mut Agent) {
    assert!(
        !config_path(agent).exists(),
        "a config file was written: {}",
        fs::read_to_string(config_path(agent)).unwrap_or_default()
    );
}

#[then(expr = "the import notes that the key {string} was left out")]
fn notes_key(agent: &mut Agent, key: String) {
    assert_stderr_names(agent, &key);
}

#[then("the import notes that these keys were left out:")]
fn notes_keys(agent: &mut Agent, step: &Step) {
    let table = step.table.as_ref().expect("the step has a table");
    for row in table.rows.iter().skip(1) {
        assert_stderr_names(agent, &row[0]);
    }
}

#[then(expr = "the import notes that the profile {string} wasn't found")]
fn notes_missing_profile(agent: &mut Agent, name: String) {
    assert_stderr_names(agent, &name);
}

#[then(expr = "the import notes that the tag {string} was dropped")]
fn notes_dropped_tag(agent: &mut Agent, tag: String) {
    assert_stderr_names(agent, &tag);
}

#[then("the import warns that pruning will run from one place")]
fn warns_prune(agent: &mut Agent) {
    assert_stderr_names(agent, "Pruning will run from one place");
}

#[then("the import warns that backup hooks now run once per plan")]
fn warns_hooks(agent: &mut Agent) {
    assert_stderr_names(agent, "once per plan");
}

#[then("the import warns that the profile's environment was left out")]
fn warns_env(agent: &mut Agent) {
    assert_stderr_names(agent, "global.env");
}

#[then(expr = "the import warns that the secret {string} is written in the config")]
fn warns_inline_secret(agent: &mut Agent, field: String) {
    assert_stderr_names(agent, &field);
}

#[then(
    expr = "the import warns that the location in {string} holds a password written in the config"
)]
fn warns_location_secret(agent: &mut Agent, field: String) {
    assert_stderr_names(
        agent,
        &format!("The location in {field} holds a password as text"),
    );
}

/// `{walked scratch}` in the whole path stands for the scratch folder as a
/// backup walks it, its links followed (`/var` is `/private/var` on macOS).
#[then(
    expr = "the import warns that the glob {string} of the plan {string} matches nothing, and to write {string} instead"
)]
fn warns_glob_never_matches(agent: &mut Agent, glob: String, plan: String, whole: String) {
    let walked = std::fs::canonicalize(agent.scratch()).expect("the scratch folder resolves");
    let whole = whole.replace("{walked scratch}", &walked.display().to_string());
    let line = glob_warning(agent, &glob, &plan);
    assert!(line.contains(&format!("as in {whole}.")), "{line}");
}

#[then(
    expr = "the import warns that the glob {string} of the plan {string} matches nothing, and to write the whole path, starting with the folder of the source it belongs to"
)]
fn warns_glob_never_matches_without_path(agent: &mut Agent, glob: String, plan: String) {
    let line = glob_warning(agent, &glob, &plan);
    assert!(
        line.contains(
            "Write the whole path instead, starting with the folder of the source it belongs to."
        ),
        "{line}"
    );
    assert!(!line.contains("as in "), "{line}");
}

/// The line of the import's report that says the glob `glob` of the plan
/// `plan` matches nothing, after an import that succeeded.
fn glob_warning(agent: &mut Agent, glob: &str, plan: &str) -> String {
    assert_succeeded(agent);
    let said = said(agent);
    let warning = format!("The glob {glob} of the plan {plan} matches nothing");
    said.lines()
        .find(|line| line.contains(&warning))
        .unwrap_or_else(|| panic!("no {warning:?} in: {said}"))
        .to_owned()
}

#[then(
    expr = "the import warns that the location in {string} holds a credential that belongs in rclone's own configuration"
)]
fn warns_rclone_location_secret(agent: &mut Agent, field: String) {
    assert_succeeded(agent);
    let said = said(agent);
    let warning = format!("The location in {field} holds a credential as text");
    let line = said
        .lines()
        .find(|line| line.contains(&warning))
        .unwrap_or_else(|| panic!("no {warning:?} in: {said}"));
    // rclone is started with the location in its arguments, where a `${…}`
    // reference is refused, so the warning sends the credential to rclone.
    assert!(
        line.contains("rclone's own configuration file")
            && line.contains("rclone:remote:path")
            && !line.contains("${"),
        "{line}"
    );
}

#[then(expr = "the import gives no warning about {string}")]
fn no_warning_about(agent: &mut Agent, field: String) {
    assert_succeeded(agent);
    let said = said(agent);
    assert!(!said.contains(&field), "{said}");
}

#[then(expr = "the import never shows {string}")]
fn import_never_shows(agent: &mut Agent, text: String) {
    let said = said(agent);
    assert!(!said.contains(&text), "{said}");
}

#[then("the saved config has:")]
fn saved_config_has(agent: &mut Agent, step: &Step) {
    assert_has(agent, step, &saved_config(agent));
}

#[then(expr = "the saved config doesn't set {string}")]
fn saved_config_doesnt_set(agent: &mut Agent, path: String) {
    let config = saved_config(agent);
    assert_eq!(lookup(&config, &path), None, "{config:#?}");
}

#[then("the export succeeds")]
fn export_succeeds(agent: &mut Agent) {
    assert_succeeded(agent);
}

#[then("the exported profile has:")]
fn exported_has(agent: &mut Agent, step: &Step) {
    assert_has(agent, step, &exported_profile(agent));
}

#[then(expr = "the exported profile doesn't set {string}")]
fn exported_doesnt_set(agent: &mut Agent, path: String) {
    let profile = exported_profile(agent);
    assert_eq!(lookup(&profile, &path), None, "{profile:#?}");
}

#[then(expr = "the exported profile doesn't hold {string}")]
fn exported_doesnt_hold(agent: &mut Agent, text: String) {
    let profile = agent
        .interop
        .exported
        .as_deref()
        .expect("a plan was exported");
    assert!(!profile.contains(&text), "{profile}");
}

#[then(expr = "the exported profile reads the password from the keyring entry {string}")]
fn exported_reads_keyring(agent: &mut Agent, entry: String) {
    let (service, name) = entry.split_once('/').expect("a service and a name");
    let command = if cfg!(target_os = "macos") {
        format!("security find-generic-password -s {service} -a {name} -w")
    } else {
        format!("secret-tool lookup service {service} username {name}")
    };
    let expected = toml::Value::String(command);
    assert_eq!(
        lookup(&exported_profile(agent), "repository.password-command"),
        Some(&expected)
    );
}

#[then(expr = "the export is refused naming {string}")]
fn export_refused(agent: &mut Agent, text: String) {
    assert_refused_naming(agent, &text);
}

#[then("the export made no profile")]
fn export_made_none(agent: &mut Agent) {
    let ran = ran(agent);
    assert!(
        ran.code == Some(1) && ran.stdout.is_empty(),
        "{}",
        said(agent)
    );
}

#[then(expr = "the export never shows {string}")]
fn export_never_shows(agent: &mut Agent, text: String) {
    let said = said(agent);
    assert!(!said.contains(&text), "{said}");
}

#[then(expr = "the export notes that {string} is to be set instead")]
fn export_notes_variable(agent: &mut Agent, variable: String) {
    assert_stderr_names(agent, &variable);
}

#[then("the export warns that rustic's forget needs a keep option")]
fn export_warns_keep_option(agent: &mut Agent) {
    assert_stderr_names(agent, "keep-*");
}

/// The keyring command an export on this platform names.
fn keyring_program() -> &'static str {
    if cfg!(target_os = "macos") {
        "security"
    } else {
        "secret-tool"
    }
}

/// The commands run with `PATH` set to the scratch folder `bin/`, which holds
/// what the scenario put there and nothing else.
fn path_of_scratch_bin(agent: &mut Agent) -> PathBuf {
    let bin = agent.scratch().join("bin");
    fs::create_dir_all(&bin).expect("the bin folder");
    agent
        .interop
        .env
        .push(("PATH".to_owned(), bin.display().to_string()));
    bin
}

#[given("the PATH holds no keyring command")]
fn path_without_keyring_command(agent: &mut Agent) {
    path_of_scratch_bin(agent);
}

#[given("the PATH holds the platform's keyring command")]
fn path_with_keyring_command(agent: &mut Agent) {
    use std::os::unix::fs::PermissionsExt;
    let program = path_of_scratch_bin(agent).join(keyring_program());
    fs::write(&program, "#!/bin/sh\nexit 1\n").expect("the command");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("its mode");
}

/// `secret-tool` comes in a package of its own on Linux, which the warning
/// names; macOS's `security` comes with the system, and the warning names it.
#[then(
    "the export warns that the platform's keyring command isn't on the PATH, naming its package"
)]
fn export_warns_keyring_command(agent: &mut Agent) {
    let program = keyring_program();
    assert_stderr_names(agent, &format!("with {program}, which is not on"));
    if program == "secret-tool" {
        assert_stderr_names(agent, "libsecret package on Fedora");
        assert_stderr_names(agent, "libsecret-tools on Debian and Ubuntu");
    }
}

#[then("the export gives no warning")]
fn export_gives_no_warning(agent: &mut Agent) {
    assert_succeeded(agent);
    let ran = ran(agent);
    assert!(ran.stderr.is_empty(), "{}", ran.stderr);
}

#[then(expr = "rustic would have removed {int} snapshots")]
fn rustic_would_remove(agent: &mut Agent, count: usize) {
    if agent.interop.rustic_missing {
        // The step that needed rustic recorded the skip.
        return;
    }
    assert_eq!(agent.interop.rustic_removes.len(), count);
}

#[then("Ladderback removed exactly the snapshots rustic would have removed")]
async fn same_removals(agent: &mut Agent) {
    if agent.interop.rustic_missing {
        return;
    }
    let after: BTreeSet<SnapshotId> = snapshots_of(agent)
        .await
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let removed: BTreeSet<SnapshotId> = agent
        .interop
        .before
        .iter()
        .filter(|id| !after.contains(*id))
        .cloned()
        .collect();
    assert_eq!(removed, agent.interop.rustic_removes);
}
