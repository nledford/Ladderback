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

**How hooks run** (settled in M1b). The `HookRunner` port runs each entry as a process, without a shell, in a process group of its own, with the worker's scrubbed environment (§4.1 allow-list) plus `LADDERBACK_HOOK`, `LADDERBACK_PLAN`, `LADDERBACK_REPOSITORY`, `LADDERBACK_OPERATION_ID`, `LADDERBACK_SNAPSHOT_ID` (after a successful backup) and `LADDERBACK_ERROR` (the stored failure code, for the failed and finally phases) — never a secret or the repository URL. Each hook may run 5 minutes (a constant until a later milestone makes it configurable); past it, the group gets SIGTERM, then SIGKILL after 5 s. Stdout and stderr are captured, capped at 64 KiB per stream with a truncated flag, and redacted against the run's secrets. A failing before-hook ends the run as its `on-failure` says: `error` fails the operation before the engine starts (`hook-failed`, with the exit status, or none when it timed out or never started), `warn` attaches the `hook-warning` warning, `ignore` goes on. After, failed and finally hooks never change how a finished operation ended: `error` and `warn` attach the same warning, `ignore` is silent. When a before-hook stopped the run, only the finally-hooks run.

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
- **History.** A load that migrates, and every save over an existing file, first keeps the file as it was (even one nobody could read) in `history/` next to it, named `<UTC YYYYMMDDTHHMMSS.mmm>Z-<nn>.toml` after the time of the replacing write, `nn` counting saves in one millisecond, so names sort by time; the 10 newest are kept. Files are 0600 and folders 0700. If the old version can't be kept, nothing is replaced: the load or save fails with `Unavailable`. A save over a file longer than the cap is `TooLarge`, because it can't be kept. A failure to remove an old version doesn't fail the save. The replacement is atomic (a temporary file in the same folder, flushed, then renamed), and one lock serialises every load and save of a store. **A symlinked `config.toml`** (a dotfile manager's) stays a link: a save follows the link, through any chain of them, to the file at its end, which may not exist yet, and replaces that file atomically in its own folder; a chain longer than 40 links, or a loop, is `Unavailable` and changes nothing. The history stays in the folder of the link, `history/` next to `config.toml`, not next to the target: the target's folder is typically a repository that gets committed and pushed, and the history may hold a password the config does (decision 29).
- **A migrated file is written as the current version.** `schema = <current>` and the canonical text, byte for byte what `render` gives for it, so a second load changes nothing. A file in the current version is never rewritten, so a hand-written file keeps its comments and layout until the agent saves.

**Built (C2).** The `config` adapter (`crates/adapters/config`, package `config-adapter`, as a registry crate `config` exists): `FileConfigStore` over the file with the contract suite of `ConfigStore`, the strict tables and the parity tests of the four `rustic_core` structs they map onto (and a test that the lists of mapped keys are the keys the tables take), the schema migrations on a `toml::Table` with golden files (`tests/golden/v1.toml`), the atomic write, the history and the size cap. `[hub]` is an unknown key until M4a.

**Not yet.** The adapter is not wired into `main.rs`, so "the daemon refuses to start on a newer schema" holds for the adapter, not yet for the daemon. The repositories' `[prune]` and `[check]` tables are not read (§7.3, item 7), and there are no `timezone`, `every`, overlap or metered keys (M2). The warning for an inline secret and seeding the daemon-wide redactor from the config (security.md §10.2) wait for their consumer. A golden file exists for version 1 only; the first `schema = 2` brings the second.

Known limits of the writer, to settle with the wiring: the history's order is its file names', so a clock set back makes the next save drop the newest versions first; the history folder is not flushed after a version is kept in it.

### 6.2 Importing rustic profiles

`ladderback import-profile <file.toml> [--repo-name nas]` reads a rustic profile and produces repos and plans. Profiles named by `use-profiles` are resolved the way rustic does: the platform config dir (`~/.config/rustic` on Linux, `~/Library/Application Support/rustic` on macOS), then `/etc/rustic`, then the current directory; a file's own values beat the profiles it uses. The same function is used by the UI's "Import…" dialog. Mapping for your current laptop profile:

