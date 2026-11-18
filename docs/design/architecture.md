## 3. Architecture

### 3.0 Runtime topology

```
                       ┌──────────────────────── hub machine ─────────────────────────┐
  Browser ──HTTPS──►   │  ladderback serve --hub                                         │
  (Leptos WASM UI)     │   ├─ HTTP API + UI (axum)      ├─ Hub registry (agents, plans,│
                       │   ├─ Scheduler                 │   repo owners)               │
                       │   ├─ Op log (SQLite)           ├─ Maintenance runner          │
                       │   │                            │   (forget/prune/check)       │
                       │   └─ local agent ──spawns──►  ladderback worker ──► repo        │
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

**One binary, three roles, plus an MCP server on demand:**

| Role | Command | Runs where | Responsibility |
|---|---|---|---|
| **Agent** | `ladderback serve` | every machine | Local config, scheduler, op log, spawning workers, local web UI. Works fully standalone. |
| **Hub** | `ladderback serve --hub` | one machine (often a server) | Everything an agent does, plus an agent registry, aggregated history, the repo ownership registry, maintenance for hub-managed repos, notifications |
| **Worker** | `ladderback worker` (internal) | spawned per operation | Runs exactly one `rustic_core` operation, streams events to stdout, then exits |
| **MCP server** | `ladderback mcp` (started by an MCP client) | wherever the client runs | Lets AI agents and scripts drive a running agent (a hub from M9), or a headless session of their own, over stdio (§4.6) |

Every agent is useful alone. Hub mode is opt-in, and an agent that can't reach the hub keeps backing up on schedule. It stores events in a persistent outbox and replays them when it reconnects.

### 3.1 Domain model (DDD)

Ladderback's hard problems are domain rules, not CRUD: who may prune a repository, which snapshots a policy may delete, when a backup must give up, when a missed run catches up, what a restore may overwrite. Those rules get names, types and tests of their own, independent of rustic, SQLite, axum or the sync protocol.

**Ubiquitous language.** These terms mean exactly this in code, tests, UI copy and docs.

| Term | Meaning |
|---|---|
| Agent | One machine running `ladderback serve`, identified by its agent ID |
| Hub | The agent running `--hub`; holds the registry, ownership and aggregated history |
| Agent key | The Ed25519 key pair an agent makes once and keeps in its data folder, never in its database. The hub registers its public half at pairing and checks every connection's proof against it (§7.1; §16, decision 120) |
| Hub key | The Ed25519 key pair the hub makes once, kept in its data folder like the agent key. Every agent pins its public half at pairing, whichever way the hub's TLS is served, and the hub signs what gives an agent authority with it. Not the key of the hub's own agent (§7.1; §16, decision 120) |
| Pairing token | What the hub shows once to pair one agent: the hub key, the certificate's fingerprint when the hub serves TLS itself, and a one-time key that authorizes one pairing. Single-use, short-lived, and the hub keeps only the one-time key's public half (§7.1; §16, decision 120) |
| Signed hub message | A hub message that gives an agent authority or makes it act (`Welcome`, `RepoGrants`, and from M9 `RunPlan`, `CancelOp` and `RestoreRequest`), signed by the hub key for one agent and one connection, with a sequence number and an expiry. An agent obeys no such message unsigned, replayed or expired (§7.2; §16, decision 120) |
| Handover | A statement signed by the old hub key naming the new one, which lets agents that pinned the old key follow a rotation (§7.1; §16, decision 120) |
| Service | An agent the user's **service manager** runs in the background, from login on: a systemd user unit on Linux, a launchd agent on macOS. Its **service file** says what the manager runs: this `ladderback` program's `serve`. `ladderback install-service` writes the file and has the manager start it; `ladderback remove-service` has the manager stop it and removes the file (§12). On Linux the service stops with the user's last session unless the user **lingers** (`loginctl enable-linger`) |
| Build | The program as built from one commit: its **version**, the **commit** (unknown when the build had no `git` to ask), whether tracked files had uncommitted changes (**dirty**), and whether the commit is its version's **release** (the tag `v<version>` names it). Shown as `0.3.0` for a clean build of the release, otherwise as `0.3.0+3f937cf`, or `0.3.0+3f937cf.dirty` with changes (decision 90) |
| Release check | The agent asking GitHub Releases for Ladderback's newest release while it serves: a day after its last check, never within 5 minutes of its start (5 to 65 minutes after it starts when it has never checked or that day is over), each time spread by up to an hour of jitter. On unless `[agent] check-for-updates = false`; the last one is kept in the store, so a restart neither asks again within the day nor forgets what it found (§5, §10.7; §16, decision 107) |
| Available update | The newest release a release check found, when it is a full release (not marked a prerelease, a draft or a version with a prerelease part) with a higher version than the one running. The header shows it with a link to its notes; the log says so once per version (§16, decision 107) |
| Full Disk Access probe | On macOS, the agent listing a folder only a Full Disk Access grant opens, to learn whether the person has granted it: **granted**, **missing** (the listing is refused) or **unknown** (any other answer, never reported as missing). It runs when the agent starts, before each backup and when a person asks to check again. A backup item refused while the probe before it found the grant missing gets a warning of its own (§16, decision 115) |
| Plan | What to back up, from which agent, into which repository, when, and how long to keep it |
| AI agent | A language-model program acting through MCP or the API, such as a coding assistant. Never called an agent alone, which is a machine running Ladderback |
| Configuration change | One typed edit of an agent's configuration: add, change or remove a repository, a plan or a hook, set a plan's retention, claim maintenance. Applied by `AgentConfig`, which keeps its rules; the web UI's plan editor and a proposal both write the configuration through it (§16, decision 89) |
| Proposal | Configuration changes an AI agent (or another API client) submits for a person to confirm, with the AI agent's rationale and the configuration they were based on. It changes nothing until a person approves the whole of it in the web UI, with step-up confirmation and a tick for each command it adds; it can also be rejected, withdrawn or expire, or go **stale** when a change no longer applies to the current configuration. Its summary is written by Ladderback from the changes, never by the agent (§10.5; §16, decision 89) |
| Plan dry run | Walking a draft plan's sources through its source filter to count the files and bytes it would back up and what each exclude removes, never naming a path. It opens no repository, runs no hook and reads only within the **dry-run allowlist**, the folders a person lists in Settings, none by default. Not forget's dry run (`preview_forget`) (§16, decision 89) |
| Repository | A rustic repository, identified by its **repository ID**. An **alias** is one agent's local name for it |
| Repository stats | Four figures read from a repository: how many snapshots it holds, their **total size** (the sources they hold, added up), its **stored size** (what its packs hold for them, after deduplication and compression) and **compression** (the share of the raw data's size compression saved). A **reading** is the figures with the time they were taken; readings are kept a year (§16, decision 104) |
| Location | One place a repository can be reached: a `repository` string, an optional `repo-hot` and the three option sets, with the credentials they hold, and an optional location rule. A repository has one to four, in order; every one must hold the repository's own ID. The password isn't a location's: it opens the repository, whichever location is used. A location's options never apply to another (§6.1; §16, decision 80) |
| Hot half, cold half | The two places a location with a `repo-hot` stores its repository in, as rustic splits it: the **hot half** (`repo-hot`, with `options-hot` over `options`) holds every file but the packs of file contents, so reading the index and the snapshots stays cheap, and the **cold half** (`repository`, with `options-cold` over `options`) holds everything. rustic writes the cold half first. A refusal of the credentials names the half whose server refused (`RepositoryHalf`, §16, decision 112) |
| Location rule | A condition on the network link that must hold before a location is tried: the link is wired, is Wi-Fi, or is a Wi-Fi network named in a list (or not in one). It chooses which location to try and never makes one trusted; a rule that can't be evaluated doesn't hold (§6.1, §10.6; §16, decision 80) |
| Mixed link | A network link whose IPv4 and IPv6 default routes leave by different interfaces: it names the kind of each, and a location rule holds on it only if it holds for both (§16, decision 80) |
| Network link | The network the machine reaches others by: the interface each IP family's default route leaves by, as wired, Wi-Fi (with the network's name, when it can be read) or other; a mixed link when the two families leave by different interfaces (§16, decision 80) |
| Probe | A bounded attempt to open one location with its own credentials, in a worker of its own, to learn whether it answers and which repository it holds. The daemon uses the first location, in order, that is eligible and whose probe finds the repository's ID, however soon a later one answers (§5; §16, decisions 80 and 86) |
| Early probe | A probe started at the start of the choice, before the location's turn, beside the others: only for a location whose rule holds and that makes the server prove who it is before it is sent a credential, or sends none (a local path, `rest:https://` with its certificate checked). Its answer waits for every earlier location's; any other location, a plain-HTTP one above all, is probed only in its turn (§10.6; §16, decision 86) |
| Operation | One run of one action (backup, forget, prune, check, restore, …) with a lifecycle: `Queued` → `Running` → `Succeeded` / `SucceededWithWarnings` / `Failed { reason }` / `Cancelled`. A missed deadline is `Failed { reason: DeadlinePassed }` |
| Step | One stage of a running operation as the engine names it: reading the index, reading the snapshots, backing up, finding the data the snapshots use, and the others decision 82 lists. A step has its own progress, and a page shows the step a run is in with how far that step has come. A stage the engine names in words Ladderback doesn't know is no step: the page says the run is working (§4.4; §16, decision 82) |
| Trigger | What started an operation: `schedule`, `manual`, `catch-up`, `retry`, `hub`, `mcp`, or `api-token` with the token's label and ID (never its secret) |
| Operation log (history) | The record of every operation: when it was queued, started and finished, how it ended, what the engine reported, and its first 10,000 warnings with the total. It is written as an operation runs and never needed to run one |
| Recent log | The newest lines of the daemon's own log since it started, kept in memory and shown on the Log page: each with when it was written (UTC, to the millisecond), its level, its source (the part of Ladderback, or the crate, that wrote it) and its message, as the log wrote it and masked as the log is (I4). At most 1,000 lines and 1 MiB of text, the oldest let go first; a message is cut at 4 KiB and a source at 256 bytes, each ending in `…`. A restart starts it empty (`domain::daemon_log::RecentLog`, decision 111) |
| Persistent queue | The operations the operation log holds as `Queued`, waiting for their turn. It outlives the daemon: when the daemon starts again, a queued backup of a plan the configuration still has, on the repository it was queued for, runs under its own ID, built from the configuration of that moment; any other queued operation, or a backup that can't be built, ends `Failed { reason: DaemonStopped }` (§16, decision 73) |
| Orderly stop | The daemon stopping because it was told to (a service stop or restart, a restart asked for in Settings, Ctrl-C, the computer shutting down), not a crash, a kill or a power cut. It stops its workers, then ends each operation still running as `Failed { reason: DaemonShutDown }`, recorded and not announced; queued ones stay queued. A running operation a stop didn't get as far as ending, like one a crash left, ends `Failed { reason: DaemonStopped }` at the next start, and is announced (§4.2; §16, decisions 91, 92 and 94) |
| Snapshot | A rustic snapshot, read from the repository; never created or edited except through an operation |
| Pack / index | rustic's terms: a pack is a file of the repository's data, written by a backup or a prune; the index lists what each pack holds. Data in a pack no index lists can't be reused by a backup, and a check warns about the pack until a prune removes it |
| Cache slot | One running worker's folder of rustic's cache for one repository: `<cache folder>/<repository ID>/<n>`, the lowest `n` no other running worker of that repository holds. The worker holds it with an exclusive lock while it runs, so no two workers share one, and the next operation on the repository reads the index, the snapshots and the tree packs from it instead of from the repository. Nothing in it is needed: a lost cache is read again, and each time it starts the daemon removes the slots no worker will use again (§4.1, §16 decision 61) |
| Snapshot host | The host name a snapshot records: its plan's `host`, else the agent's `hostname`, else the machine's own name, which rustic chooses |
| Snapshot time | When a snapshot was taken, with the offset of the clock that took it (`2026-11-10 23:30:00-06:00`). A calendar retention rule counts the snapshot on that clock's hour, day, week, month and year, wherever the forget runs (§16, decision 56) |
| Snapshot tree | A snapshot's folders and files, as it records them: each folder's entries in the snapshot's order, each with its key (rustic's escaped form of its name, which is how a request names it), its shown name (the name as the file system wrote it, as `rustic ls` prints it, with each control, bidirectional or separator character made visible), kind (folder, file, link or other), size and modification time. Browsed one folder at a time, each read live from the repository in pages of at most 1,000 entries; a read, so it takes no maintenance slot and writes nothing to the operation log (§4.4, §4.5; §16, decision 102) |
| Other host | The host a snapshot records, in a repository this agent maintains, when the snapshot came from another machine: this agent didn't tag it, and either another agent tagged it in the last 30 days (or later), whatever host it records, since two machines can share one name, or its host is none of this agent's: not the machine's own name, the agent's `hostname`, or any plan's `host`. A snapshot this agent tagged is this host's under any name, as one made before the machine was renamed; one whose agent tags disagree counts as another agent's. Its snapshots suggest that another machine writes to the repository and may maintain it too (§7.3; §16, decision 62) |
| Source filter | What a plan leaves out of its backup: glob patterns, glob files, ignore files such as `.gitignore`, marker files, extended attributes and other file systems (`SourceFilter`, §6.1). rustic reads a glob with a slash in the middle from the root of the file system; one written from a source reaches none and is a never-matching glob (`NeverMatchingGlob`, warned of as `glob-never-matches`, #510) |
| Plan tags | `ladderback-plan:<id>` and `ladderback-agent:<id>`; they make a snapshot **managed** by a plan. Two tags of one kind that carry different text name no owner, and the snapshot is orphaned (§7.3) |
| Unmanaged group | Untagged snapshots, grouped by the repository's `group-by` |
| Orphaned group | Tagged snapshots whose plan or agent no longer exists, or whose agent tags or plan tags disagree |
| Adopt | Retag an unmanaged or orphaned group into a plan |
| Maintenance owner | The single party that runs forget, prune and check for a repository |
| Claim / Grant / Lease | An agent's request to own maintenance; the hub's acceptance; the time-limited, epoch-numbered form of that acceptance |
| Deadline | The latest time a backup may write its snapshot (I3) |
| Prune margin | 1 h + the longest recorded prune on that repository |
| Catch-up | One run in place of any number of runs missed while the agent was off or asleep |
| Next run | When a plan runs next: the schedule's next moment after the last run started (a cron expression's on the wall clock of its zone, or an `every` interval in real time), plus jitter; planned when a run starts and kept while the agent is away (§5) |
| Jitter | How far after its moment a run may be put, so that machines on one schedule don't start at once; never as far as the schedule's following moment |
| Wake | The machine waking from sleep, or its clock set forward: wall-clock time ran more than 2 min ahead of monotonic time between two samples of the clocks 30 s apart. A run missed meanwhile can catch up, and a catch-up run after a wake first waits up to 2 min for the network (§5) |
| Overlap policy | What a plan does when its next run comes due while its last run is still going (running, or waiting in the persistent queue): `skip` it (the default) or `queue` it to start once the last one ends (`Overlap`, §5). A plan runs one backup at a time and never has two runs waiting |
| Concurrency limit | How many backups the agent runs at once, across all its plans: its `concurrency`, 1 unless the configuration says otherwise. A backup beyond it waits in the persistent queue for a running one to end (`BackupQueue`, §5) |
| Retry | A backup a plan's `retry` policy runs again after a backup the schedule started failed: at most `attempts` of them, the first `backoff` after the failure and each after twice the wait before it, at most 1 h. A **series** of retries stops at the plan's next scheduled run, when its attempts are spent, or at a success or a cancel; a retry is skipped while a run of its plan is going. A backup a person or another program asked for is never retried (`RetrySchedule`, §5; §16, decision 76) |
| Notification rule | Which **events** an agent tells, for which plans (all when it names none), and the **channel** it tells them to: `events → channel` (§11). An event is a fact Execution reports, named as `backup.failed` |
| Channel | Where a notification goes: a webhook, an ntfy topic or a healthchecks check, by its URL. The URL, and the token or password it signs in with, are secrets, read only when a notification is sent |
| Template | A rule's own text for its messages, with placeholders for the event's facts (`{plan}`, `{code}`, …) and nothing else, so it can't hold a secret (I4). Without one, the message is the catalog's text in the agent's locale |
| Step-up confirmation | Re-entering the password (or, when auth is off, typing the snapshot's or the proposal's short ID) before a restore, a download or approving a proposal |
| Download | One entry of a snapshot handed to the browser of the person who asked: a file as itself, a folder or the snapshot's top as one archive in the format they pick (tar or zip). Confirmed by step-up, handed over once within a minute of the confirmation, and streamed a piece at a time. It is never queued and holds no maintenance slot, but the history records it as it does a restore (§10.4) |
| Admin password | The one password that signs in to an agent's web UI and API. Set with `ladderback set-password`, at the terminal or from standard input, or in Settings, where changing it takes the current one; only its argon2id hash is kept (§10.1) |
| Sign-in | Asking for the admin password before the web UI and API answer. **On** while an admin password is set, which the daemon decides when it starts; **off** otherwise, and then anyone who can reach the agent can use it: anyone on this machine on loopback, anyone who reaches the address on the network, where the agent and every page warn so (§10.1) |
| Exposure | Who can reach an agent at the address it listens on: this machine, on a loopback address, or **the network**, on any other, which needs an `--allowed-host` (§10.1) |
| Session | What a sign-in starts: a cookie and the CSRF token its changed requests carry. It ends at sign-out, at **Sign out everywhere**, at the end of the **session length** after it started, 12 hours after its last **use** when it has an **idle limit**, or when the admin password is set anew. Kept in the store by its token's digest, so it outlives a restart (§10.1) |
| Idle limit | The 12 hours a session may go unused before it ends. A session has one unless the person ticked **Keep me signed in on this device** when signing in, and every session has one while the session length is Until I close the browser |
| Session length | How long every session lasts after its sign-in, set in Settings as **Stay signed in for**: Until I close the browser, 7 days, 30 days (the default), 90 days, 1 year or Until I sign out. It applies to sessions already open, measured from each one's sign-in (§10.1) |
| Sign out everywhere | Ending every browser's session at once, this one's included, from Settings; API tokens are untouched |
| API token | What lets a program other than a browser, such as MCP connect mode, use the API while sign-in is on: `ladderback_<id>_<secret>`, with a label and its **scopes** (`read`, `run`, `preview`, `propose`), each opening the routes it names. Created in Settings, where its secret is shown once; it lives until it is revoked or the admin password is set anew. Not a session's or a CSRF token (§10.1; §16, decision 93) |
| Background read | A request the web UI sends on its own, not because a person did something: a refresh after an event, opening the events stream. It never uses a session, so a page left open doesn't keep one alive (§10.1) |

