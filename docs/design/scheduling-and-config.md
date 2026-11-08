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

Each agent's plans live in `~/.config/steading/config.toml` (Linux) or `~/Library/Application Support/steading/config.toml` (macOS). The file is plain TOML so it's easy to review and version. The UI edits it through the API and writes it atomically, keeping the last 10 versions. Runtime state (op log, queue, schedule state, snapshot cache) lives in SQLite. Config does not.

```toml
schema = 1                       # config schema version (see below)

[agent]
hostname = "ogc-mbp-2021"        # defaults to system hostname; must be unique on the hub
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
label = "ogc-mbp-2021 home"
# host = "..."                   # optional per-plan override of [agent] hostname
sources = ["/Users/nledford"]
tags = ["ogc-mbp-2021", "macbookpro", "macbookpro2021", "home"]   # steading-* tags are added automatically
schedule = { cron = "0 * * * *", jitter = "5m", catch-up = true, on-ac-power = false }
retry = { attempts = 3, backoff = "10m" }
git-ignore = true
exclude-if-present = [".nobackup", "CACHEDIR.TAG"]
iglobs = ["!.Trash", "!bin", "!node_modules", "!target", "!trash",
          "!/Users/nledford/Library", "!/Users/nledford/.colima/"]
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
[repositories.nas.prune]         # Steading's own struct, mapped onto rustic_core::PruneOptions
schedule = { cron = "0 3 * * 0" }
keep-delete = "6d"               # minimum 23h; instant-delete is not configurable (I2)
max-unused = "5%"

[repositories.nas.check]
schedule = { cron = "0 4 * * 0" }
read-data-subset = "5%"
```

Hook entries are either a command string or `{ command, args, on-failure = "error" | "warn" | "ignore" }`, as in rustic. The three hook levels follow rustic exactly (§16, decision 10): agent `[hooks]` and repository `hooks` wrap **every** job, including forget, prune and check; plan `hooks` wrap that plan's backup. Order: agent before, repository before, plan before, operation, then plan, repository, agent after/finally. On failure, `run-failed` then `run-finally`.

**Parsing.** The `config` adapter parses each TOML table into one flat Steading-owned struct with `rename_all = "kebab-case"` and `deny_unknown_fields`, using the same keys rustic profiles use. Because there is no `#[serde(flatten)]`, an unknown key is reported at its own file and line. The adapter then builds domain types (`RetentionPolicy`, `SourceFilter`, …), and the engine adapter maps those onto `rustic_core`'s `KeepOptions`, `Excludes`, `LocalSourceFilterOptions` and `SnapshotOptions` (§3.2, §16 decision 15).

A **parity test** per mapped `rustic_core` struct lists every upstream field as either mapped or deliberately unsupported, so a pin upgrade that adds a field fails until someone decides. The mechanism depends on the type:

- **serde-derived structs** (`KeepOptions`, `Excludes`, `LocalSourceFilterOptions`, `SnapshotOptions`): the test deserializes through a stub `Deserializer` that records the field list serde passes to `deserialize_struct`, and compares it with the mapped and unsupported lists.
- **Structs without serde** (`PruneOptions`, `CheckOptions`, `RestoreOptions`, `LsOptions`): the test builds each with an exhaustive struct literal, no `..`, so a new field is a compile error. If a struct is `#[non_exhaustive]`, a `cargo public-api` diff on pin bumps replaces the literal.

