//! The profile of one plan, as rustic 0.11.4 reads it.
//!
//! The text is written by hand, table by table in a fixed order, rather than
//! by a serializer: the same plan gives the same text every time, a table
//! that has nothing to say is not there, and a reader of the file sees the
//! plan's own settings first. `toml` reads the result back in the tests, and
//! the stock `rustic` reads it in the golden test of the CLI.
//!
//! What goes in:
//!
//! - `[global.hooks]`: the agent's hooks, `[repository]` with the way to read
//!   the password and the options that are text, `[repository.hooks]`;
//! - one `[[backup.snapshots]]` entry named by the plan's ID, with the plan's
//!   sources, label, host, tags (its own, then the two that make a snapshot
//!   this plan's), filter and hooks;
//! - `[forget]` with the plan's retention and `group-by = ""`, and
//!   `[snapshot-filter]` that selects the snapshots carrying both of the
//!   plan's tags. A forget over all snapshots of one group would otherwise
//!   reach the other plans', and the other agents'.
//!
//! What stays out: the schedule, the retry and the repository's maintenance
//! (rustic has no place for them, and a forget that prunes would run outside
//! Ladderback's own maintenance), and every secret. A password is written as
//! the command or the file that reads it, and an option that is a reference
//! or a secret is left out with the variable that rustic reads it from.