**Bounded contexts.**

| Context | Kind | Aggregates and domain services | Rules it protects |
|---|---|---|---|
| **Shared kernel** | — | `RepositoryId`, `AgentId`, `PlanId`, `OperationId`, `SnapshotRef { id, time, host, label, paths, tags }`, `Timestamp` | Used by every context; changes need agreement across all of them |
| **Planning** | Core | `AgentConfig` (root; owns `Plan` and `RepositoryConfig` entities; its configuration changes are its methods), `Proposal` (root; base configuration hash, changes, rationale, status, expiry), value objects `Schedule`, `RetentionPolicy`, `SourceFilter`, `Hooks`, `RetryPolicy`, `SecretRef`, `Location`, `LocationRule`, `ConfigChange` | Plan IDs unique per agent; a plan names an existing repository alias; schedules and retention validate; a repository has one to four locations, each with its own settings and none inheriting another's; a proposal changes nothing until approved, names secret sources never values, and is approved whole, against the diff that was shown |
| **Execution** | Core | `Operation` (root; one run's lifecycle), `PlanSchedule` (root; per plan: `next_run`, `last_success`, active operation, retry attempt), domain services `NextRun` (cron, interval, jitter, time zone, catch-up), `Deadline`, `RetrySchedule`, `WakeDetector` (wall-clock vs monotonic samples) | Valid lifecycle transitions; catch-up collapses missed runs; overlap policy; deadline (I3); retries stop at the next scheduled run |
| **Maintenance** | Core | `RepositoryOwnership` (root; claims, grant, lease, epoch; the "no other agent uses this repo" facts are passed in by the use case), domain services `ForgetSelection` (managed / orphaned / unmanaged partition and the delete list), `PruneGuard` | One owner (I1); never instant delete (I2); forget only touches managed snapshots (I6); serialized maintenance (I7, enforced atomically by the store) |
| **Restore** | Supporting | `RestoreRequest` (root), `RestorePolicy` | New folder by default; in-place only from the agent's own UI; hub restores need `allow-hub-restore`; step-up confirmation |
| **Fleet** | Supporting | `Agent` (root; identity, hostname), `PairingToken` (single use, TTL), `HostnameRegistry` domain service | Unique hostnames; tokens used once; re-pairing reuses identity |
| **Profile interop** | Supporting | Anti-corruption layer only (`adapters/interop`): translates rustic profiles to and from Planning | List replacement, hook order, `[snapshot-filter]` export rules (§6.2) |
| **Secrets** | Generic | `SecretRef`, `Redactor` (replaces every resolved secret value, and its common encodings, with `***`) | No secret in logs, templates or sync messages (I4) |
| **Notifications, auth** | Generic | `Notifications` (the channels and the rules, owned by `AgentConfig`), value objects `Channel`, `Rule`, `Template`, `EventKind`; `Sessions`, `SignInWaits` and `ApiToken` (`domain::access`) | A rule names a channel and plans the configuration has; a channel holds no secret as text and starts no program by a relative path; a template reads only an event's facts (I4). A session ends unused or old, and with a new admin password; a changed request carries its session's CSRF token; failed sign-ins make the next one wait, never lock the admin out; an API token opens only the routes of its scopes, ends with a new admin password, and a failed one counts as a failed sign-in |
| **Updates** | Generic | Value objects `Version` (SemVer 2.0.0 precedence) and `Release`, domain services `available_update` and `next_check` (`domain::updates`) | A prerelease or a version no higher than the running one is never an update; a check comes at most once a day, never within 5 minutes of the agent's start, after jitter, and, planned on a clock set back, never more than a day ahead; outside text that isn't a version is refused, never shown |

**Context map.** Planning feeds Execution (plans become operations) and, through the hub, Maintenance (published plans carry retention). Execution emits domain events that Maintenance, notifications and the sync outbox consume. Two anti-corruption layers keep foreign models out: profile interop for rustic's TOML model, and the engine adapter for `rustic_core`'s types. The sync protocol in `api` is the published language between agent and hub; `app` speaks its own `AgentReport` and `HubCommand` types, and the sync adapter translates.

**Domain events and their consumers.**

| Event | Consumed by |
|---|---|
| `OperationQueued`, `OperationStarted`, `OperationFinished { outcome }` | UI live updates, sync outbox, notifications (`backup.*`, `forget.failed`, `prune.failed`, `check.failed`, `restore.failed`) |
| `SnapshotCreated` | Snapshot catalog refresh |
| `RetryScheduled`, `BackupMissed` | Notifications (`backup.retrying`, `backup.missed`) |
| `CheckFoundErrors` | Notifications (`check.errors`) |
| `PruneRefused`, `ClaimRejected`, `MaintenancePaused { reason }` | Notifications (`prune.refused`, `repo.claim_rejected`, `repo.maintenance_paused`) |
| `MaintenanceGranted`, `LeaseExpired` | Maintenance scheduling on the agent |
| `UnmanagedGroupsFound` | Notifications (`repo.unmanaged_groups`), repository page |
| `AgentPaired`, `AgentWentOffline` | Dashboard; notifications (`agent.offline`) |

The sync outbox (§7.2) persists the events the hub needs.

**Retention evaluation stays in rustic.** `ForgetSelection` decides *which* snapshots each policy applies to; applying `keep-*` rules to a group is delegated to the `Retention` port, implemented with rustic's own `ForgetGroups`, so Ladderback never reimplements retention. The export golden test (§6.2) checks the two agree. A calendar rule counts each snapshot on the calendar of its own snapshot time, as rustic does, so the evaluation reads the repository's snapshot files, which carry the offset, never the listing, which carries it only to show it (§16, decision 56).

**Outside input is parsed once, where it enters.** Text from outside the process (`config.toml`, HTTP requests, imported profiles, worker output, sync messages) becomes domain types in the adapter, or the `api` type, that receives it. `RepositoryAlias::parse`, `PlanId::parse`, `RepositoryId::parse` and the other value objects return either a value that keeps its rules or an error the adapter reports. `domain` and `app` take those types, never the raw string, and don't check them again. A type that can't hold an invalid value is preferred to a check plus the test that proves it: an enum for a closed set of states (`OperationStatus`), and a newtype with a private field and a `parse` constructor for a constrained value, with no public way around its rules.

### 3.2 Code architecture: hexagonal, with a DDD core

Ladderback's complexity sits at its edges: one set of rules is driven by an HTTP API, a CLI, a scheduler and hub commands, and it drives a rustic engine in a child process, SQLite, a WebSocket, OS keyrings, power and wake signals, notification channels and the clock. Ports and adapters name exactly that, and let every rule be tested with fakes for all of it.

| Option | Verdict |
|---|---|
| **Hexagonal (ports and adapters) around a DDD core** (chosen) | Fits many inbound drivers and many swappable outbound systems. The domain and application crates form two inner rings, as in Onion |
| Clean Architecture | Same dependency rule, but its presenters and interactor/output-port pairs add ceremony with no payoff: the UI is a separate WASM app consuming JSON |
| Onion | Equivalent rings; Hexagonal's port vocabulary describes the many external actors better. The inner split is borrowed from it |
| Layered by technology (v0.2's `daemon` crate holding server, scheduler, store and runner) | Rejected: lease, deadline and forget rules would sit next to axum and sqlx, untestable without them |

```mermaid
flowchart LR
  subgraph Inbound adapters
    HTTP[axum HTTP API + UI]
    CLI[ladderback CLI]
    SYNCIN[sync: hub commands / agent reports]
    LOOPS[runtime loops: scheduler, job runner, lease timers]
  end
  subgraph Core
    APP[app: use cases + port traits]
    DOM[domain: aggregates, value objects, domain services, events]
  end
  subgraph Outbound adapters
    ENG[engine: worker process + rustic_core]
    STORE[store: SQLite]
    CFG[config: config.toml + migrations]
    SEC[secrets]
    NOTE[notify]
    REL[releases: GitHub Releases]
    PLAT[platform: clock, power, network, identity]
    SYNCOUT[sync: hub link]
  end
  HTTP --> APP
  CLI --> APP
  SYNCIN --> APP
  LOOPS --> APP
  APP --> DOM
  ENG -. implements port .-> APP
  STORE -. implements port .-> APP
  CFG -. implements port .-> APP
  SEC -. implements port .-> APP
  NOTE -. implements port .-> APP
  REL -. implements port .-> APP
  PLAT -. implements port .-> APP
  SYNCOUT -. implements port .-> APP
```

**Inbound ports (use cases in `app`).**

- **Agent:** `RunBackup`, `TickScheduler`, `CancelOperation`, `RunMaintenance` (forget, prune, check), `EnforceDeadlines`, `AdoptGroup`, `RequestRestore`, `Download`, `ImportProfile` / `ExportProfile`, `EditConfig`, `ApplyRepoGrants`, read-side queries for the dashboard.
- **Hub:** `PairAgent`, `ReportRepos` (claims), `DecideClaim`, `IngestHeartbeat`, `IngestOpEvents` (and acknowledge), `RenewLeases`, `ExpireLeases`, `MarkAgentOffline`, `ForwardCommand` (run, cancel, restore, fetch log).

Each adapter translates its input, calls one use case, and maps the result; no adapter holds a rule. The long-running loops (scheduler tick, job runner, lease timers, wake checks) are async functions in `app` driven by the `Clock` port; the composition root only spawns them.

**Outbound ports (traits in `app`).**

| Port | Capability | Real adapter | Test double |
|---|---|---|---|
| `Engine` | Repository ID, init, test connection, probe of one location (§16, decision 80), backup, list and delete snapshots, retag (for Adopt), prune, check, restore, ls, stats. `run` returns a `RunningJob`: a stream of `EngineEvent`s plus a `JobControl` whose `stop` ends the job gracefully (SIGTERM, a grace period, then SIGKILL); dropping the `RunningJob` kills the job at once. `dump` returns a byte stream carried on its own pipe, separate from the event stream | Spawns `ladderback worker` (links `rustic_core`) | Scripted fake engine |
| `Retention` | Pure: apply a `RetentionPolicy` to `SnapshotRef`s at a given `now` | `rustic_core::ForgetGroups`, in-process | Small keep-last / keep-within fake; the shared contract suite checks it against the real adapter |
| `OperationStore` | Operations and queue; `try_start` refuses a second running maintenance operation per repo (I7) | sqlx SQLite | In-memory |
| `StatsStore` | Keep a repository's stats readings for a year, and answer the latest (§16, decision 104) | sqlx SQLite | In-memory |
| `ScheduleStore`, `SnapshotCatalog`, `Outbox`, `LeaseCache` (agent), `OwnershipRegistry` and `AgentRegistry` (hub) | Persistence | sqlx SQLite | In-memory |
| `ConfigStore` | Load (parsing and migrating old schemas), and atomically save `AgentConfig` | TOML file with version history | In-memory |
| `SecretResolver` | Resolve a `SecretRef` | Command, keyring, file, env | Map-backed fake |
| `Keyring` | Keep a typed repository password in its OS keyring entry, never overwriting one that holds another secret, and remove an entry it added (§10.2, decision 110) | The `keyring` crate, as the resolver reads it, under the resolver's time limit | In-memory entries in the map-backed fake |
| `Clock` | Wall-clock and monotonic time, `sleep_until` (wall-clock) and `sleep` (monotonic: time asleep doesn't count) | System clocks (`Instant`), tokio timers | Controllable fake clock that fires timers when advanced, and can `suspend` the machine (wall-clock time moves, monotonic time doesn't) |
| `ApiTokenStore` | Keep, list, find and revoke API tokens, at most 100, and record when each was last used (§10.1) | sqlx SQLite | In-memory |
| `RecentLogStore` | Keep the recent log (§3.1) within its caps and list it newest first; the logger records into it from any thread, so a call never waits on more than the lines and never logs (decision 111) | `app::recent_log::RecentLogRing`: the domain's `RecentLog` behind a mutex, in memory, so it has no I/O to fake | The same ring, which the contract suite checks once |
| `Random` | Jitter, session, CSRF and API tokens; later UUIDv7 IDs, epochs, pairing tokens | OS randomness | Seeded |
| `AdminPassword` | Check an attempt against the admin password, set a new one, and a fingerprint that tells two kept passwords apart (§10.1) | argon2id hash file in the data dir, mode 0600 | In memory, as typed |
| `Identity` | The agent's Ed25519 key: public key and signing | Key file in the data dir | Fixed test key |
| `PowerSupply` | AC power or battery (§16, decision 63) | `/sys/class/power_supply` on Linux, `pmset -g batt` on macOS | Scripted fake |
| `Network` | Whether the machine is online (§16, decision 63) | `/sys/class/net` on Linux, `scutil --nwi` on macOS | Scripted fake |
| `NetworkLink` | The network link (§3.1): wired, Wi-Fi with its name when it can be read, other, or mixed (each IP family's default route apart), or why the platform can't say (§16, decision 80) | `/proc/net/route`, `/proc/net/ipv6_route`, `/sys/class/net` and NetworkManager's D-Bus API on Linux; `route`, `networksetup`, `ifconfig` and `system_profiler` run as decision 63 runs its commands, on macOS | Scripted fake |
| `ServiceManager` | Install, start, stop and remove the agent's user service (§12, decision 64) | systemd user unit through `systemctl --user` on Linux, launchd agent through `launchctl` on macOS | In-memory fake |
| `EventPublisher` | Deliver domain events to in-process subscribers (UI, outbox, notifications) | Broadcast channel | Recording fake |
| `Notifier` | Deliver a rendered notification | reqwest, lettre | Recording fake |
| `ReleaseFeed` | Ladderback's newest release: its version and whether it is a prerelease (§10.7; §16, decision 107) | reqwest `GET` of GitHub's latest-release API | Scripted fake that records when it was asked |
| `HubLink` / `AgentLink` | Send `AgentReport`s to the hub / `HubCommand`s to an agent | WebSocket sync | In-memory pair |
| `HookRunner` | Run hook commands with a scrubbed environment, their output masked with the run's redactor, and name the credentials that environment holds | Process spawner | Recording fake |

**Port style.** Ports are object-safe traits used as `Arc<dyn Port>`, written with `#[async_trait]` so their futures are `Send` and use cases can be spawned. One boxed future per port call costs nothing next to the I/O behind it, and the composition root stays readable with a dozen ports. Streams are `futures::stream::BoxStream`. No port signature mentions a tokio, sqlx, axum or rustic type.

**Port contract tests.** For each port, `app` exports a generic contract suite (feature `fakes`). The fake runs it in `app`'s tests, and each adapter crate runs the same suite against its real implementation, so a fake can't drift from reality.

**Dependency rule, enforced in CI.**

- `domain` has no I/O, no async and no ambient time or randomness. It may use `serde`, `uuid` (without `v4`/`v7` features), `jiff` (the workspace dependency turns on `tzdb-bundle-always`, so the build carries the IANA time-zone database and zones resolve on a host with no `tzdata`, such as a minimal container image, and in the browser, which has none; `jiff` reads the system's database first and uses the bundled one only when it finds none, §16 decision 30) and the cron crate `croner` with `default-features = false, features = ["jiff"]` (`chrono` is only croner's default; time goes through `jiff` alone, and `just architecture` bans `chrono` from `domain` and `app`, so a default-features slip fails CI; M0 confirms `jiff` and `croner` build for `wasm32`). Its error types implement `Display` and `Error` by hand: they carry codes and values, never sentences (§8.4).
- `app` depends only on `domain`, `api`, `async-trait` and `futures`. `api`, outside the rings, is an exception (§16, decision 106): the use cases return its `UiEvent`, `ErrorCode`, `ErrorDto` and `OperationStep`. It is a pure, wasm-safe crate of wire types with no I/O, so it can't bring I/O into the core; the cost is that a change to the wire format reaches the use cases.
- **Banned crates:** a CI step runs `cargo tree -e normal,build --prefix none` on `domain` (also with `--target wasm32-unknown-unknown`) and on `app`, and fails if `tokio`, `sqlx`, `axum`, `hyper`, `reqwest`, `rustic_core`, `rustic_backend`, `fluent-bundle`, `fluent-langneg`, `unic-langid`, `chrono` or any `icu_*` crate appears. Dev-dependencies are excluded, so tests may use a runtime.
- **Banned calls:** `clippy.toml` `disallowed-methods` / `disallowed-types` in `domain` and `app` forbid `std::fs`, the file-system methods of `std::path::Path`, `std::env`, `std::process`, `std::net`, `SystemTime::now`, `Instant::now`, `jiff::Timestamp::now`, `jiff::Zoned::now`, `Uuid::now_v7` and `rand`. Time and randomness come from `Clock` and `Random`. The two files are kept byte-identical, and the architecture check fails if they differ, or if a `.clippy.toml` sits beside either: clippy would read it instead and ignore the list.
- **wasm build:** CI builds `domain` for `wasm32-unknown-unknown`. This proves the UI can reuse its validation (schedules, retention syntax); purity is enforced by the two checks above, not by the wasm build.
- Adapters depend on `app`, `domain`, `api` and `spawn-gate`, never on each other, and `adapters/notify` and `adapters/mcp` on `l10n` too. Only the composition root (`src/main.rs` and its `composition` module) knows every adapter and wires them per role: agent, hub, worker, CLI, MCP. `l10n` is a presentation library outside the rings, used by `web`, `adapters/notify`, `adapters/mcp` (a refusal's sentence, §4.6) and the CLI (§8.4).
- **Outside the rings:** `api` (the wire contract) and `spawn-gate` (one child process starts at a time, so none inherits another's pipes) depend on no workspace crate; `l10n` depends on `api`; `web` on `api` and `l10n`; and `testkit`, test support that is only ever a dev-dependency, on `spawn-gate`.
- **The crate graph, enforced:** `just architecture` reads `cargo metadata` (no build) and fails, naming the edge, on any dependency between workspace crates that the table in `scripts/check-crate-graph.ts` doesn't allow. It checks every kind: normal, build, any target, optional or not. A dev-dependency may also be the crate itself or one its row allows in tests alone: `testkit`, and `spawn-gate` for `l10n`. A workspace crate with no row fails too. The table and this section change together (§13.7.6).
- **Owners of foreign models, enforced:** the same check fails when a crate other than its owner depends on a foreign model, or on a crate of its family (`sqlx-core`, `axum-core`, `toml_datetime`), as a normal or build dependency, on any target: `rustic_core`, `rustic_backend` and the other `rustic_*` crates (`engine`), `sqlx` (`store`), `toml` and `toml_edit` (`config-adapter`, `interop`), `keyring` and `dbus-secret-service` (`secrets`), `rmcp` (`mcp`), `axum` (`http-adapter` and the root crate). Dev-dependencies are exempt, as for the banned crates: a test may use one as an oracle, and nothing of it ships.
- **No async in `domain`, enforced:** the same check fails on an `async fn`, block or closure in `crates/domain/src`, outside comments and literals, and on a `#[path]`, an `include!` or a symbolic link there, which would bring in code it doesn't read.
- The worker binary is an adapter: it maps a `JobSpec` onto `rustic_core` calls and reports events. It makes no policy decisions: deadlines, snapshot IDs to delete and retag targets all arrive in the `JobSpec`. Its watchdog is a thin loop that applies the `Deadline` rule from `domain`.