**Schema versioning.** `schema` is required. The versioned file formats, the migration chain and the parser live in `adapters/config` and are tested with golden files from every past schema version; `domain` keeps only the invariants (unique plan IDs, aliases that exist). Migrations run on load and write the old file to the version history first. The daemon refuses to start on a schema newer than it understands. Backrest needed four config migrations and still broke upgrades ([#389](https://github.com/garethgeorge/backrest/issues/389), [#421](https://github.com/garethgeorge/backrest/issues/421)); this is planned from M1b.

### 6.2 Importing rustic profiles

`steading import-profile <file.toml> [--repo-name nas]` reads a rustic profile and produces repos and plans. Profiles named by `use-profiles` are resolved the way rustic does: the platform config dir (`~/.config/rustic` on Linux, `~/Library/Application Support/rustic` on macOS), then `/etc/rustic`, then the current directory; a file's own values beat the profiles it uses. The same function is used by the UI's "Import…" dialog. Mapping for your current laptop profile:

| rustic profile | Steading |
|---|---|
| `[repository] repository / repo-hot / password* / key* / options / options-hot / options-cold` | `[repositories.<name>]` (secrets per §10.2) |
| `[backup] host`, per-snapshot `host` | Per-plan `host`. If every snapshot entry agrees, `[agent] hostname` instead |
| `[backup] git-ignore`, `exclude-if-present`, `globs`, `iglobs`, `tags`, … (section defaults) | Each plan gets the **effective** value: the snapshot entry's own value if set, otherwise the `[backup]` value. For list fields a non-empty snapshot list **replaces** the default list; lists are never concatenated. This matches rustic |
| `[[backup.snapshots]] name / sources / label / tags / iglobs / options` | One `[[plans]]` each. `id` comes from `name`, else the label slugified |
| `[backup] group-by` | Not imported: it only picks the backup's parent, and Steading picks parents by plan tag (§7.3). Notice shown |
| `[forget] keep-*` / `keep-within-*` | `[plans.retention]` on every plan from this profile |
| `[forget] prune = true` | Not copied into plans. It decides the repo's requested `maintenance` (§7.3). The importer warns: *"Pruning will run from one place (hub or this agent), not after every forget."* |
| `[global] group-by` / `[forget] group-by` | Repo `group-by`, used only to group untagged snapshots for adoption (§7.3) |
| `[global.hooks]` | Agent `[hooks]` |
| `[repository.hooks]` | Repository `hooks` |
| `[backup.hooks]`, `[backup.snapshots.hooks]` | Plan `hooks`. `run-before`: backup-level entries, then the snapshot entry's. `run-after`, `run-failed`, `run-finally`: the snapshot entry's, then backup-level, matching rustic's order. Each plan is its own job, so backup-level hooks run once per plan rather than once per `rustic backup` call; the importer says so |
| `[global.env]` | Not imported; notice shown. Use `password-env` or `${VAR}` (§10.2) |
| `[copy]`, `[webdav]`, `[mount]`, `[init]`, `[snapshot-filter]`, `prometheus*`, `opentelemetry`, `log-*` | Ignored, with a notice |

**Export.** `steading export-profile --plan home` writes one profile per plan that the stock `rustic` CLI accepts:

- Only keys rustic knows. rustic's config structs use `deny_unknown_fields`, so Steading-only keys (`schedule`, `retry`, `maintenance`, conditions) are dropped.
- Retention goes into `[forget]` with `group-by = ""`, scoped with `[snapshot-filter] filter-tags = ["steading-plan:home,steading-agent:<id>"]`. The empty `group-by` puts every snapshot the filter selects into one group, as Steading's tag grouping does; with rustic's default grouping, history from before a change of sources or label would be split off and kept. The two tags must be one comma-joined entry: separate list entries are OR-ed, which would also match other agents' `home` plans. Without a filter, `rustic forget` applies the policy to **every** group in the repo. `[forget] filter-*` keys are never used: rustic 0.11.4 accepts them but ignores them. (All three behaviours checked with rustic 0.11.4.)
- The plan's `steading-plan` and `steading-agent` tags are added to the exported `tags`, so CLI backups made after leaving Steading are still selected by the filter.
- `prune` is not set in `[forget]`.
- Secrets are written as `password-command` or `password-file`. A keyring source becomes a `password-command` for the platform's keyring CLI; an env source becomes `password-command = "printenv NAME"`. An `options` value containing `${…}` is left out of the profile, and the export prints the `RUSTIC_REPO_OPT_<KEY>` environment variable to set instead (rustic reads backend options from those). A `repository` or `repo-hot` containing `${…}` fails the export with an error naming the key.
- Hooks are placed back into `[global.hooks]`, `[repository.hooks]` and `[backup.snapshots.hooks]`. With one plan per profile, putting the plan's combined hooks at snapshot level runs them in the same order.

Golden test: on a scratch repo, `rustic -P <exported> forget --dry-run` removes exactly the snapshot IDs that Steading's own forget dry run removes.