use crate::KeyringCommand;
use app::ports::{ExportRefusal, PlanExport, ProfileNotice, RenderedProfile};
use domain::planning::{
    GlobFile, HookEntry, Hooks, KeepCount, KeepWithin, OnFailure, RepositorySettings,
};
use domain::secrets::{
    KeyringEntry, Part, SecretRef, Template, is_secret_option, rclone_secret, url_credentials,
    url_passwords,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The profile of `export`, and the options it left out. A keyring password
/// is read with `keyring`, looked for in `path`.
pub(crate) fn render(
    export: &PlanExport,
    keyring: KeyringCommand,
    path: &[PathBuf],
) -> Result<RenderedProfile, ExportRefusal> {
    let plan = &export.plan;
    // rustic reads `a,b` as the two tags `a` and `b`, so the tag can't be
    // written as it is.
    if let Some(tag) = plan.tags.iter().find(|tag| tag.as_str().contains(',')) {
        return Err(ExportRefusal::TagComma {
            tag: tag.as_str().to_owned(),
        });
    }
    let mut out = Out::default();
    out.text.push_str(&format!(
        "# A rustic profile for the Ladderback plan {}, written by Ladderback.\n",
        plan.id.as_str()
    ));
    hooks(&mut out, "global.hooks", &export.agent_hooks);

    let (repository, mut notices) = repository(&export.repository.settings, keyring, path)?;
    if plan.retention.is_empty() {
        notices.push(ProfileNotice::RetentionEmpty);
    }
    out.section("repository");
    out.string("repository", &repository.location);
    if let Some(hot) = &repository.hot {
        out.string("repo-hot", hot);
    }
    out.string(repository.password.0, &repository.password.1);
    for (header, options) in repository.options {
        if !options.is_empty() {
            out.section(&format!("repository.{header}"));
            for (name, value) in options {
                out.string(&key(&name), &value);
            }
        }
    }
    hooks(&mut out, "repository.hooks", &export.repository.hooks);

    out.array_section("backup.snapshots");
    out.string("name", plan.id.as_str());
    out.list("sources", plan.sources.iter().map(|source| source.as_str()));
    if let Some(label) = &plan.label {
        out.string("label", label);
    }
    if let Some(host) = plan.host.as_ref().or(export.agent_hostname.as_ref()) {
        out.string("host", host.as_str());
    }
    let [plan_tag, agent_tag] = export.tags.tags();
    out.list(
        "tags",
        plan.tags
            .iter()
            .map(|tag| tag.as_str())
            .chain([plan_tag.as_str(), agent_tag.as_str()]),
    );
    let filter = &plan.filter;
    out.list_if_any("globs", &filter.globs);
    out.list_if_any("iglobs", &filter.iglobs);
    for (name, files) in [
        ("glob-files", &filter.glob_files),
        ("iglob-files", &filter.iglob_files),
    ] {
        if !files.is_empty() {
            out.list(name, files.iter().map(GlobFile::as_str));
        }
    }
    out.flag("git-ignore", filter.git_ignore);
    out.flag("no-require-git", filter.no_require_git);
    out.list_if_any("custom-ignorefiles", &filter.custom_ignorefiles);
    out.list_if_any("exclude-if-present", &filter.exclude_if_present);
    out.list_if_any("exclude-if-xattr", &filter.exclude_if_xattr);
    out.flag("one-file-system", filter.one_file_system);
    hooks(&mut out, "backup.snapshots.hooks", &plan.hooks);

    out.section("forget");
    out.string("group-by", "");
    let retention = &plan.retention;
    for (name, count) in [
        ("keep-last", retention.keep_last),
        ("keep-minutely", retention.keep_minutely),
        ("keep-hourly", retention.keep_hourly),
        ("keep-daily", retention.keep_daily),
        ("keep-weekly", retention.keep_weekly),
        ("keep-monthly", retention.keep_monthly),
        ("keep-quarter-yearly", retention.keep_quarter_yearly),
        ("keep-half-yearly", retention.keep_half_yearly),
        ("keep-yearly", retention.keep_yearly),
    ] {
        if let Some(count) = count {
            out.integer(name, count);
        }
    }
    for (name, span) in [
        ("keep-within", &retention.keep_within),
        ("keep-within-minutely", &retention.keep_within_minutely),
        ("keep-within-hourly", &retention.keep_within_hourly),
        ("keep-within-daily", &retention.keep_within_daily),
        ("keep-within-weekly", &retention.keep_within_weekly),
        ("keep-within-monthly", &retention.keep_within_monthly),
        (
            "keep-within-quarter-yearly",
            &retention.keep_within_quarter_yearly,
        ),
        (
            "keep-within-half-yearly",
            &retention.keep_within_half_yearly,
        ),
        ("keep-within-yearly", &retention.keep_within_yearly),
    ] {
        if let Some(span) = span {
            out.within(name, span);
        }
    }

    // One entry, with both tags: rustic ANDs the tags of one entry, and ORs
    // separate entries.
    out.section("snapshot-filter");
    out.list("filter-tags", [format!("{plan_tag},{agent_tag}").as_str()]);

    Ok(RenderedProfile {
        text: out.text,
        notices,
    })
}

// --- The repository --------------------------------------------------------

/// What `[repository]` holds.
struct Repository {
    location: String,
    hot: Option<String>,
    /// The key and the value that say how to read the password.
    password: (&'static str, String),
    /// `options`, `options-hot` and `options-cold`: the table's name, and
    /// the options of it that are text.
    options: Vec<(&'static str, BTreeMap<String, String>)>,
}

/// The repository's own tables of options: the key of its table in the
/// profile and in the config, and the prefixes of the environment variables
/// rustic reads an option from: its own (`RUSTIC_REPO_OPT_FOO_BAR` is
/// `foo-bar`), and OpenDAL's (`OPENDAL_FOO_BAR` is `foo_bar`).
struct OptionTable {
    key: &'static str,
    rustic: &'static str,
    opendal: &'static str,
}

const OPTIONS: OptionTable = OptionTable {
    key: "options",
    rustic: "RUSTIC_REPO_OPT_",
    opendal: "OPENDAL_",
};
const OPTIONS_HOT: OptionTable = OptionTable {
    key: "options-hot",
    rustic: "RUSTIC_REPO_OPTHOT_",
    opendal: "OPENDALHOT_",
};
const OPTIONS_COLD: OptionTable = OptionTable {
    key: "options-cold",
    rustic: "RUSTIC_REPO_OPTCOLD_",
    opendal: "OPENDALCOLD_",
};

fn repository(
    settings: &RepositorySettings,
    keyring: KeyringCommand,
    path: &[PathBuf],
) -> Result<(Repository, Vec<ProfileNotice>), ExportRefusal> {
    let mut notices = Vec::new();
    let location = plain("repository", &settings.repository)?;
    let hot = settings
        .repo_hot
        .as_deref()
        .map(|hot| plain("repo-hot", hot))
        .transpose()?;
    let password = password(&settings.password, keyring, path, &mut notices)?;
    let mut options = Vec::new();
    for (table, values) in [
        (&OPTIONS, &settings.options),
        (&OPTIONS_HOT, &settings.options_hot),
        (&OPTIONS_COLD, &settings.options_cold),
    ] {
        options.push((table.key, text_options(table, values, &mut notices)?));
    }
    Ok((
        Repository {
            location,
            hot,
            password,
            options,
        },
        notices,
    ))
}

/// What a value of the repository says, when it says only text and no
/// password: a reference is one rustic can't resolve, and a password in a
/// URL or a secret in an rclone connection string would be written out, so
/// the key is refused. A user name without a password is written: it is
/// part of the address, and where it is a token the config holds it as text
/// already.
fn plain(key: &str, value: &str) -> Result<String, ExportRefusal> {
    let text = text_of(value).ok_or_else(|| ExportRefusal::Reference {
        key: key.to_owned(),
    })?;
    // The profile would hold the password as text, in the file and in what
    // `rustic show-config` prints.
    if url_passwords(&text).is_empty() && !rclone_secret(&text) {
        Ok(text)
    } else {
        Err(ExportRefusal::UrlPassword {
            key: key.to_owned(),
        })
    }
}

/// The text a config value stands for, or `None` when it refers to something
/// (or isn't a template at all).
fn text_of(value: &str) -> Option<String> {
    let template = Template::parse(value).ok()?;
    template
        .parts()
        .iter()
        .map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            Part::Reference(_) => None,
        })
        .collect()
}

/// The options of `table` that are text and not secrets. The others are
/// noted as left out with the variable to set instead. An option whose
/// value is an address with a user or a password in it (`demo:pw@host`,
/// with or without its scheme) is a secret, as a token may be the user.
fn text_options(
    table: &OptionTable,
    options: &BTreeMap<String, String>,
    notices: &mut Vec<ProfileNotice>,
) -> Result<BTreeMap<String, String>, ExportRefusal> {
    let mut kept = BTreeMap::new();
    for (name, value) in options {
        match text_of(value) {
            Some(text) if !is_secret_option(name) && url_credentials(&text).is_empty() => {
                kept.insert(name.clone(), text);
            }
            _ => notices.push(ProfileNotice::OptionLeftOut {
                option: name.clone(),
                variable: variable(table, name)?,
            }),
        }
    }
    Ok(kept)
}

/// The environment variable rustic reads the option `name` from: its own for
/// a kebab-case name (`RUSTIC_REPO_OPT_NO_CACHE`), OpenDAL's for a snake_case
/// one (`OPENDAL_SAS_TOKEN`). Both variables give a lower-case name, so any
/// other name can't be set that way.
fn variable(table: &OptionTable, name: &str) -> Result<String, ExportRefusal> {
    let nameable = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
        });
    let (snake, kebab) = (name.contains('_'), name.contains('-'));
    if !nameable || (snake && kebab) {
        return Err(ExportRefusal::Reference {
            key: format!("{}.{name}", table.key),
        });
    }
    let prefix = if snake { table.opendal } else { table.rustic };
    Ok(format!(
        "{prefix}{}",
        name.to_ascii_uppercase().replace('-', "_")
    ))
}