| rustic profile | Ladderback |
|---|---|
| `[repository] repository / repo-hot / password* / key* / options / options-hot / options-cold` | `[repositories.<name>]` (secrets per §10.2) |
| `[backup] host`, per-snapshot `host` | Per-plan `host`. If every snapshot entry agrees, `[agent] hostname` instead |
| `[backup] git-ignore`, `exclude-if-present`, `globs`, `iglobs`, `tags`, … (section defaults) | Each plan gets the **effective** value: the snapshot entry's own value if set, otherwise the `[backup]` value. For list fields a non-empty snapshot list **replaces** the default list; lists are never concatenated. This matches rustic |
| `[[backup.snapshots]] name / sources / label / tags / iglobs / options` | One `[[plans]]` each. `id` comes from `name`, else the label slugified |
| `[backup] group-by` | Not imported: it only picks the backup's parent, and Ladderback picks parents by plan tag (§7.3). Notice shown |
| `[forget] keep-*` / `keep-within-*` | `[plans.retention]` on every plan from this profile |
| `[forget] prune = true` | Not copied into plans. It decides the repo's requested `maintenance` (§7.3). The importer warns: *"Pruning will run from one place (hub or this agent), not after every forget."* |
| `[global] group-by` / `[forget] group-by` | Repo `group-by`, used only to group untagged snapshots for adoption (§7.3) |
| `[global.hooks]` | Agent `[hooks]` |
| `[repository.hooks]` | Repository `hooks` |
| `[backup.hooks]`, `[backup.snapshots.hooks]` | Plan `hooks`. `run-before`: backup-level entries, then the snapshot entry's. `run-after`, `run-failed`, `run-finally`: the snapshot entry's, then backup-level, matching rustic's order. Each plan is its own job, so backup-level hooks run once per plan rather than once per `rustic backup` call; the importer says so |
| `[global.env]` | Not imported; notice shown. Use `password-env` or `${VAR}` (§10.2) |
| `[copy]`, `[webdav]`, `[mount]`, `[init]`, `[snapshot-filter]`, `prometheus*`, `opentelemetry`, `log-*` | Ignored, with a notice |

**Export.** `ladderback export-profile --plan home` writes one profile per plan that the stock `rustic` CLI accepts:

- Only keys rustic knows. rustic's config structs use `deny_unknown_fields`, so Ladderback-only keys (`schedule`, `retry`, `maintenance`, conditions) are dropped.
- Retention goes into `[forget]` with `group-by = ""`, scoped with `[snapshot-filter] filter-tags = ["ladderback-plan:home,ladderback-agent:<id>"]`. The empty `group-by` puts every snapshot the filter selects into one group, as Ladderback's tag grouping does; with rustic's default grouping, history from before a change of sources or label would be split off and kept. The two tags must be one comma-joined entry: separate list entries are OR-ed, which would also match other agents' `home` plans. Without a filter, `rustic forget` applies the policy to **every** group in the repo. `[forget] filter-*` keys are never used: rustic 0.11.4 accepts them but ignores them. (All three behaviours checked with rustic 0.11.4.)
- The plan's `ladderback-plan` and `ladderback-agent` tags are added to the exported `tags`, so CLI backups made after leaving Ladderback are still selected by the filter.
- `prune` is not set in `[forget]`.
- Secrets are written as `password-command` or `password-file`. A keyring source becomes a `password-command` for the platform's keyring CLI; an env source becomes `password-command = "printenv NAME"`. An `options` value containing `${…}` is left out of the profile, and the export prints the `RUSTIC_REPO_OPT_<KEY>` environment variable to set instead (rustic reads backend options from those). A `repository` or `repo-hot` containing `${…}` fails the export with an error naming the key.
- Hooks are placed back into `[global.hooks]`, `[repository.hooks]` and `[backup.snapshots.hooks]`. With one plan per profile, putting the plan's combined hooks at snapshot level runs them in the same order.

Golden test: on a scratch repo, `rustic -P <exported> forget --dry-run` removes exactly the snapshot IDs that Ladderback's own forget dry run removes.
