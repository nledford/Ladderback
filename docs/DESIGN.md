# Steading — Design

> **Status:** Draft v0.1 · 2026-11-08
> **Name:** Steading (binary `steading`). A steading is a farmhouse with its outbuildings: the hub and its agents.

A self-hosted backup manager for [rustic](https://rustic.cli.rs). It provides a web UI for connecting repositories, defining backup plans and schedules, and watching backup status and errors across several computers. Think "Backrest, but built on `rustic_core`". It is written from scratch in Rust, and the frontend is written in Leptos.

---

## 1. Goals and non-goals

### Goals

1. **Plans and schedules.** Define what to back up, from which machine, into which repository, how often, and how long to keep it.
2. **One place to see every machine.** Show status, live progress, history, warnings and errors for every connected computer.
3. **Lock-free by construction.** Use rustic's lock-free design. Backups on many hosts and a prune can overlap safely. Exactly one place runs `forget`/`prune`/`check` for each repository.
4. **Compatible with rustic profiles.** Import existing rustic TOML profiles, and export any plan back to a profile, so users can always drop back to the `rustic` CLI. No lock-in.
5. **Single static binary.** The same program runs as daemon, hub, worker and CLI, with the web UI embedded. It runs headless on servers and as a user service on desktops and laptops.
6. **Linux and macOS first.** That covers Bazzite desktops, Linux servers and a MacBook. Windows comes later.

### Non-goals (for v1)

- Not a general rustic GUI covering every command (`copy`, `merge`, `rewrite`, `repair`, `webdav`, `mount`). Those stay CLI-only at first.
- No support for restic repositories with restic doing maintenance. See §7.3 for why restic must never prune a repository this tool manages.
- No multi-tenant SaaS. This is one owner and their machines.
- No mobile app. A responsive web UI is enough.

---

## 2. Background: what we build on

### 2.1 `rustic_core` (library, v0.13.0, Apache-2.0 OR MIT)

Checked against `rustic-rs/rustic_core@main` (2026-11-05):

| Need | API |
|---|---|
| Open repo | `Repository::new_with_progress(&RepositoryOptions, &RepositoryBackends, pb)?.open(&Credentials)?` |
| Backends | `rustic_backend::BackendOptions { repository, repo_hot, options, .. }.to_backends()` (local, REST, rclone, OpenDAL: S3, B2, SFTP, …) |
| Backup | `repo.to_indexed_ids()?.backup(&BackupOptions, &PathList, SnapshotFile)` |
| Snapshots | `get_all_snapshots()`, `get_snapshot_group(..)`, `delete_snapshots(&[SnapshotId])` |
| Retention | `KeepOptions` (all `keep-*` and `keep-within-*`, incl. quarter/half-yearly) and `ForgetGroups::from_grouped_snapshots_with_retention(..)` |
| Prune | `prune_plan(&PruneOptions)` then `prune(&opts, plan)` |
| Check | `check(CheckOptions) -> CheckResults` |
| Browse / restore | `node_from_snapshot_path`, `ls`, `prepare_restore` and `restore`, `dump(&Node, &mut impl Write)` |
| Stats | `infos_files()`, `infos_index()` |
| Progress | `trait ProgressBars { fn progress(type, prefix) -> Progress }`. A progress object has `set_length`/`inc`/`set_title`/`finish`/`error(item, during, message)` |

Facts that shape the design:

- **The API is synchronous and uses rayon internally.** Nothing in it is `async`.
- **The OpenDAL backend creates its own Tokio runtime**, so calling it from inside a Tokio task risks the "runtime within runtime" panic.
- **There is no cancellation API.** A running backup or prune cannot be stopped cooperatively.
- **Option structs are `serde`-deserializable with `kebab-case`**: `KeepOptions`, `Excludes` (`globs`, `iglobs`, …), `LocalSourceFilterOptions` (`git-ignore`, `exclude-if-present`, …), `SnapshotOptions`, `ParentOptions`, `PruneOptions`, `CheckOptions`. They match the TOML keys rustic profiles use. Several use `deny_unknown_fields`, which doesn't combine with `#[serde(flatten)]`. See §6.
- **The default snapshot grouping is `host,label,paths`**. Forget policies apply per group.
- **`rustic_config` is currently empty.** The profile structs (`[repository]`, `[backup]`, `[[backup.snapshots]]`, `[forget]`) live in the `rustic` CLI crate (`rustic-rs`, Apache-2.0 OR MIT), which is not published as a stable library. We mirror that layout ourselves, reusing the core option types.

### 2.2 Why lock-free matters

`rustic prune` does not delete unreferenced pack files right away. It moves them to a `packs_to_delete` list in the index and only deletes them on a **later** prune, once they've been marked for at least `keep-delete` (default **23 h**). If a concurrent backup reused one of those packs, the next prune recovers it.

So backups and prunes can overlap, **provided no backup runs longer than `keep-delete`**. The tool enforces this (see §7.3). One rule is mandatory: never `--instant-delete`.

### 2.3 Leptos

- Stable **0.8.23**. **0.9.0-rc1** is on `main`. Target 0.8, and move to 0.9 once it is stable.
- We use **client-side rendering (CSR)**, built with `trunk` to WASM. The build output is embedded in the daemon binary (`rust-embed`) and served by axum. Rationale in §8.1.

---

## 3. Architecture overview

```
                       ┌──────────────────────── hub machine ─────────────────────────┐
  Browser ──HTTPS──►   │  steading serve --hub                                         │
  (Leptos WASM UI)     │   ├─ HTTP API + UI (axum)      ├─ Hub registry (agents, plans)│
                       │   ├─ Scheduler                 ├─ Maintenance runner          │
                       │   ├─ Op log (SQLite)           │   (forget/prune/check)       │
                       │   └─ local agent ──spawns──►  steading worker ──► repo        │
                       └──────────────▲────────────────────────────────────────────────┘
                                      │ WebSocket (agent dials out, TLS, signed identity)
             ┌────────────────────────┼─────────────────────────┐
     ┌───────┴────────┐       ┌───────┴────────┐        ┌───────┴────────┐
     │ laptop (agent) │       │ server1 (agent)│        │ server2 (agent)│
     │ scheduler      │       │ scheduler      │        │ scheduler      │
     │ op log         │       │ op log         │        │ op log         │
     │ worker ─► repo │       │ worker ─► repo │        │ worker ─► repo │
     └────────────────┘       └────────────────┘        └────────────────┘
```

**One binary, three roles:**

| Role | Command | Runs where | Responsibility |
|---|---|---|---|
| **Agent** | `steading serve` | every machine | Local config, scheduler, op log, spawning workers, local web UI. Works fully standalone. |
| **Hub** | `steading serve --hub` | one machine (often a server) | Everything an agent does, plus an agent registry, aggregated history, maintenance for shared repos, notifications |
| **Worker** | `steading worker` (internal) | spawned per operation | Runs exactly one `rustic_core` operation, streams events to stdout, then exits |

Every agent is useful alone. Hub mode is opt-in, and an agent that can't reach the hub keeps backing up on schedule. It queues events and replays them when it reconnects.

---

## 4. Components

### 4.1 Worker process (the engine boundary)

Each operation (backup, forget, prune, check, restore, ls, dump, stats) runs in a **child process**:

```
daemon ──stdin: JobSpec (JSON)──► steading worker
daemon ◄─stdout: NDJSON Event stream── (progress, log, warning, result)
daemon ──SIGTERM → (grace) → SIGKILL──► cancel
```

Why a subprocess instead of `spawn_blocking`:

1. **Cancellation.** `rustic_core` has none, but a process can be killed. Killing is safe: an interrupted backup leaves only unreferenced packs and no snapshot, and the next prune cleans them up. Prune writes new index files before removing old ones, so a kill should leave extra data, not missing data, and the next prune finishes the job. M1 must prove this with a kill-at-random-points test against a scratch repo.
2. **No runtime nesting.** OpenDAL's internal Tokio runtime never meets the daemon's runtime.
3. **Isolation.** Memory from large indexes is returned to the OS when the worker exits, and a panic or out-of-memory kill in the engine can't take down the daemon.
4. **Privilege separation (later).** On servers, the worker can run as root to read `/etc` while the daemon and web UI run unprivileged. This uses a small setuid-free helper started via a systemd unit, or `sudo` with a fixed command.

**Event protocol** (crate `api::worker`):

```rust
enum WorkerEvent {
    Started { op_id: Uuid, pid: u32 },
    ProgressStart { key: u32, kind: ProgressKind /* Spinner|Counter|Bytes */, prefix: String },
    ProgressLength { key: u32, len: u64 },
    ProgressInc { key: u32, inc: u64 },    // coalesced: at most ~4 msgs/s per key
    ProgressTitle { key: u32, title: String },
    ProgressFinish { key: u32 },
    Warning { item: Option<String>, during: String, message: String }, // from Progress::error
    Log { level: Level, message: String },
    Result(OpResult), // BackupSummary | ForgetSummary | PruneStats | CheckResults | …
    Failed { error: String, kind: ErrorKind },
}
```

`ProgressBars` is implemented by `EventProgressBars`, which writes these events. Per-file errors raised through `Progress::error` become **warnings**, so an operation can end as `Success`, `SuccessWithWarnings` or `Failed`.

### 4.2 Daemon (agent)

- **axum 0.8** HTTP server. It serves the embedded UI, `/api/v1/*` (JSON) and `/api/v1/events` (WebSocket for live updates). It binds to `127.0.0.1:9898` by default.
- **Scheduler** (§5). It turns plans into jobs and respects per-agent concurrency limits.
- **Job runner.** It spawns workers, consumes events, updates the op log and broadcasts to UI clients. If a hub is connected, it forwards to the hub too.
- **Store.** SQLite via `sqlx`, in WAL mode (§9).
- **Config.** `config.toml` in the platform config dir is the source of truth (§6.1).
- **Secrets** (§10.2).
- **Hub client** (§7). It keeps an outbound WebSocket open, with reconnect, backoff and an event replay queue.

### 4.3 Hub

Everything an agent has, plus:

- **Agent registry**: identity public key, hostname, version, last seen, online status.
- **Aggregated op log.** It receives operation records and summaries from agents. Raw logs stay on the agent and are fetched on demand.
- **Plan registry.** Agents publish their plans (minus secrets), so the hub knows the retention policy for each `(repo, host, label, paths)` group.
- **Maintenance runner** for **hub-managed repos** (§7.3): forget, prune and check.
- **Notifications** (§11).
- **Remote control** (milestone 4b): "run now", cancel, and plan edits pushed to the agent.

### 4.4 Web UI (Leptos CSR)

Pages:

1. **Dashboard.** A card per machine: online status, last backup per plan (time, result, size added), next run and any running operation with live progress. A list of recent failures and warnings.
2. **Machine detail.** Plans, history timeline, logs and an agent settings summary.
3. **Plans.** Create and edit sources, excludes (globs, iglobs, git-ignore, exclude-if-present), tags and label, schedule, retention, hooks and repository. Also "Import rustic profile…" and "Export as rustic profile".
4. **Repositories.** Add or connect (local, SFTP, REST, S3, B2, rclone…), test the connection, init, stats (`infos_files`/`infos_index`), and set the maintenance owner and schedule.
5. **Snapshots.** Filter by host, label, tag or path. Snapshot detail shows the summary and a tree browser (`ls`). Restore to a path on the originating agent, or download a file or directory (`dump` streamed as tar/zip).
6. **Operations.** A searchable history with full logs, warnings and a cancel button.
7. **Settings.** Auth, hub pairing, notifications.

Live updates come from one WebSocket per tab carrying `UiEvent` (operation started/progress/finished, agent online/offline). The page fetches initial state over REST.

---

## 5. Scheduling

- **Schedule types:** cron expressions (`croner` syntax) or simple intervals ("every 1 h"), with optional **jitter** to spread load across machines.
- **Catch-up (anacron-style).** `next_run` is stored in the DB. If the machine was asleep or off when a run was due, it runs once at startup or wake instead of being skipped. Several missed runs collapse into one. This matters for laptops.
- **Conditions** (optional, per plan): only on AC power, not on a metered connection, and a minimum interval since the last success. These are checked by a small platform module (`/sys/class/power_supply` and NetworkManager on Linux, IOKit on macOS).
- **Concurrency.** Default 1 running backup per agent, configurable. Lock-free means correctness never depends on this; it only limits load. Different repos can run in parallel if allowed.
- **Overlap policy.** If a plan's previous run is still active when the next is due: `skip` (default) or `queue`.
- **Maintenance jobs** (forget/prune/check) are scheduled only on the repo's **maintenance owner** (§7.3).

---

## 6. Configuration

### 6.1 Source of truth

Each agent's plans live in `~/.config/steading/config.toml` (Linux) or `~/Library/Application Support/steading/config.toml` (macOS). The file is plain TOML so it's easy to review and version. The UI edits it through the API and writes it atomically, keeping a `.bak`. Runtime state (op log, schedule state, snapshot cache) lives in SQLite. Config does not.

```toml
[agent]
hostname = "ogc-mbp-2021"        # defaults to system hostname
concurrency = 1

[hub]                            # optional
url = "https://hub.example.lan:9899"

[repositories.nas]
repository = "sftp://backup@nas.lan/backups/rustic"   # passed to rustic_backend
password-command = "security find-generic-password -s rustic-nas -w"
maintenance = "hub"              # "hub" | "local" | "none"
options = { }                    # backend options (BTreeMap<String,String>)

[[plans]]
id = "home"
repository = "nas"
label = "ogc-mbp-2021 home"
sources = ["/Users/nledford"]
tags = ["ogc-mbp-2021", "macbookpro", "macbookpro2021", "home"]
schedule = { cron = "0 * * * *", jitter = "5m", catch-up = true, on-ac-power = false }
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

Plan option groups deserialize **directly into `rustic_core` types**: `Excludes`, `LocalSourceFilterOptions`, `KeepOptions` and `SnapshotOptions`. Because several of these use `deny_unknown_fields`, we **don't** `#[serde(flatten)]` them together. Instead:

1. Parse the plan table as `toml::Table`.
2. Partition keys by a static list of the fields each core type owns. The list is derived from the types, and a unit test fails when `rustic_core` adds a field.
3. Deserialize each partition into its type, and report unknown keys with file and line.

### 6.2 Importing rustic profiles

`steading import-profile <file.toml> [--repo-name nas]` reads a rustic profile, including `use-profiles` chains from the rustic config dir, and produces repos and plans. The same function is used by the UI's "Import…" dialog. Mapping for your current laptop profile:

| rustic profile | Steading |
|---|---|
| `[repository] repository / repo-hot / password* / options*` | `[repositories.<name>]` (secrets per §10.2) |
| `[backup] host = "ogc-mbp-2021"` | `[agent] hostname` (or a per-plan `host` override) |
| `[backup] git-ignore`, `exclude-if-present`, `globs`, `iglobs`, … (global defaults) | Copied into **each** imported plan. Section-level defaults merge into each `[[backup.snapshots]]` entry, as rustic does |
| `[[backup.snapshots]] sources / label / tags / iglobs` | One `[[plans]]` each. `id` comes from the label, slugified |
| `[forget] keep-*` / `keep-within-*` | `[plans.retention]` on every plan from this profile |
| `[forget] prune = true` | Not copied into plans. It decides the repo's `maintenance` setting (§7.3). The importer warns: *"Pruning will run from one place (hub or this agent), not after every forget."* |
| `[global] group-by` / `[forget] group-by` | Repo-level `group-by` (default `host,label,paths`) |
| `[global] hooks`, `[repository] hooks`, `[backup] hooks` | Plan `hooks` (same semantics: before/after/failed/finally), merged in that order |
| `[copy]`, `[webdav]`, `[mount]` | Ignored, with a notice |

`steading export-profile --plan home` does the reverse. It writes a profile that the stock `rustic` CLI can run unchanged, and that is the guarantee against lock-in.

---

## 7. Multiple computers

### 7.1 Pairing and identity

1. On the hub: **Settings → Agents → New pairing token**. The token is single-use, with a TTL of 15 min by default.
2. On the agent: `steading pair https://hub:9899 <token>` (or the same in the agent's UI).
3. The agent generates an **Ed25519 keypair** once and keeps it in its data dir. It sends its public key and the token. The hub records the agent and discards the token.
4. Every later connection is a WebSocket over TLS. The hub sends a nonce, the agent signs it, and the hub checks the signature against the registered key. To revoke an agent, delete it on the hub.

Hub TLS can be either of these:
- **Built-in.** A self-signed cert whose SHA-256 fingerprint is pinned during pairing (shown in the hub UI and included in the token string).
- **External.** A reverse proxy such as Caddy or Traefik, using normal CA validation. Only `/sync/v1` needs to be reachable by agents. Keep `/` and `/api` on a trusted network.

### 7.2 Sync protocol (`/sync/v1`, WebSocket, JSON, versioned)

Agent → hub:
- `Hello { agent_version, hostname, protocol: 1 }`
- `PlansSnapshot { plans: [PlanPublic] }`, sent on connect and on change. Secrets are never included.
- `OpEvent { op_id, seq, event }`. Every event has a monotonically increasing `seq`, so the agent can replay from the hub's last acknowledged `seq` after a disconnect.
- `Heartbeat { running_ops, next_runs }`

Hub → agent:
- `Ack { seq }`
- `RunPlan { plan_id }`, `CancelOp { op_id }` (milestone 4b)
- `FetchLog { op_id }` → agent replies `LogChunk`
- `UpdatePlan { plan }` (milestone 4b, opt-in per agent: "allow hub to edit my plans")

The browser never talks to agents directly. When looking at a remote machine, the hub UI calls hub APIs, and the hub forwards requests like "run now" or "fetch log" over the agent's socket.

### 7.3 Repository maintenance: exactly one owner

Each repository has a `maintenance` owner:

- `hub` (default when a hub is configured and several agents use the repo): the hub runs forget, prune and check.
- `local`: one specific agent runs them. This is for repos used by a single machine.
- `none`: nothing in Steading prunes. Use this if an existing cron job keeps doing it during migration.

**Forget algorithm (hub):**

1. Load all snapshots and group them by the repo's `group-by` (default `host,label,paths`).
2. For each group, find the published plan whose `(host, label, sources)` matches, and apply that plan's `KeepOptions`.
3. **Groups with no matching plan are never touched by default.** Examples are snapshots from an old cron job or a retired machine. The UI lists them as "unmanaged groups", and the user can attach a retention policy or delete them explicitly.
4. Run a dry run first and log what would be removed. Optionally require manual approval when more than N% of a group's snapshots would be deleted, as a safety net against a bad policy.

**Prune** runs after forget, on its own schedule (default weekly), with `keep-delete` ≥ 23 h. `--instant-delete` is never offered.

**Long-backup guard.** The hub knows every running backup's start time from agent heartbeats. It delays a prune while any backup against that repo has been running longer than `keep-delete − 1 h`, and it warns when a plan's historical duration gets close to `keep-delete`. For a first backup that may take days, the UI suggests raising `keep-delete` (for example to `3d`) until it finishes.

**Check** runs weekly by default, with an optional `read-data-subset` such as `5%` for a rolling data check.

**Restic warning.** If a restic client must still write to the repo, only its **backups** are safe (they take a shared lock, which rustic ignores, and restic simply doesn't see packs in `packs_to_delete`). **restic `prune` and `check` must never run** on a repo Steading maintains, because restic would delete the packs rustic marked for delayed deletion. The repo page says this explicitly.

---

## 8. Technology choices

### 8.1 Frontend: Leptos CSR with an explicit REST API

| Option | Pros | Cons |
|---|---|---|
| **CSR with explicit REST + WebSocket API** (chosen) | Single `trunk` build output that's easy to embed. The API is a real, versioned contract that the hub, the CLI and a future Tauri shell can all reuse. Shared types via an `api` crate | A bit more boilerplate than server functions |
| SSR + hydration with server functions | Less API code. Fast first paint | Couples UI and server build (`cargo-leptos`, two targets). Server fns aren't a stable contract for agents or a CLI. SEO and first paint don't matter for a localhost dashboard |

UI kit: plain Leptos plus Tailwind (with the `trunk` Tailwind hook) or a small hand-rolled component set. Charts (snapshot sizes over time, durations) use SVG rendered in Leptos, with no JS chart library.

### 8.2 Backend crates

| Concern | Crate |
|---|---|
| Async runtime / HTTP | `tokio`, `axum 0.8`, `tower-http` |
| Engine | `rustic_core 0.13`, `rustic_backend 0.7` (pinned, wrapped in our `engine` crate) |
| DB | `sqlx` (SQLite, compile-time-checked queries, migrations) |
| Scheduling | `croner` |
| Embedding UI | `rust-embed` |
| Auth | `argon2`, `axum-login` or a small cookie-session layer |
| Crypto / identity | `ed25519-dalek`, `rustls` |
| Secrets | `keyring` (macOS Keychain, Secret Service on Linux) |
| Notifications | `reqwest` (webhooks, ntfy, Gotify, healthchecks.io), `lettre` (SMTP) |
| CLI | `clap` |

### 8.3 Workspace layout

```
steading/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ api/                    # serde types shared by daemon, web (wasm), worker, sync protocol
│  ├─ engine/                 # rustic_core wrapper: JobSpec → operation, EventProgressBars,
│  │                          # profile import/export, config parsing
│  ├─ daemon/                 # axum server, scheduler, store, job runner, hub + hub client
│  ├─ web/                    # Leptos CSR app (trunk)
│  └─ platform/               # power/network conditions, service install (systemd/launchd)
├─ src/main.rs                # `steading` binary: serve | worker | pair | import-profile | export-profile | install-service
├─ migrations/
└─ docs/
```

`engine` is the only crate that depends on `rustic_core`, which contains the impact of its pre-1.0 API churn. `api` compiles to `wasm32` (no tokio, no rustic_core).

---

## 9. Data model (SQLite)

```sql
-- runtime state; config lives in config.toml
CREATE TABLE operations (
  id TEXT PRIMARY KEY,            -- UUIDv7 (time-ordered)
  agent_id TEXT NOT NULL,         -- 'local' on agents; registry id on hub
  kind TEXT NOT NULL,             -- backup|forget|prune|check|restore|stats|...
  plan_id TEXT, repo_id TEXT,
  trigger TEXT NOT NULL,          -- schedule|manual|catch-up|hub
  status TEXT NOT NULL,           -- queued|running|success|warning|failed|cancelled
  queued_at INTEGER, started_at INTEGER, finished_at INTEGER,
  snapshot_id TEXT,               -- for backups
  summary_json TEXT,              -- OpResult (files new/changed, bytes added, duration, …)
  error TEXT,
  warning_count INTEGER DEFAULT 0
);
CREATE INDEX ops_by_plan ON operations(plan_id, started_at DESC);

CREATE TABLE op_warnings (op_id TEXT, item TEXT, during TEXT, message TEXT);
CREATE TABLE op_logs     (op_id TEXT, seq INTEGER, ts INTEGER, level TEXT, line TEXT,
                          PRIMARY KEY (op_id, seq));      -- retained N days

CREATE TABLE schedule_state (plan_id TEXT PRIMARY KEY, next_run INTEGER, last_success INTEGER);

CREATE TABLE snapshot_cache (     -- refreshed after backups/forget, and on demand
  repo_id TEXT, id TEXT, time INTEGER, hostname TEXT, label TEXT,
  paths_json TEXT, tags_json TEXT, summary_json TEXT,
  PRIMARY KEY (repo_id, id)
);

-- hub only
CREATE TABLE agents (id TEXT PRIMARY KEY, hostname TEXT, pubkey BLOB, version TEXT,
                     paired_at INTEGER, last_seen INTEGER, last_acked_seq INTEGER);
CREATE TABLE agent_plans (agent_id TEXT, plan_id TEXT, plan_json TEXT, updated_at INTEGER,
                          PRIMARY KEY (agent_id, plan_id));
```

Retention: logs 30 days, operations 1 year (configurable), snapshot cache rebuilt as needed.

---

## 10. Security

### 10.1 Web UI

- Binds to `127.0.0.1` by default. Changing to `0.0.0.0` requires auth to be configured first.
- Single admin user with an argon2id password hash, `HttpOnly`/`SameSite=Strict` session cookies and CSRF protection on mutating routes.
- Optional "trusted reverse-proxy header" mode for SSO behind Authelia or Authentik.

### 10.2 Repository secrets

In order of preference:
1. `password-command`, as rustic supports. For example macOS `security find-generic-password …`, `secret-tool lookup …` or `pass show …`.
2. OS keyring via `keyring`, which the UI can write to.
3. `password-file` (mode 0600 enforced).
4. An inline `password` in config is accepted on import, with a warning and an offer to move it into the keyring.

Secrets are passed to workers through the `JobSpec` on stdin, never as argv or environment variables. **Secrets never leave the agent.** Hub maintenance needs the repo password, so for hub-managed repos the **hub has its own secret entry** for that repo, configured on the hub. Agents don't push passwords.

### 10.3 Agent ↔ hub

Ed25519 identities, TLS (pinned or CA), and single-use short-lived pairing tokens. The protocol carries metadata and logs, never file contents or secrets.

---

## 11. Notifications

Configured on the hub (or on a standalone agent). Each rule is **event → channel**:

- Events: `backup.failed`, `backup.warning`, `backup.missed` (no success within N × interval), `maintenance.failed`, `check.errors`, `agent.offline` (> N min), `repo.unmanaged_groups`.
- Channels: generic webhook (JSON), ntfy, Gotify, Discord/Slack webhook, SMTP.
- **healthchecks.io-style pings per plan**: `start`, `success` and `fail` URLs. These are the most robust "it silently stopped" detector, because they work even when the whole Steading system is down.

---

## 12. Packaging and deployment

- **Linux:** a static-ish `x86_64`/`aarch64` binary. `steading install-service [--user|--system]` writes a systemd unit (a user unit on desktops, a system unit on servers).
  - On **Bazzite and other atomic Fedora** systems, put it in `~/.local/bin` (or `/usr/local/bin`, which is writable) with a user unit. No layering needed.
- **macOS:** a universal binary, with `install-service` writing a launchd agent plist. The worker needs **Full Disk Access** (TCC) to read `~/Documents`, `~/Desktop`, Mail and so on. The UI detects permission errors in warnings and links to System Settings → Privacy & Security.
- **Docker:** an image for hub deployments on servers or a NAS.
- Distribution: GitHub Releases (cargo-dist), a Homebrew tap, and possibly the AUR later.

---

## 13. Milestones

Rough effort assumes one experienced developer working part-time (~10–15 h/week).

| # | Milestone | Deliverables | Effort |
|---|---|---|---|
| **M0** | Skeleton | Workspace, CI (fmt, clippy, test, wasm build), `api` crate, worker protocol with a fake operation, daemon serving an embedded "hello" Leptos app | 1 wk |
| **M1** | Single machine MVP | Repo connect/init/test; plans in `config.toml`; profile **import** and export; manual backup via worker with live progress in UI; snapshot list; op log and history; forget/prune/check run manually on the local owner | 3–5 wks |
| **M2** | Schedules and errors | Scheduler with catch-up and jitter; warnings vs failures; notifications (webhook, ntfy, healthchecks); service install (systemd, launchd); auth | 2–3 wks |
| **M3** | Browse and restore | Snapshot tree browser; restore to path; download via `dump` (tar/zip); repo stats page | 2 wks |
| **M4a** | Hub: visibility | Pairing, sync protocol, aggregated dashboard, event replay, agent offline alerts | 3–4 wks |
| **M4b** | Hub: control and maintenance | Hub-managed forget/prune/check with plan-matched retention, unmanaged-group handling, long-backup guard; remote run/cancel; opt-in remote plan edits | 3–4 wks |
| **M5** | Polish | Conditions (AC power / metered), charts, Docker image, release packaging, docs | 2–3 wks |

A **usable personal tool** (M0–M2: scheduled, lock-free backups with notifications on each machine) takes about **6–9 weeks part-time**. **Backrest-like multi-host management** (through M4b) takes about **4–5 months part-time**.

---

## 14. Risks

| Risk | Mitigation |
|---|---|
| `rustic_core` is pre-1.0, so its API changes | Pin exact versions. All usage goes through the `engine` crate. Contract tests run against a temp repo in CI |
| `deny_unknown_fields` and flattening when parsing profiles | Key-partitioning parser (§6.1) with a test that detects new upstream fields |
| Divergence from rustic CLI profile semantics (merging, `use-profiles`, env substitution) | Golden tests: parse the rustic repo's `config/*.toml` examples and compare against expected plans |
| A long backup collides with prune's `keep-delete` window | Long-backup guard (§7.3) and duration warnings |
| Leptos 0.8 → 0.9 migration | Keep UI components thin and logic in `api`/view-models. Migrate once 0.9 is stable |
| macOS TCC permissions | Detect them and link to the settings pane. Document this in setup |
| Memory use on very large repos | Worker isolation. Document `--no-cache` and cache dir options. Measure early with a large test repo |
| Hub is a single point of control | Agents back up without the hub. Maintenance can fall back to `local` on a chosen agent |

---

## 15. Open questions

1. ~~**Name.**~~ Settled: Steading.
2. **Config editing from the hub.** Ship M4b with read-only remote plans, or allow edits from the start (opt-in per agent)?
3. **Secrets on the hub.** Is a separate hub-side password entry per repo acceptable, or should agents be able to delegate a password to the hub, encrypted to the hub's key?
4. **Windows.** Is it in scope for v1? (It changes the service install, path handling and VSS snapshots.)
5. **Hot/cold repos** (`repo-hot`). Support them in the UI from M1, or leave them config-only?
6. **Restore permissions.** Should restores from the hub UI to an agent require re-authentication or a confirmation on that agent?