/// The key and the value that make rustic read the password the way
/// Ladderback does. A password written in the config is never repeated. A
/// keyring password is read with `keyring`, and a notice says so when none
/// of the folders of `path` holds it.
fn password(
    secret: &SecretRef,
    keyring: KeyringCommand,
    path: &[PathBuf],
    notices: &mut Vec<ProfileNotice>,
) -> Result<(&'static str, String), ExportRefusal> {
    Ok(match secret {
        SecretRef::File { path, .. } => ("password-file", path.clone()),
        SecretRef::Command(command) => (
            "password-command",
            command_line(command.program(), command.args()),
        ),
        SecretRef::Env(name) => ("password-command", command_line("printenv", [name])),
        SecretRef::Keyring(entry) => {
            let program = keyring.program();
            if !path.iter().any(|folder| runnable(&folder.join(program))) {
                notices.push(ProfileNotice::KeyringCommandMissing {
                    program: program.to_owned(),
                });
            }
            ("password-command", keyring_command(entry, keyring))
        }
        SecretRef::Inline(_) => return Err(ExportRefusal::InlineSecret),
    })
}

/// Whether `file` is a file that can be run: what a shell, and rustic,
/// would find on the `PATH`.
fn runnable(file: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    file.metadata()
        .is_ok_and(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
}

/// The command that prints the secret of a keyring entry (§10.2).
fn keyring_command(entry: &KeyringEntry, keyring: KeyringCommand) -> String {
    match keyring {
        KeyringCommand::Security => command_line(
            "security",
            [
                "find-generic-password",
                "-s",
                entry.service(),
                "-a",
                entry.name(),
                "-w",
            ],
        ),
        KeyringCommand::SecretTool => command_line(
            "secret-tool",
            [
                "lookup",
                "service",
                entry.service(),
                "username",
                entry.name(),
            ],
        ),
    }
}

// --- Commands ----------------------------------------------------------------

/// A program and its arguments as one string that splits back into them, the
/// way a POSIX shell splits words (what rustic's `CommandInput` does).
fn command_line<S: AsRef<str>>(program: &str, args: impl IntoIterator<Item = S>) -> String {
    std::iter::once(word(program))
        .chain(args.into_iter().map(|arg| word(arg.as_ref())))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One word, quoted when a shell would read it as more or less than itself.
fn word(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./,:=+@%".contains(c));
    if safe {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// A hook as rustic reads it: the command line as a string when a failure is
/// an error, which is what a string means, else a table that says so.
fn hook(entry: &HookEntry) -> String {
    let command = &entry.command;
    if entry.on_failure == OnFailure::Error {
        return quoted(&command_line(command.program(), command.args()));
    }
    let args: Vec<_> = command.args().iter().map(|arg| quoted(arg)).collect();
    format!(
        "{{ command = {}, args = [{}], on-failure = {} }}",
        quoted(command.program()),
        args.join(", "),
        quoted(entry.on_failure.as_str()),
    )
}

fn hooks(out: &mut Out, header: &str, hooks: &Hooks) {
    if hooks.is_empty() {
        return;
    }
    out.section(header);
    for (name, entries) in [
        ("run-before", &hooks.run_before),
        ("run-after", &hooks.run_after),
        ("run-failed", &hooks.run_failed),
        ("run-finally", &hooks.run_finally),
    ] {
        if !entries.is_empty() {
            let items: Vec<_> = entries.iter().map(hook).collect();
            out.raw(name, &format!("[{}]", items.join(", ")));
        }
    }
}

// --- TOML ----------------------------------------------------------------------

#[derive(Default)]
struct Out {
    text: String,
}

impl Out {
    /// Starts the table `[header]`, after a blank line.
    fn section(&mut self, header: &str) {
        self.text.push_str(&format!("\n[{header}]\n"));
    }

    /// Starts an entry of the array of tables `[[header]]`.
    fn array_section(&mut self, header: &str) {
        self.text.push_str(&format!("\n[[{header}]]\n"));
    }

    /// `key = value`, the value being TOML already.
    fn raw(&mut self, key: &str, value: &str) {
        self.text.push_str(&format!("{key} = {value}\n"));
    }

    fn string(&mut self, key: &str, value: &str) {
        self.raw(key, &quoted(value));
    }

    fn list<'a>(&mut self, key: &str, values: impl IntoIterator<Item = &'a str>) {
        let items: Vec<_> = values.into_iter().map(quoted).collect();
        self.raw(key, &format!("[{}]", items.join(", ")));
    }

    fn list_if_any(&mut self, key: &str, values: &[String]) {
        if !values.is_empty() {
            self.list(key, values.iter().map(String::as_str));
        }
    }

    fn integer(&mut self, key: &str, count: KeepCount) {
        self.raw(key, &count.get().to_string());
    }

    fn within(&mut self, key: &str, span: &KeepWithin) {
        self.string(key, &span.to_string());
    }

    /// A switch rustic has off unless it is on: written only when on.
    fn flag(&mut self, key: &str, on: bool) {
        if on {
            self.raw(key, "true");
        }
    }
}

/// A key, bare when TOML allows it and quoted when not.
fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare { name.to_owned() } else { quoted(name) }
}

/// A TOML basic string. The characters TOML does not allow in one are
/// written as escapes.
fn quoted(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_ascii_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}
