## 5. Scheduling

- **Schedule types:** cron expressions (`croner` syntax) or simple intervals ("every 1 h"), with optional **jitter** to spread load across machines.
- **Time zone.** Cron runs in the machine's local time by default; a schedule may set `timezone = "UTC"` or an IANA zone. A local time skipped by a DST change runs at the next valid time; a repeated hour runs once.
- **Catch-up (anacron-style).** `next_run` is stored in the DB. If the machine was asleep or off when a run was due, it runs once at startup or wake instead of being skipped. Several missed runs collapse into one. This matters for laptops.
- **Wake detection.** A ticker compares wall-clock time against the monotonic clock every 30 s. A gap of more than 2 min means the machine slept or the clock jumped, and all `next_run` values are recomputed. (Backrest's scheduler hung after sleep without this, [#172](https://github.com/garethgeorge/backrest/issues/172).)
- **Network wait.** A catch-up run after wake first waits, for up to 2 min, until the repository host resolves. Runs fired before the network is up otherwise fail ([Backrest #882](https://github.com/garethgeorge/backrest/issues/882)).
- **Conditions** (optional, per plan): only on AC power, not on a metered connection, and a minimum interval since the last success. These are checked by a small platform module (`/sys/class/power_supply` and NetworkManager on Linux, IOKit on macOS).
- **Persistent queue.** Queued jobs, including manual "run now" requests, are `operations` rows with status `queued`. They survive restarts and config reloads ([Backrest #1436](https://github.com/garethgeorge/backrest/issues/1436)).
- **Retry.** Per plan: `retry = { attempts, backoff }`. A failed backup is re-queued with exponential backoff from `backoff`, capped at 1 h. Retries stop at the next scheduled run. Retries are separate from hooks.
- **Concurrency.** Default 1 running backup per agent, configurable. Lock-free means correctness never depends on this; it only limits load.
- **Overlap policy.** If a plan's previous run is still active when the next is due: `skip` (default) or `queue`.
- **Maintenance jobs** (forget/prune/check) are scheduled only on the repo's **granted maintenance owner** and run one at a time per repo (§7.3).

---

## 6. Configuration

### 6.1 Source of truth

Each agent's plans live in `~/.config/ladderback/config.toml` (Linux) or `~/Library/Application Support/ladderback/config.toml` (macOS). The file is plain TOML so it's easy to review and version. The UI edits it through the API and writes it atomically, keeping the last 10 versions. Runtime state (op log, queue, schedule state, snapshot cache) lives in SQLite. Config does not.

```toml
schema = 1                       # config schema version (see below)

[agent]
hostname = "demo-laptop"         # defaults to system hostname; must be unique on the hub
concurrency = 1
nice = 10
allow-hub-restore = false        # §10.4
locale = "en-US"                 # notification text (§8.4); defaults to the POSIX locale, then en-US

[hub]                            # optional
url = "https://hub.example.lan:9899"

[hooks]                          # = rustic [global.hooks]: wraps every job on this agent
run-before = []

[repositories.nas]
repository = "sftp://backup@nas.lan/backups/rustic"   # passed to rustic_backend
password-command = "security find-generic-password -s rustic-nas -w"
maintenance = "hub"              # requested owner: "hub" | "local" | "none" (§7.3)
group-by = "host,label,paths"    # only used to group untagged snapshots (§7.3)
options = { }                    # backend options (BTreeMap<String,String>); also options-hot/-cold
hooks = { run-before = [], run-after = [], run-failed = [], run-finally = [] }  # = [repository.hooks]

[[plans]]
id = "home"
repository = "nas"
label = "demo-laptop home"
# host = "..."                   # optional per-plan override of [agent] hostname
sources = ["/home/demo"]
tags = ["demo-laptop", "laptop", "home"]   # ladderback-* tags are added automatically
schedule = { cron = "0 * * * *", jitter = "5m", catch-up = true, on-ac-power = false }
retry = { attempts = 3, backoff = "10m" }
git-ignore = true
exclude-if-present = [".nobackup", "CACHEDIR.TAG"]
iglobs = ["!.Trash", "!bin", "!node_modules", "!target", "!trash",
          "!/home/demo/Downloads", "!/home/demo/.cache/"]
hooks = { run-before = [], run-after = [], run-failed = [], run-finally = [] }

[plans.retention]                # = rustic_core::KeepOptions
keep-within-hourly = "1 day"
keep-within-daily = "1 week"
keep-within-weekly = "1 months"
keep-within-monthly = "1 year"
keep-within-quarter-yearly = "2 years"
keep-within-half-yearly = "3 years"
keep-within-yearly = "4 years"
```

The maintenance owner's config also holds the repo's maintenance settings. For a hub-managed repo that is the hub's own `config.toml`, together with the hub's secret for the repo (§10.2):

```toml
[repositories.nas.prune]         # Ladderback's own struct, mapped onto rustic_core::PruneOptions
schedule = { cron = "0 3 * * 0" }
keep-delete = "6d"               # minimum 23h; instant-delete is not configurable (I2)
max-unused = "5%"

[repositories.nas.check]
schedule = { cron = "0 4 * * 0" }
read-data-subset = "5%"
```

Hook entries are either a command string or `{ command, args, on-failure = "error" | "warn" | "ignore" }`, as in rustic. The three hook levels follow rustic exactly (§16, decision 10): agent `[hooks]` and repository `hooks` wrap **every** job, including forget, prune and check; plan `hooks` wrap that plan's backup. Order: agent before, repository before, plan before, operation, then plan, repository, agent after/finally. On failure, `run-failed` then `run-finally`.

**How hooks run** (settled in M1b). The `HookRunner` port runs each entry as a process, without a shell, in a process group of its own, with the worker's scrubbed environment (§4.1 allow-list) plus `LADDERBACK_HOOK`, `LADDERBACK_PLAN`, `LADDERBACK_REPOSITORY`, `LADDERBACK_OPERATION_ID`, `LADDERBACK_SNAPSHOT_ID` (after a successful backup) and `LADDERBACK_ERROR` (the stored failure code, for the failed and finally phases) — never a secret or the repository URL. Each hook may run 5 minutes (a constant until a later milestone makes it configurable); past it, the group gets SIGTERM, then SIGKILL after 5 s if the hook is still running, the way a password command is stopped (security.md §10.2). Each signal goes once and only while the hook is unreaped (a group is named by its leader's process id, which another process may hold once the leader has been reaped and the group has no live member), so a hook that ends on the SIGTERM is not signalled again, and a process it started that ignored the SIGTERM too is left running. While it is stopped, the hook's output pipes stay open and are read and dropped until it is reaped, so a SIGTERM handler can write to them however much it says, and what it says is not part of the captured output. The wait after the SIGKILL is bounded (5 s), so a hook no signal reaches still ends the call as timed out. A hook that ends by itself is reaped when it ends, though a process it started still holds its stdout or stderr open: what is in the pipes at the exit, and what they carry for 1 s after it, is captured, then the runner lets go of them (the second is not counted against the 5 minutes: a hook that ended in time is never timed out), and a process that writes there later gets SIGPIPE (EPIPE where it ignores that), so a hook that starts something long-lived redirects its output (decision 34). Stdout and stderr are captured, capped at 64 KiB per stream with a truncated flag, and redacted against the run's secrets; a stream the cap cut is masked as a cut text, so a secret the cap cut through leaves nothing of itself (security.md §10.2, decision 45). A failing before-hook ends the run as its `on-failure` says: `error` fails the operation before the engine starts (`hook-failed`, with the exit status, or none when it timed out or never started), `warn` attaches the `hook-warning` warning, `ignore` goes on. After, failed and finally hooks never change how a finished operation ended: `error` and `warn` attach the same warning, `ignore` is silent. When a before-hook stopped the run, only the finally-hooks run.

**Parsing.** The `config` adapter parses each TOML table into one flat Ladderback-owned struct with `rename_all = "kebab-case"` and `deny_unknown_fields`, using the same keys rustic profiles use. Because there is no `#[serde(flatten)]`, an unknown key is reported at its own file and line. The adapter then builds domain types (`RetentionPolicy`, `SourceFilter`, …), and the engine adapter maps those onto `rustic_core`'s `KeepOptions`, `Excludes`, `LocalSourceFilterOptions` and `SnapshotOptions` (§3.2, §16 decision 15).

A **parity test** per mapped `rustic_core` struct lists every upstream field as either mapped or deliberately unsupported, so a pin upgrade that adds a field fails until someone decides. The mechanism depends on the type:

- **serde-derived structs** (`KeepOptions`, `Excludes`, `LocalSourceFilterOptions`, `SnapshotOptions`): the test deserializes through a stub `Deserializer` that records the field list serde passes to `deserialize_struct`, and compares it with the mapped and unsupported lists.
- **Structs without serde** (`PruneOptions`, `CheckOptions`, `RestoreOptions`, `LsOptions`): the test builds each with an exhaustive struct literal, no `..`, so a new field is a compile error. If a struct is `#[non_exhaustive]`, a `cargo public-api` diff on pin bumps replaces the literal.

**Schema versioning.** `schema` is required. The versioned file formats, the migration chain and the parser live in `adapters/config` and are tested with golden files from every past schema version; `domain` keeps only the invariants (unique plan IDs, aliases that exist). Migrations run on load and write the old file to the version history first. The daemon refuses to start on a schema newer than it understands. Backrest needed four config migrations and still broke upgrades ([#389](https://github.com/garethgeorge/backrest/issues/389), [#421](https://github.com/garethgeorge/backrest/issues/421)); this is planned from M1b.

**Domain models and the `ConfigStore` port.** `domain::planning` holds the models. `AgentConfig` is the whole file: `AgentSettings`, the agent-level `Hooks`, a `BTreeMap` of `RepositoryConfig` by `RepositoryAlias`, and the `Plan`s in the order written. Its only constructor, `AgentConfig::new`, checks the rules that reach across parts (a plan ID is unique, a plan's repository exists, a plan has a source) and reports the first plan that breaks one; the default is the empty configuration. Each leaf is a type that cannot hold a wrong value (`Concurrency`, `Nice`, `Hostname`, `Source`, `Locale`, `CronExpression`, `Schedule`, `RetryPolicy`, `RetentionPolicy` with `KeepCount` and `KeepWithin`, `Hooks`, `Maintenance`, `GroupBy`), and a value out of range is an `InvalidValue` that names its kind. `app::ports::ConfigStore` has `load` and `save` over the whole `AgentConfig`. `ConfigError` says what failed (`Unavailable`, `TooLarge`, `SchemaMissing`, `SchemaNewer`, `Invalid`), in which file and, for `Invalid`, at which key and position. It never carries a secret. A store that was never saved to loads `AgentConfig::default()`. The five `config-*` error codes map one to one onto these variants.

Decisions the models make where this document was silent:

- **Names.** `RetentionPolicy` and `RetryPolicy`, as in architecture.md §3.1.
- **Maintenance defaults to `none`.** A repository that doesn't say is not asking anyone to run forget, prune or check on it; owning that work is something a user grants (§7.3).
- **Cron has five fields.** Seconds and a year are refused, and nicknames such as `@daily` are read. A cron string is capped at 512 bytes. This leaves the other two fields to be allowed later without breaking a valid file. An expression that never fires (`0 0 31 2 *`) is refused; `0 0 29 2 *` fires, once in four years.
- **Durations.** A `jitter` is not negative and at most 366 days. A `backoff` is at least 1 s and at most 1 h, the cap §5 puts on the doubling wait; **retry attempts** are 1 to 100, so that a plan with no schedule, which has no next run to stop its retries, still stops. A `keep-within` is not negative and has no cap: rustic adds it to a snapshot's time with a saturating add.
- **Limits.** `concurrency` is 1 to 64, `nice` 0 to 19. The cap on concurrency is Ladderback's, so that a number from a file can't size a pool of threads. A `keep-*` count is `-1` (all) or 0 up to `i32::MAX`, as in rustic.
- **Schedule defaults.** No jitter, `catch-up = true`, `on-ac-power = false` (any power source).
- **An empty `RetentionPolicy` keeps everything.** `RetentionPolicy::is_empty` is true when no setting is set, and a plan without a policy is never forgotten from.
- **A policy that keeps nothing is refused, per plan with history** (decided 2026-11-09, decision 25). A policy that sets only counts of `0` (`keep-last = 0` and nothing else), or only a zero-length `keep-within` (`"0s"`), is not empty, and keeps nothing: forget would remove every snapshot of the plan. The model accepts it because rustic does, and `0` is a meaningful count beside others. A forget refuses such a policy before anything is selected, with `RetentionKeepsNothing` naming the plan (HTTP 422) — but only for a plan whose group holds snapshots: a plan that has never backed up has nothing a policy could remove. Rustic's own `forget` refuses a policy that sets nothing, so forget must not be asked to run when `is_empty` is true; an empty policy here keeps everything.
- **`RepositoryConfig` carries its backend settings.** `RepositorySettings` (the repository string, `repo-hot`, where the password is, and the three option maps) moved from `app::secrets` into `domain::planning`, with its masked `Debug` and `masked_options`; `app::secrets` imports it. Fields and options keep `${VAR}` and `${file:/path}` references exactly as written, to be resolved when a job starts (§10.2).
- **`ConfigError::Invalid` is typed.** It is `{ file, key, at, problem, detail }`. `problem` is a `Problem` code, never a sentence (§8.4): `NotUtf8`, `Syntax`, `UnknownKey`, `MissingKey`, `WrongType`, `SchemaNotAVersion`, `SchemaTooOld { found, oldest }`, `Migration`, `Value(ValueKind)`, `Id(IdKind)`, `ReservedTag`, `Password(PasswordProblem)`, `Violation(Violation)` and `Unwritable`. `key` is the path of the setting (`plans[1].schedule.cron`) and `at` the line and column, counted from 1, of its value, or of the key for an unknown one; a migrated file has no `at`, because its text is not the user's. `detail` holds only what the parser said: `toml::de::Error::message()`, never its `Display`, which prints the source line, with every quoted text cut out, so `password = 123456789` reports `invalid type: integer, expected a string` and nothing of the number. It is empty when `problem` says it all, and a migration step's own words for `Migration`. Hook commands and passwords are never echoed, not even by `Value(HookCommand)`.
- **The password sources.** A repository has exactly one of `password`, `password-command`, `password-keyring` (`service/name`), `password-file` (an absolute path, read as rustic reads a password file: its first line) and `password-env`. None or several is `Password(Missing | Several)` at the repository; a relative file, a keyring entry without a `/` and a command that can't be split are refused at their own key. A `${file:…}` password, which reads the whole file, has no key of its own: saving one is `Unwritable`.
- **Load order.** The syntax; then `schema` alone, so that a file in a newer version is refused for that before anything else in it is read (`SchemaNewer`), a file with none is `SchemaMissing` even if the rest is wrong, and one older than the oldest the build migrates is `SchemaTooOld`; then the migration steps after the file's version, run on a `toml::Table`; then the strict tables, then the domain's types, then `AgentConfig::new`. The first unknown key reported is the first in the file, not the first by name. A file is read up to its cap (1 MiB) plus one byte, so a file that never ends is `TooLarge`; so is a save that would write more.
- **A table needs what it needs.** A plan has `id`, `repository` and `sources`; a repository has `repository`; a `schedule` has `cron`; a `retry` has both `attempts` and `backoff` (neither has a default); a table hook has `command`. A missing one is `MissingKey`, at the table that lacks it.
- **Layout of the written file.** `schema` first, then `[agent]`, `[hooks]`, a table for each repository (in alias order) and each plan (in file order) with its `[plans.retention]`. Whatever is at its default isn't written: no `maintenance = "none"`, no default `group-by`, no empty hooks, no `catch-up = true`. Small values (schedule, retry, options, hooks) are inline tables. A hook is its command line as `shell_words` joins it, unless its `on-failure` isn't `error`, and then a table. Comments are not kept when the file is saved. The text is the same for the same configuration. The layout is written by the adapter because `toml`'s serializer moves a list holding tables behind the other keys of its table.
- **History.** A load that migrates, and every save over an existing file, first keeps the file as it was (even one nobody could read) in `history/` next to it, named `<number>-<UTC YYYYMMDDTHHMMSS.mmm>Z.toml`: a six-digit number counting up from the highest in the folder, then the time of the replacing write, which is only for a person reading the names. The number is the order: the 10 highest are kept, so a clock set back (by a person, or by NTP) never makes a save drop the newest versions. A file in `history/` whose name isn't `<number>-<stamp>.toml` is not a version: it is not counted and never forgotten. Files are 0600 and folders 0700. If the old version can't be kept, nothing is replaced: the load or save fails with `Unavailable`. A save over a file longer than the cap is `TooLarge`, because it can't be kept. A version is flushed to disk, and so is the `history/` folder (and the config folder, in which it may be new), before the file is replaced; a failure to flush is a failure to keep: the version just written is removed again, nothing is replaced, and the load or save is `Unavailable`, so a file system that refuses to flush a folder can't save. A failure to remove an old version doesn't fail the save. The replacement is atomic (a temporary file in the same folder, flushed, then renamed), and one lock serialises every load and save of a store. **A symlinked `config.toml`** (a dotfile manager's) stays a link: a save follows the link, through any chain of them, to the file at its end, which may not exist yet, and replaces that file atomically in its own folder; a chain longer than 40 links, or a loop, is `Unavailable` and changes nothing. The new file is a new inode: its mode is 0600 whatever the target's was, and its owner, ACLs and hard links aren't carried over. When it is the folder that is the link (`~/.config/ladderback` leading into a repository), `config.toml` isn't a link, and `history/` is in the folder the link leads to. The history stays in the folder of the link, `history/` next to `config.toml`, not next to the target: the target's folder is typically a repository that gets committed and pushed, and the history may hold a password the config does (decision 29).
- **A migrated file is written as the current version.** `schema = <current>` and the canonical text, byte for byte what `render` gives for it, so a second load changes nothing. A file in the current version is never rewritten, so a hand-written file keeps its comments and layout until the agent saves.

**Built (C2).** The `config` adapter (`crates/adapters/config`, package `config-adapter`, as a registry crate `config` exists): `FileConfigStore` over the file with the contract suite of `ConfigStore`, the strict tables and the parity tests of the four `rustic_core` structs they map onto (and a test that the lists of mapped keys are the keys the tables take), the schema migrations on a `toml::Table` with golden files (`tests/golden/v1.toml`), the atomic write, the history and the size cap. `[hub]` is an unknown key until M4a.

**Not yet.** The adapter is not wired into `main.rs`, so "the daemon refuses to start on a newer schema" holds for the adapter, not yet for the daemon. The repositories' `[prune]` and `[check]` tables are not read (§7.3, item 7), and there are no `timezone`, `every`, overlap or metered keys (M2). Seeding the daemon-wide redactor from the config (security.md §10.2) waits for its consumer. The warning for an inline secret is built: the daemon logs it when it loads the config at start, and the repository list carries it (§16, decision 37). A golden file exists for version 1 only; the first `schema = 2` brings the second.

### 6.2 Importing rustic profiles

`ladderback import-profile <file.toml> [--repo-name nas]` reads a rustic profile and adds a repository and its plans to the configuration, in one save. Profiles named by `use-profiles` are resolved the way rustic does: the platform config dir (`~/.config/rustic` on Linux, `~/Library/Application Support/rustic` on macOS), then `/etc/rustic`, then the current directory; a file's own values beat the profiles it uses. The importer reads those files and nothing else: it never opens the repository, runs a hook or reads a password file. The UI's "Import…" dialog and the MCP `preview_import` tool (§4.6) will use the same port. Mapping for your current laptop profile:

| rustic profile | Ladderback |
|---|---|
| `[repository] repository / repo-hot / password* / key* / options / options-hot / options-cold` | `[repositories.<name>]` (secrets per §10.2). `<name>` is `--repo-name`, else the file name without its extension, as a slug |
| `[backup] host`, per-snapshot `host` | Per-plan `host`. If every snapshot entry agrees (and the configuration has no plans and the agent no hostname), `[agent] hostname` instead |
| `[backup] git-ignore`, `exclude-if-present`, `globs`, `iglobs`, `tags`, … (section defaults) | Each plan gets the **effective** value: the snapshot entry's own value if set, otherwise the `[backup]` value. For list fields a non-empty snapshot list **replaces** the default list; lists are never concatenated. This matches rustic |
| `[[backup.snapshots]] name / sources / label / tags / iglobs / options` | One `[[plans]]` each. `id` is a slug of `name`, else of the label, else of the last folder of the first source (below) |
| `[backup] group-by` | Not imported: it only picks the backup's parent, and Ladderback picks parents by plan tag (§7.3). Notice shown |
| `[forget] keep-*` / `keep-within-*` | `[plans.retention]` on every plan from this profile |
| `[forget] prune = true` | Not copied into plans. It decides the repo's requested `maintenance` (§7.3). The importer warns: *"Pruning will run from one place (hub or this agent), not after every forget."* |
| `[global] group-by` / `[forget] group-by` | Repo `group-by`, used only to group untagged snapshots for adoption (§7.3) |
| `[global.hooks]` | Agent `[hooks]` |
| `[repository.hooks]` | Repository `hooks` |
| `[backup.hooks]`, `[backup.snapshots.hooks]` | Plan `hooks`. `run-before`: backup-level entries, then the snapshot entry's. `run-after`, `run-failed`, `run-finally`: the snapshot entry's, then backup-level, matching rustic's order. Each plan is its own job, so backup-level hooks run once per plan rather than once per `rustic backup` call; the importer says so |
| `[global.env]` | Not imported; notice shown. Use `password-env` or `${VAR}` (§10.2) |
| `[copy]`, `[webdav]`, `[mount]`, `[init]`, `[snapshot-filter]`, `prometheus*`, `opentelemetry`, `log-*` | Ignored, with a notice. A `[snapshot-filter]` beside retention is refused instead (below) |

**What an import refuses.** A profile is input nobody vetted (§13.5), and what Ladderback can't carry over faithfully, or could only guess at, is refused rather than dropped, so nothing is imported and nothing is written:

- A key rustic doesn't know in the top level, `[global]`, `[backup]`, a snapshot entry, `[forget]`, `[snapshot-filter]` or `[global.hooks]`, which rustic 0.11.4 refuses too. The refusal names the key's path (`forget.keep-lots`) and its line. `[repository]` is the one table rustic reads leniently, and an unknown key there is a notice. A file that isn't TOML, isn't UTF-8, or can't be read is refused naming the file.
- Sizes and depth: a file over 1 MiB, more than 1000 snapshot entries, `use-profiles` nested deeper than 8 levels, more than 32 profiles in one import, a `use-profiles` list of more than 32 names (whether or not a file exists for them), and a profile that uses itself (a cycle is found by the canonical path). A name in `use-profiles` is a plain file name, never a path: `/`, `\`, `..` and a NUL byte are refused, so a profile can't reach a file outside the folders listed above.
- Amplification: the lists and hooks of `[backup]` are copied into every plan, so the plans of one import may hold 100 000 items in all (sources, globs, tags and hooks; a list of `[backup]` counts once per entry). Above that the profile is refused, at the key `backup`, before any plan is made. More than 200 different keys left out are refused too, at the 201st, so a `[repository]` that is read leniently can't be made to hold a list nobody reads.
- A value that is more than 64 KiB once its `${` are escaped (below): the configuration would read it as a template and refuse it on its next load, so the import refuses it at its key (`repository.repository`, `repository.repo-hot`, `repository.options.<name>`).
- A relative source or `-`, a `[backup]` that names `name` or `sources` (rustic refuses them at backup time), nested snapshot entries, `stdin-command`, a relative `password-file`, no repository, and no password source at all (Ladderback can't prompt for one).
- `[forget] keep-tags` and `keep-ids`: dropping them would let Ladderback's forget remove what rustic keeps.
- A non-empty `[snapshot-filter]` beside any retention, the profile's own or one it uses: rustic's forget then reaches only the snapshots the filter selects, and the same retention on a plan reaches all the plan's own, so it would remove what rustic keeps. The refusal names the first key of the filter (`snapshot-filter.filter-tags`) in the file that has it. Without retention the filter is only a notice. The one filter that is never refused is the one an export writes (a single `filter-tags` entry of a plan tag and an agent tag, below): it selects what the plan's forget reaches anyway, so an exported profile imports back.
- `[global] profile-substitute-env = true`: rustic would then read `${VAR}` in the profiles it uses as a variable, and the importer keeps that text as text (below). Turned off it is a key left out.
- Two plans that get one ID, or an ID the configuration already has, and a repository alias the configuration already has.

**Names and values.**

- A plan's ID is a slug: ASCII letters in lower case and digits, any other run a single `-`, trimmed, at most 64 bytes. It comes from the entry's `name`, else its `label`, else the last folder of its first source, else `plan`. A clash is refused, never numbered `-2`, because the person is the one who knows which plan is which (decision 27).
- The hostname rule: when every imported plan names one host, and the configuration has no plans and the agent no hostname, that host becomes `[agent] hostname` and the plans name none. When the agent already has that hostname, the plans that name it drop it. Any other host stays on its plan.
- `[global.hooks]` are appended after the agent's own hooks, so an import never replaces them.
- A tag written `a,b` is two tags, as rustic reads it. A tag of Ladderback's own (`ladderback-*`) is dropped with a notice, so that an exported profile imports back as the same plan.
- A password written in the profile (`password`, or a credential option such as `secret_access_key`) is kept as written, as the profile has it, with the warning `inline-secret` naming the field, never its value. `password-file` and `password-command` are kept as they are. `key`, `key-file` and `key-command` are noted, and refused when no other password source remains. The priority is rustic's: `key`, `key-file`, `key-command`, `password`, `password-file`, `password-command`.
- `RUSTIC_PASSWORD`, `RUSTIC_PASSWORD_FILE`, `RUSTIC_PASSWORD_COMMAND`, `RUSTIC_REPOSITORY` and `RUSTIC_REPO_OPT_*` (§10.2) only fill what the profile leaves out, and they are kept as references (`password-env = "RUSTIC_PASSWORD"`, `repository = "${RUSTIC_REPOSITORY}"`), never as their values. A profile's own setting wins. The command reads the process's environment in one place, and no other code does.
- Every `${` in `repository`, `repo-hot` and the option values a profile wrote becomes `$${`, so the configuration reads it as text. Without it a hostile profile could write `${file:/etc/shadow}` into a repository string and have the daemon read the file at the next job (§10.2). Only the references the importer makes itself, from the environment above, are left as `${…}`.
- The notices go to standard error, one warning each, and a summary of what was added to standard output. A refusal exits with 1. With `--json` there is one document on standard output and nothing on standard error: the notices are its `notices`, as codes and arguments, and a refusal is its `error` (§4.5).

**Export.** `ladderback export-profile --plan home` writes the profile of one plan that the stock `rustic` CLI accepts, on standard output (the notices go to standard error, and a refusal writes nothing and exits with 1):

- Only keys rustic knows. rustic's config structs use `deny_unknown_fields`, so Ladderback-only keys (`schedule`, `retry`, `maintenance`, conditions) are dropped.
- The profile has one `[[backup.snapshots]]` entry. Its `name` is the plan's ID, so the profile imports back as the same plan even when it has a label. Its `host` is the plan's, else the agent's hostname. The tables come in a fixed order, a table with nothing in it is left out, and the same plan gives the same text every time.
- Retention goes into `[forget]` with `group-by = ""`, scoped with `[snapshot-filter] filter-tags = ["ladderback-plan:home,ladderback-agent:<id>"]`. The empty `group-by` puts every snapshot the filter selects into one group, as Ladderback's tag grouping does; with rustic's default grouping, history from before a change of sources or label would be split off and kept. The two tags must be one comma-joined entry: separate list entries are OR-ed, which would also match other agents' `home` plans. Without a filter, `rustic forget` applies the policy to **every** group in the repo. `[forget] filter-*` keys are never used: rustic 0.11.4 accepts them but ignores them. (All three behaviours checked with rustic 0.11.4.)
- A plan with no retention keeps everything. Its profile is still written, with no `keep-*` key, and the export says that rustic's `forget` refuses to run on it until one is set (rustic 0.11.4: "specify at least one keep-* option"). That is safe, because nothing is removed, but it differs from Ladderback, which never forgets from such a plan.
- The plan's `ladderback-plan` and `ladderback-agent` tags are added to the exported `tags`, so CLI backups made after leaving Ladderback are still selected by the filter. A plan tag with a comma in it fails the export, naming the tag: rustic would read it as two tags, and the filter and the person's own tags would no longer mean what they did.
- `prune` is not set in `[forget]`: a forget that prunes would run outside Ladderback's own maintenance (I1).
- Secrets are written as `password-command` or `password-file`, never as text (I4). A keyring source becomes a `password-command` for the platform's keyring CLI (`security find-generic-password -s <service> -a <name> -w` on macOS, `secret-tool lookup service <service> username <name>` elsewhere); an env source becomes `password-command = "printenv NAME"`. A password written in the configuration fails the export with `inline-secret`.
- An `options` value containing `${…}`, and an option whose name holds a credential (`is_secret_option`, whatever its value), is left out of the profile, and the export prints the environment variable to set instead: `RUSTIC_REPO_OPT_<KEY>` for a name without `_`, `OPENDAL_<KEY>` for a snake_case name, with the `HOT` and `COLD` forms for `options-hot` and `options-cold`. An option whose name no variable could carry (upper case, spaces, both `-` and `_`) fails the export, naming the key. An option whose value is an address with a user or a password in it is left out the same way, whether it has a scheme or not: `endpoint = "https://user:secret@host"`, a proxy written `user:secret@host:3128`, and a token used as the user name (`token@host`) are all credentials. (A value with an `@` in a path or an e-mail address is left out too: that is the safe side of the guess.)
- A `repository` or `repo-hot` containing `${…}` fails the export with an error naming the key, and so does one that holds a password in its URL (`rest:https://user:secret@host/path`): the profile would have to repeat the password as text. The error names the key and never the value. A user name without a password is written: it is part of the address, and where it is a token, the config holds it as text already. An `rclone:` connection string (`rclone::s3,provider=AWS,secret_access_key=…:bucket`) is refused the same way when one of its `key=value` pieces, split at `,` and `:`, has a key `is_secret_option` names; the value is never shown. A credential under another name in a connection string (rclone's obscured `pass`, for one) is not found: keep it in a config file rclone reads.
- Hooks are placed back into `[global.hooks]`, `[repository.hooks]` and `[backup.snapshots.hooks]`. With one plan per profile, putting the plan's combined hooks at snapshot level runs them in the same order. A hook is its command line as a shell would split it, as a string when its `on-failure` is `error` and as a table when it isn't.

**Tests.** `features/interop/import.feature` and `export.feature` hold the scenarios; the use cases run against fakes, and the `@e2e` scenarios run the real command on real files, the reader and the writer each with a contract suite of their own port (`contract::profiles`). Parity tests pin the key lists of the reader to the structs of the pinned `rustic_core` (§6.1). Two golden checks use the stock `rustic` and skip, with `testkit::skip!`, where it isn't installed: `rustic -P <exported> show-config` reads a profile that sets every key the writer knows, and on a scratch repo `rustic -P <exported> forget --dry-run --json` removes exactly the snapshot IDs that Ladderback's own forget removes.
