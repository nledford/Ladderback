## 4. Components

### 4.1 Worker process (the engine boundary)

Each operation (backup, forget, prune, check, restore, ls, dump, stats) runs in a **child process**:

```
daemon ──stdin: JobSpec (JSON)──► ladderback worker
daemon ◄─stdout: NDJSON Event stream── (progress, log, warning, result)
daemon ──SIGTERM → (grace) → SIGKILL──► cancel
```

Why a subprocess instead of `spawn_blocking`:

1. **Cancellation.** `rustic_core` has none, but a process can be killed. The worker installs a SIGTERM handler that stops emitting events and exits; without it the grace period would be meaningless. Killing is safe for data:
   - An interrupted backup leaves only unreferenced packs and no snapshot, and the next prune cleans them up. On the local backend it also leaves `*-tmp-*` files, which nothing in rustic removes. Once a job has opened a repository, the worker deletes the ones older than 24 h from each half that is a local folder (the cold half, and the hot half of a hot/cold repository), but only where the backend writes them: `config-tmp-…` in the half's root and `<id>-tmp-…` in `keys`, `snapshots`, `index` and `data/<xx>`. A job that fails to open its repository (a mistyped path, a wrong password, a missing hot half) deletes nothing.
   - Prune writes new packs and index files, then deletes the old index files, then the packs. A kill at any point leaves extra data or duplicate index entries, not missing data; the next prune deduplicates and finishes the job. This only holds without `early-delete-index` (I2).
   - M1a proves this with a kill test against a scratch repo, for the local backend and for OpenDAL's `fs` service. It kills real workers right after a chosen progress event, of a backup and of a prune that has packs to repack, spreading the kills over the events of an uninterrupted run, and then checks that the repository is sound and still takes a backup. OpenDAL's `fs` service writes in place unless `atomic_write_dir` is set, which can leave half a file under its final name, so the guarantee above holds for an `opendal:fs` repository only with `atomic_write_dir` (a directory on the same filesystem).
2. **No runtime nesting.** The backends' internal Tokio runtimes never meet the daemon's runtime.
3. **Isolation.** Memory from large indexes is returned to the OS when the worker exits, and a panic or out-of-memory kill in the engine can't take down the daemon.
4. **Deadline enforcement** (I3, §7.3). `rustic_core`'s `backup()` writes the snapshot itself and offers no hook before that write. Instead, once the worker has written `Started` for a job with a deadline budget, a watchdog thread compares wall-clock time against `index_read_at + budget` and, once it passes, writes `Failed { DeadlinePassed }` as the last line and calls `process::exit(75)`. Its first check runs before the job's work starts, so a zero budget stops the job before it writes anything. After that it looks again at least every `watchdog_interval` (5 s by default) in short sleeps, so a machine that was suspended is caught within one interval of waking. A writer stuck on a full pipe can't hold the exit back: the watchdog tries the stream once, and exits with 75 whether or not it could write the failure. The watchdog stops once the backup has returned: the snapshot is written, and the result goes out before the repository's index is dropped, so a deadline that passes meanwhile can't turn a finished backup into a failure. The daemon also kills the worker at the deadline. The only remaining window is a snapshot write already in progress, which the prune margin covers.
5. **Privilege separation (later).** On servers, the worker can run as root to read `/etc` while the daemon and web UI run unprivileged. This uses a small setuid-free helper started via a systemd unit, or `sudo` with a fixed command. Before building it, hooks and `password-command` must be restricted to root-owned config, or the unprivileged UI could escalate to root through them.

Each running worker of a repository whose ID is known gets its own `cache_dir`, `<cache-root>/ladderback/<repo-id>/<slot>`, because rustic's cache has no cross-process locking: the slot is the lowest number no other running worker of that repository uses, and it is free again when the worker exits. The worker also holds an exclusive `flock` on its slot directory for as long as it runs, and a daemon looking for a slot skips directories whose lock is held, so a worker that outlived a restarted daemon never shares its cache with a new one; a worker that finds its directory held runs with `no_cache`. Without a known ID or a cache root, the worker runs with `no_cache`. It runs with `nice` (and `ionice` on Linux) as configured per agent.

**Event protocol** (module `engine::job`, §8.3):

```rust
enum WorkerEvent {
    Started { op_id: Uuid, pid: u32, index_read_at: Timestamp }, // taken just before the index is read
    ProgressStart { key: u32, kind: ProgressKind /* Spinner|Counter|Bytes */, prefix: String },
    ProgressLength { key: u32, len: u64 },
    ProgressInc { key: u32, inc: u64 },    // coalesced: at most ~4 msgs/s per key
    ProgressTitle { key: u32, title: String },
    ProgressFinish { key: u32 },
    Warning { item: Option<String>, during: String, code: WarningCode, detail: String }, // from Progress::error; detail = rustic's text
    Log { level: Level, message: String },   // logs are never localized (§8.4)
    Result(OpResult), // BackupSummary | ForgetSummary | PruneStats | CheckReport | …
    Failed { code: ErrorCode, args: Vec<(String, MsgArg)>, detail: Option<String> }, // codes from `api` (§8.4)
}
```

`ProgressBars` is implemented by `EventProgressBars`, which writes these events. Per-file errors raised through `Progress::error` become **warnings**, so an operation can end as `Succeeded`, `SucceededWithWarnings` or `Failed` (§3.1). The worker builds a `Redactor` (§3.1) from its `JobSpec`'s repository and `repo_hot`, password, options, `options_hot`, `options_cold` and resolved secrets (`RepositorySpec.secrets`, copied from `RepositoryAccess.secrets`) and passes every `Log`, `Warning` and error through it (I4); §10.2 lists what it masks. A worker stopped by its watchdog emits nothing more; the daemon records the operation as `Failed { reason: DeadlinePassed }`.

**Message cap.** The daemon reads the worker's stdout one line at a time and the worker reads its stdin to the end, and each side reads at most `MAX_MESSAGE_BYTES` (1 MiB, in `engine::job`) of a message plus one byte to tell it from a longer one (§13.5, rule 2). An event line past the cap fails the operation as `EngineFailed` with the cap in its detail, and the daemon kills the worker; a worker answers a `JobSpec` past the cap with `InvalidJobSpec`; the daemon refuses to start a worker for a spec it would reject. A line that never ends therefore never grows a buffer past the cap. The worker keeps its own lines under the cap: it cuts each text field (log messages, warning items and details, failure details) to 64 KiB, after redaction, so a cut can never keep the start of a secret.

**What is implemented of this protocol (M1a, plus the secrets work of M1b):**

- A job is `Fake | Identify | Backup | Prune | Check`. The repository password travels as a plain string on stdin only, and `JobSpec` also carries `cache_dir` and `watchdog_interval_ms`.
- A repository with a hot half (`repo_hot`) is opened through both halves. `options` apply to both, `options_hot` to the hot half only and `options_cold` to the cold half only (rustic's own rule), and the job's repository string is the cold half. rustic reads the hot half's config first and requires the keys of the halves to match, so a missing hot half fails the job at the open. It keeps keys, snapshots, index files and tree packs in the hot half and reads them from there, so a check fails when a snapshot is missing from it; file data is in the cold half only. A `RepositorySpec` that leaves the hot fields out, as an older daemon's would, is refused as `InvalidJobSpec` (`options_hot` and `options_cold` have no default) instead of being opened as a cold-only repository.
- The first line of a worker's stream, once its spec has parsed and the redactor is installed and before it opens anything, is a `Log` at the debug level reading `job accepted`, with nothing of the job in it: it is the proof of life the daemon's start timeout waits for (below). A spec that doesn't parse gets `Failed(InvalidJobSpec)` alone. `Started` is written when the job's clock starts: right before `to_indexed_ids()` for a backup, before `prune_plan` for a prune, before `check` for a check, and after opening the repository for `Identify`. Progress that rustic begins earlier (listing snapshots) never reports, and only `Log` and `Failed` can precede `Started`.
- `ProgressInc` is coalesced on the leading edge: the first goes out at once, the rest are added up and sent at most every 250 ms per key, and the remainder is flushed before `ProgressFinish`. A source rustic could not read (`scan`, `archival`) is a `SourceUnreadable` warning; any other `Progress::error` is `Unknown`.
- `Failed.code` is `deadline-passed` only when the watchdog ended the worker; all else rustic reports is `operation-failed` with `detail` = rustic's one-line message, after redaction. Exit codes: 75 deadline, 143 SIGTERM, 74 events could not be written. The client maps `Failed { DeadlinePassed }`, and EOF with exit code 75, to `DeadlinePassed`.
- Stopping a job sends SIGTERM, waits the grace period (10 s by default), then SIGKILL; `stop()` returns once the process has exited, and the job's events end without a result. Dropping a running job kills the worker at once, but after a result it has 10 s to finish. At the deadline the daemon drops the job, a kill with no grace; it checks the deadline on every event too, so a stream that never pauses can't hold it off. A result stands even when it arrives past the deadline: its snapshot is already written.
- Two timeouts keep a hung worker from holding the daemon. A new worker has one deadline of 30 s (`WorkerEngine::with_start_timeout`), counted from the start of the write of the job to its stdin, to take the job: it ends with the worker's first complete line of any kind, which is normally the `job accepted` log line above. Opening the repository, which a slow backend can make take longer, comes after that line and is not bounded by this timeout. A worker that doesn't read a spec larger than the pipe's buffer (64 KiB on Linux) leaves the write blocked, so `run` kills it with its process group and fails with `EngineError::StartTimedOut`. A worker that took its job and then says nothing is killed with its group by the job's event stream, which ends with a single `Failed(EngineStartTimedOut)`; `run` doesn't wait for it, because that would delay a user's cancel until the first event. Either way the operation fails as `EngineStartTimedOut` without having started. After its first line a worker is not on this clock: how long it takes from there is for the job's deadline and for cancel. A timeout too long to represent, such as `Duration::MAX`, never passes. Once a worker's event stream has ended, the process has 10 s to exit (`with_exit_timeout`): one that stays alive is killed, and the operation fails as `EngineExitTimedOut`. The wait inside `Control::stop()` after SIGKILL has no timeout of its own: its signature can't fail, and a SIGKILL can't be refused short of an uninterruptible sleep.
- Backup chooses the parent with `PlanTags::parent`: explicit `ParentOptions.parents`, or rustic's own match when it returns `None`. Extra tags are `UserTag`s, which never start with the reserved `ladderback-` prefix in any case, so they can't make a snapshot look like another plan's (I6); the worker refuses such a tag in a spec too. Prune always runs with `instant_delete = false` and `early_delete_index = false`, and `keep_delete` is a `KeepDelete`, never under rustic's 23 h default; the worker refuses a shorter one too. Its summary counts `packs_marked = unused + repack`, `packs_deleted = packs_to_delete.remove` and `packs_recovered = packs_to_delete.recover`. Check counts findings by level, and a check that ran to its end succeeds.
- The `Redactor` masks each secret raw, fully percent-encoded with either hex case, and as the url crate encodes a userinfo (rustic quotes URLs that way). A password in the repository URL is masked as written and decoded, and also when a raw `/` in it ends the authority early.
- **Worker environment.** A worker is started with `env_clear()` and only the variables of an allow-list from the daemon's environment (module `engine::environment`): `PATH`, `HOME`, `USER`, `LOGNAME`, `TMPDIR`, `TZ`, `LANG`, `LC_ALL`, `SSH_AUTH_SOCK`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `RCLONE_CONFIG` and the proxy variables `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, each also in lowercase. Names are matched exactly. Everything else, such as `AWS_*`, `AZURE_*`, `GOOGLE_*`, `RUSTIC_*`, `RCLONE_CONFIG_*` and the variable a secret reference was read from, is not passed; a repository that needs a credential gets it through `${…}` in its options (§10.2). A proxy URL with credentials embedded passes as it is, so those credentials stay in the worker's environment (a residual risk the user takes by writing them there; no secret source is added for them), but they are masked wherever the worker's output shows them: the daemon adds the password and the whole userinfo of each of `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` (either case, with or without the scheme; `NO_PROXY` lists hosts, not a URL) to the job's `Redactor`. The base is the daemon's own environment, read at each job, unless `WorkerEngine::with_environment` replaces it; tests and a composition root that has read its environment already use that.
- **Masking by the daemon.** The programs a worker starts (rclone, ssh) write to its stderr without going through its `Redactor`, so the daemon builds a `Redactor` from the job's repository and `repo_hot`, password, the three option sets and secrets, and the proxy credentials above, and masks each stderr line before logging it. A line cut at the cap first loses `longest_match() - 1` bytes of its end, so a secret straddling the cut can't leave its first characters behind. It masks every text a worker's events carry as well: failure details, warnings, progress text and log lines, and the failure it reports for a line it can't read. The worker masks its own, but it rejects a job spec before it has the job's secrets, and a parse error can quote the value it choked on, and it knows nothing of the proxy credentials. A fake job has no secrets of its own and masks only the proxy credentials.
- OpenDAL's local file service is the repository string `opendal:fs` with the option `root`.
- A worker leads a process group of its own, and stop and kill signal the group: SIGTERM, then SIGKILL after the grace period. A worker that exits by itself, at its deadline (75), on a broken event stream (74), on a SIGTERM (143) or by crashing, gets the group a SIGKILL as well, so the `rclone` of an rclone backend, which `process::exit` doesn't stop, ends with the worker. On Linux the daemon sees the exit with `waitid(WNOWAIT)` and signals the group while the worker is an unreaped zombie, which still holds its process id. nix has no `waitid` on macOS, so there the worker is reaped first and the group is signaled by the id it had. POSIX keeps a process id from being reused while a process group of that id has a live member, so the window left is a group with no live member, whose id a new process takes and makes itself leader of between the reaping and the signal. The worker keeps the event pipe on a close-on-exec descriptor of its own and points descriptor 1 at `/dev/null`, so no program it starts inherits the pipe or can write into the stream. Once the worker has exited, the daemon reads what is left in the pipe for one more second at most, so a process the worker left behind that holds the pipe open can't keep the events from ending. On Linux the worker asks for SIGTERM when its parent dies (`PR_SET_PDEATHSIG`); elsewhere it ends at its next event, when the pipe is broken.
- A progress or log callback that finds the event stream unwritable (the daemon is gone) ends the worker at once with code 74, and so does the job's own write failing. A panic is written to stderr with the job's secrets redacted. The worker's stderr is a pipe the daemon reads on a task of its own, never the terminal: a worker in a process group of its own is a background job to a terminal with `stty tostop`, and a write to the terminal would suspend it. Each line, cut at the message cap, goes to the daemon's log as a warning (target `ladderback::worker`), without holding up the events, and the reading ends with the worker like the events' does.
- Integration tests run the worker through `engine-worker`, this crate's own binary, which does what `ladderback worker` does.

### 4.2 Daemon (agent)

- **axum 0.8** HTTP server. It serves the embedded UI, `/api/v1/*` (JSON) and `/api/v1/events` (WebSocket for live updates). It binds to `127.0.0.1:9898` by default. Request checks (§10.1) apply whether or not auth is enabled.
- **Scheduler** (§5). It turns plans into jobs and respects per-agent concurrency limits.
- **Job runner.** It spawns workers, consumes events, writes the op log (queued, running, finished; a write the store refuses never changes how the operation ends, §9) and broadcasts to UI clients. If a hub is configured, it also writes events to the sync outbox.
- **Store.** SQLite via `sqlx`, in WAL mode (§9).
- **Logging.** `serve` installs `env_logger` first, writing one plain line per record to stderr, with millisecond timestamps and colour only on a terminal. The default level is `info`, because env_logger's own would hide the worker's warnings (§4.1); `RUST_LOG` directives layer on top of that default, and an unset, empty or blank value means the default. It logs `ladderback <version> starting` before it binds; the `listening on` line stays a plain `eprintln!`, and `ladderback worker` installs no logger.
- **Config** (§6.1). `config.toml` in the platform config dir, `LADDERBACK_CONFIG_DIR` when set (a non-empty value; tests and a second daemon use it). The platform folders: macOS `~/Library/Application Support/ladderback` for both config and data; Linux `$XDG_CONFIG_HOME|$HOME/.config/ladderback` and `$XDG_DATA_HOME|$HOME/.local/share/ladderback`; `LADDERBACK_DATA_DIR` overrides the data dir, which holds `ladderback.db` and the agent ID. No home directory and no override is a typed exit, not a guess. `serve` reads the config before it binds: a file this build cannot read — unreadable, too large, missing or newer schema, invalid — is a typed startup exit (the catalog's `cli-config-*` messages) that names the file and the version it found; it never starts on a silent empty configuration, and never prints "listening on" first. An empty configuration is fine: an agent with nothing to do still serves.
- **Operations API.** `POST /api/v1/operations` with `{"plan": …}` starts a backup of a named plan and answers `202` with the operation's ID; refusals are typed (`plan-unknown`, `invalid-body`, `config-unavailable`). E2 grows it into the operations page: `GET /api/v1/operations?after=<cursor>&limit=<n>&plan=<id>` answers the keyset history page, newest first, running operations included — the page's initial state over REST, which a client that then subscribes closes the gap a late subscriber has (§4.4); `GET /api/v1/operations/{id}` answers one operation's detail with its warnings; `POST /api/v1/operations/{id}/cancel` takes the empty JSON body `{}` (the JSON-body rule applies to every mutating method) and cancels politely: the worker stops, the finally-hooks run, the operation ends `cancelled`. Cancel's scope is running operations only — nothing is queued today; a finished operation is `operation-not-running` (409), an unknown ID `operation-unknown` (404), a cursor that isn't an operation ID `invalid-cursor` (400), and `limit` is clamped, never refused. Operation IDs are UUIDv7, which is what the log orders by. A second backup of the same plan runs beside the first; I7's slot rule is maintenance-only. The agent, repository and plan hooks (§6.1) wrap every API-started run, read from the configuration as it stands when the run starts.
- **Secrets** (§10.2).
- **Hub client** (§7). It keeps an outbound WebSocket open, with reconnect, backoff and outbox replay.

### 4.3 Hub

Everything an agent has, plus:

- **Agent registry**: agent ID, identity public key, hostname (unique), version, last seen, online status, clock offset.
- **Aggregated op log.** It receives operation records and summaries from agents. Raw logs stay on the agent and are fetched on demand.
- **Plan registry.** Agents publish their plans (minus secrets), so the hub knows the retention policy for each `ladderback-plan` tag.
- **Repo ownership registry.** It maps each rustic repository ID to its single maintenance owner (I1, §7.3).
- **Maintenance runner** for **hub-managed repos** (§7.3): forget, prune and check, serialized per repo by `OperationStore::try_start` (I7).
- **Downloads.** For hub-managed repos, the hub opens the repository with its own secret and streams `dump` output to the browser. No file contents cross the sync protocol.
- **Notifications** (§11).
- **Remote control** (milestone 4b): "run now", cancel, and restore into a new folder on agents that allow it (§10.4). Plans are **view-only from the hub** in v1. Editing them from the hub is a post-v1 feature, opt-in per agent (§16, decision 2).

### 4.4 Web UI (Leptos CSR)

Every page follows §4.5. Pages:

1. **Dashboard.** A card per machine: online status, last backup per plan (time, result, size added), next run, overdue plans and any running operation with live progress. A list of recent failures and warnings.
2. **Machine detail.** Plans, history timeline, logs and an agent settings summary.
3. **Plans.** Create and edit sources, excludes (globs, iglobs, git-ignore, exclude-if-present), tags and label, schedule, retry policy, retention, hooks and repository. Also "Import rustic profile…" and "Export as rustic profile".
4. **Repositories.** Add or connect (local, SFTP, REST, S3, B2, rclone…), test the connection, init, stats (`infos_files`/`infos_index`), and request a maintenance owner and set the maintenance schedule. Shows the repository ID, the granted owner and any rejected claim. Testing opens the repository and reports its ID; a write-permission check is **not yet built** (it belongs with the maintenance-owner work).
5. **Snapshots.** Filter by host, label, tag, plan or path. Snapshot detail shows the summary and a tree browser (`ls`). Untagged snapshots show which plan they would be adopted into (§7.3).
   - **Download** a file or directory (tar/zip, built from `ls` + `dump`). On the hub this uses the hub's own repo access and only works for hub-managed repos.
   - **Restore** follows §10.4: by default into a **new folder** (`~/ladderback-restore/<host>-<snapshot-short-id>-<timestamp>/`). Restoring over the original location is offered only in the agent's own UI, with its own confirmation.
6. **Operations.** A searchable history with full logs, warnings and a cancel button.
7. **Settings.** Auth, hub pairing, notifications.

Live updates come from one WebSocket per tab carrying `UiEvent` (operation started/progress/finished, agent online/offline). The page fetches initial state over REST. The daemon forwards the worker's events to subscribers unchanged: coalescing happens once, in the worker (§4.1), and the daemon never re-coalesces. Progress bars are stateful per key on the way out (`Begin` publishes 0, `Advance` adds, `Length` sets the total; a title or a finished bar publishes nothing), so a page sees current-of-total numbers, not deltas. The events WebSocket answers a client's ping with a pong, so a live page survives an idle network.

### 4.5 Accessibility

Accessibility is designed in from the first screen, not audited in at the end. The target is WCAG 2.2 AA (A1–A6, §0).

**Supported assistive technologies** follow the supported platforms:

- Orca with Firefox on Linux;
- VoiceOver with Safari on macOS;
- keyboard-only use in any current browser. Safari reaches links and buttons with Tab only when "Press Tab to highlight each item" is on, or with Option+Tab; the manual pass uses that setting.

**Structure.**

- **Native HTML first:** `<button>`, `<a>`, `<table>` with `<th scope>`, `<progress>`, `<dialog>` opened with `showModal()`, `<details>`, `<time datetime>`. ARIA only where no native element exists, and then following the WAI-ARIA Authoring Practices pattern for that widget.
- **Page skeleton:** landmarks (`header`, `nav`, `main`), exactly one `<h1>`, descriptive headings and labels with no skipped levels (2.4.6), and a "Skip to content" link. Text in another language, such as rustic's verbatim English messages, carries its own `lang` (3.1.2).
- **Route changes** (path navigation): the router updates `document.title` (page, then machine, then "Ladderback") and moves focus to the new page's `<h1>` (`tabindex="-1"`). Screen readers read the focused heading, so the announcer stays silent. Changing filters, sorting or pages updates the URL query without moving focus.
- **Pointer targets:** at least 24 × 24 CSS px (2.5.8). Nothing needs dragging (2.5.7) or works only on hover.
- **Tooltips and popovers** can be dismissed with Escape, stay open while hovered, and stay until dismissed (1.4.13). The `title` attribute never carries information on its own.
- **Sessions** last 24 hours, above 2.2.1's 20-hour exception. Expiry never discards unsaved form input; the user signs in again and continues.

**The shared component set** in `web` is the only place widgets are built. Each component has an accessibility contract and tests for it:

| Component | Contract |
|---|---|
| Dialog (confirmations, step-up confirmation) | Native modal `<dialog>`. Focus moves in, the background is inert, Escape cancels, and focus returns to the control that opened it. A destructive button is never the default. Errors inside the dialog, such as a wrong password, use a `role="alert"` inside it |
| Data table (snapshots, operations, history) | A real `<table>`. Sortable headers are buttons with `aria-sort`. Row actions are named with their row ("Restore snapshot 3f2a…"). The new page and row count are announced |
| Tree (snapshot browser, M3) | WAI-ARIA tree pattern: one tab stop with roving `tabindex`; arrow keys (Left and Right swap in right-to-left layouts), Home and End, type-ahead; `aria-expanded`, `aria-level`, `aria-setsize` and `aria-posinset` for lazily loaded children; `aria-busy` while a folder loads. Download and Restore for the selected node sit in a details pane next to the tree, reached with Tab. If M3's VoiceOver pass shows the tree is unusable there, the fallback is a nested list of disclosures |
| Status badge | An icon with a distinct shape per state, visible text, and color. Never color alone (A2) |
| Progress | A visual bar (`aria-hidden`) that moves smoothly, plus a focusable `<progress>` whose value and `aria-valuetext` ("42 %, 3.1 GiB of 7.4 GiB, about 6 minutes left") change at most every 30 s. Screen readers can speak value changes themselves, so the exposed value is throttled too |
| Form field | Visible `<label>`; hint and error linked with `aria-describedby`; `aria-invalid` on error; `autocomplete` tokens where a field has a known purpose (1.3.5). On a failed submit, focus moves to an error summary (a heading with `tabindex="-1"`) that names each problem and links to its field. Errors say what is wrong and how to fix it (3.3.1, 3.3.3). Validation never interrupts typing |
| Tabs, menus, disclosures | Their WAI-ARIA patterns, or native `<details>` where it suffices |
| Toast | Spoken through the announcer and never steals focus. Never the only place a failure appears: failures also stay on the page |

**Live updates (A3).** `UiEvent`s update the page visually, but only the **announcer** talks to screen readers.

- **Two regions:** a polite `role="status"` and an assertive `role="alert"`. Both sit in the static `index.html`, outside the Leptos mount point, empty and never `display: none`, so they exist before anything is written to them.
- **Polite:** an operation finished, an agent came back. **Assertive:** only a failure that needs the user now, such as a failed restore.
- **Writing:** each message clears the region and sets the text on the next tick, so a repeated message is announced again.
- **Bursts are merged:** the same message within 2 s is said once, and ten agents changing at once become one summary ("3 backups finished, 1 failed").
- **Modal dialogs** make the rest of the page inert, including the regions. While one is open, announcements queue and are spoken when it closes.
- **Progress** is never written to a region. The start and the finish are announced.
- **Pause:** the dashboard has a "Pause live updates" control (2.2.2). While paused, the page stops changing and a banner counts pending changes.

**Settled in the first build** (`crates/web/src/announcer.rs` for the timing, `live.rs` for the regions):

- **Where it lives:** the timing is pure logic over an injected clock in a module of the `web` binary, so `cargo nextest run -p web` runs its tests on the host; the glue that writes the regions and owns the one timer is `live.rs`. The regions in `index.html` carry `role`, `aria-live` and `aria-atomic="true"`, are visually hidden with a class and never `display: none`, and the app mounts into `#app` beside them.
- **Merge window:** 2 s, from the first polite message, not the last, so a steady trickle cannot postpone the write for ever. The messages in it are joined with a space in arrival order (catalog messages are whole sentences) and a text already in the batch is said once. A page that wants a summary ("3 backups finished, 1 failed") builds it and sends it as one message; the announcer only merges. At most 16 messages are held for one write, so a modal left open cannot grow the queue without bound.
- **Write, clear, then set:** the region is cleared when the window closes and set 50 ms later. A message for a region that is still waiting to be set joins that set. Blank text and progress updates are ignored.
- **Modal dialogs:** opens are counted, so nested modals hold messages until the last one closes. Closing flushes everything held at once, without a new 2 s window, assertive first. A close with no modal open is ignored. A polite message sent inside an open window and held by a modal joins the same batch. A write already waiting for its tick when a modal opens is made after it closes. A page that unmounts a dialog while it is open (a route change) must tell the announcer it closed, because a removed `<dialog>` raises no `close` event.
- **Assertive messages wait for a modal too:** the design's "announcements queue" covers them, and it is the behaviour that works. A modal makes the rest of the page inert, and VoiceOver and JAWS say nothing for a live region in inert content (w3c/aria#1854), so an assertive message let through would be lost, and it is the urgent one. They skip the merge window; two for the same region that arrive within the 50 ms before the first is set, or while a modal is open, are joined into one set. The same limit of 16 holds per region. A failure that happens inside a dialog belongs in a `role="alert"` inside that dialog (the Dialog contract above), which needs no announcer. Moving the regions into the open dialog instead was left out: it needs a design change, and the real Dialog component is the place to revisit it.
- **Route changes:** `document.title` follows the path. An unknown path is the not-found page, with its own `<h1>`, and so is `/_gallery` in a build without the gallery. Each page's `<h1>` takes focus when it is shown after a navigation. The page load itself leaves focus at the top, where the skip link is.
- **The skip link handles its own activation:** its click handler moves focus to `<main>` and stops the click. Left alone, the router takes the click on `#main` and only scrolls to it, so focus stays where it was. The router ignores a click that something else has already stopped, so it needs no option, and the address and the history stay as they were (a `rel="external"` on the link, tried first, worked too, but called an in-page link external and left a `/#main` entry that Back had to step over).
- **The gallery** is the `gallery` feature of `web`: off by default and off in a release build, which `just architecture` checks. `just web-test` builds the UI with it, and `just test-ui` runs the scenarios against that build.

**Settled in the component build** (`crates/web/src/components.rs`, exercised by the gallery):

- **The first six components** are `ModalDialog` (native `<dialog>` + `showModal()`, focus wrapped from the last tab stop to the first and Shift-Tab from the first to the last by a keydown handler on the dialog, Escape and the Close button both closing, focus returning to the invoker, and opens counted by the announcer), `SortableTable` (a real `<table>` with a `<caption>`, `scope`d headers and a sort button whose result is announced politely), `FormField` (a `<label>`, a hint and an error wired through `aria-describedby`, `aria-invalid` on error), `StatusBadge` (visible text on a colored pill, never color alone), `Progress` (`role="progressbar"` with `aria-valuenow`/`aria-valuemin`/`aria-valuemax`, never written to a live region) and `Toast` (rendered through the announcer, polite; it disappears without an announcement). Tabs, menus, disclosures and the tree stay for later milestones. Every gallery demo shows its loading, empty, error and success states where they apply, and the demo strings come from `ui.ftl`.
- **Badge tokens:** `style.css` carries `--badge-{loading,empty,error,success}-{fg,bg}` for both color schemes, each pill its text token on its background token. The weakest pair is error at 9.9:1; every pair clears 4.5:1 in both schemes.
- **Tailwind v4** rides trunk's pipeline: a `pre_build` hook in `crates/web/Trunk.toml` runs `@tailwindcss/cli` (pinned exactly at 4.1.0 — newer 4.1.x resolve an optional `oxide-wasm32-wasi` tarball this npm policy refuses) over `tailwind.css`, whose `@theme inline` maps the tokens of `style.css` as Tailwind colors, theme and utilities only, no preflight, so `style.css` stays the base stylesheet and the tokens stay the single source of truth. Its output `tw-out.css` is committed so a build works without `npm` (the hook only installs when `node_modules` is missing), and `index.html` links both.
- **The contrast gate** (A4) is `just test-contrast`: `scripts/check-contrast.mjs` reads the token pairs named in it from `style.css` — text and muted text on surface (4.5:1), the focus ring on surface (3:1) and the eight badge pairs (4.5:1), in both light and dark — and fails any pair under WCAG 2.2 AA. It needs only Node, so it is not in `just verify`; CI takes it in a later PR.

**Time, numbers and charts.**

- Relative times ("3 h ago") are always paired with the absolute time, both in `<time datetime>` and in the accessible name. Text inside a live region never ticks.
- Sizes and durations come from `l10n` (§8.4). Where an abbreviation reads badly aloud, the visible text is `aria-hidden` and a visually hidden spoken form, from the catalog's `.spoken` attribute, sits next to it. An accessible name always contains the visible label text (2.5.3).
- SVG charts (§8.1) have `role="img"`, a title and a one-sentence summary ("Snapshot size grew from 41 to 44 GiB over 30 days"). A "Show as table" toggle exposes the same numbers as a real table.

**Visual adaptation.**

- Light and dark themes follow `prefers-color-scheme`, with a manual override.
- `prefers-contrast: more` strengthens borders and focus rings, and `forced-colors: active` falls back to system colors without losing state.
- `prefers-reduced-motion` stops every animation except the progress bar's value. Nothing flashes.
- Text resizes to 200 % without loss, and pages reflow at 320 × 256 CSS px (A4).
- Color tokens live in one CSS file, and a test checks every foreground and background pair against A4's ratios. Once the visual `DESIGN.md` exists, `design.md lint` runs the same check on its tokens.

**Safe actions and authentication (A5).**

- Restore, in-place restore, adoption, forget and prune say what will happen before it happens and can be cancelled (3.3.4).
- Step-up confirmation (§10.4) allows paste and password managers (`autocomplete="current-password"`). The "type the snapshot short ID" check used when auth is off accepts paste, and the ID is shown as selectable text with a copy button, so it isn't a transcription test (3.3.8).
- A multi-step flow never asks again for something already entered (3.3.7).

**Beyond the web UI.**

- **Notifications:** every message has a plain-text form (email has a text part). Meaning never depends on color or emoji, and every message says what happened and what to do next.
- **CLI:** color is never the only signal, and `NO_COLOR` is honored. Spinners and redraws are off when output isn't a TTY, or with `--plain`. Every command that reports state has `--json`.

**Tests** (test first, like everything else; §13):

- **Component structure** (roles, names, ARIA attributes, DOM) runs in the browser under `wasm-bindgen-test`.
- **Component behavior** (keyboard, focus return, Escape, accessibility-tree snapshots with `toMatchAriaSnapshot`) runs in Playwright, which sends real input, against a dev-only component gallery route. Synthetic key events in `wasm-bindgen-test` don't trigger native behavior, so they can't test this.
- **Announcer:** throttling, merging, politeness, queueing during dialogs, and "the same text twice, 5 s apart, is announced twice", all with a fake clock.
- **UI journeys** (`@ui` scenarios, §13.2), from M0:
  - axe-core with tags `wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa` and `wcag22aa` on every page, in its loading, empty, error and success states;
  - keyboard-only runs through a helper that offers only key presses and role locators; a lint bans pointer actions in journey code;
  - after every Tab, the focused element must have a visible focus indicator, and `elementFromPoint` at its center must be that element or inside it (2.4.7, 2.4.11);
  - runs at 320 × 256, with a 200 % root font size, with the 1.4.12 text-spacing stylesheet injected (failing on clipped text, as in §8.4), and under emulated dark, `forced-colors`, `prefers-contrast: more` and `prefers-reduced-motion`.
- **Other surfaces:** a catalog test that every notification message has a plain-text form; a CLI `@e2e` test that output has no ANSI codes under `NO_COLOR` or without a TTY, and that `--json` parses.
- **Manual screen-reader pass** in each milestone with UI: the milestone's journeys in Orca and in VoiceOver, including whether progress bars over-announce. Automated checks catch only part of WCAG, so this pass is part of the milestone's definition of done.

### 4.6 MCP server

`ladderback mcp` lets AI agents and scripts drive Ladderback through the [Model Context Protocol](https://modelcontextprotocol.io): newline-delimited JSON-RPC 2.0 over stdio. Stdout carries only protocol messages; logs go to stderr.

**Protocol.** MCP revision `2025-06-18` or later, negotiated in `initialize`. Older revisions are refused, because results rely on `structuredContent` and `outputSchema`, which arrived in that revision. Implemented:

- **Lifecycle:** `initialize`, then `notifications/initialized`, `ping`, and shutdown on stdin EOF.
- **Tools:** `tools/list` and `tools/call`.
- **Progress and cancellation:** progress notifications, sent only when a request carries `_meta.progressToken`; `notifications/cancelled` for a pending call.
- **Errors:** an unknown tool or malformed arguments is JSON-RPC error `-32602`. A failure inside a tool is a result with `isError: true` and the same `{code, args}` payload the API returns (§8.4), never a sentence.
- **Not localized:** tool `title`s and `description`s are fixed English, like API field names (§8.4).

**Placement.** `adapters/mcp` is an inbound adapter beside `http`, with no rule of its own. Its tools call one `Backend` trait defined inside the adapter, with two implementations:

- **In-process:** receives use-case handles from `src/main.rs`.
- **Connect:** an HTTP and WebSocket client that depends only on `api`, never on `adapters/http` (§3.2).

A parity contract suite runs every tool against both implementations, so the two modes can't drift.

| Mode | Command | What it drives |
|---|---|---|
| **Connect** | `ladderback mcp --connect [URL] [--token-file PATH]` | A running agent, through `/api/v1` and its events WebSocket, like any other API client. The URL defaults to `http://127.0.0.1:9898` and must be `https` or loopback before a token is sent. The server starts even when the daemon isn't up yet, and connects on the first call. A request whose reply was lost is never resent: the tool reports that the outcome is unknown |
| **Headless** | `ladderback mcp --headless [--config DIR] [--data DIR]` | An in-process agent session wired by the composition root, as `serve` does but without HTTP or the scheduler. It takes the data-dir lock (§9), so a data dir that `serve` has open fails with `data-dir-in-use` and connect mode is the answer. Operations run only while the session lives: on stdin EOF or SIGTERM it cancels its queued operations and stops its workers (SIGTERM, then SIGKILL), so a later `serve` finds no stale queue |
| **Scratch** | `ladderback mcp --scratch` | A headless session in a new mode-0700 temporary directory (`ladderback-scratch-*`) holding config, data and a local repository, seeded with one plan over procedurally generated files. Deleted on exit; stale scratch directories from crashed sessions are swept at the next start. For demos and for agents testing Ladderback itself |

**Tools.** Each maps onto one use case or read-side query. Results are `structuredContent` built from `api` types, declared by each tool's `outputSchema` and mirrored as JSON text. Every `list_*` tool takes `limit` and a cursor, and results have a size cap, so one call can't flood the client's context.

| Tool | Does |
|---|---|
| `status` | Plans, last result and next run per plan, running operations |
| `list_plans`, `list_repositories` | Plans and repositories, with secrets as references only (§10.5) |
| `list_snapshots` | Snapshots, filtered by host, plan, tag, path or time |
| `list_operations`, `get_operation` | History, one operation's status, summary and warnings |
| `operation_log` | One operation's redacted log, paged |
| `run_plan` | Queues a backup now, under the plan's overlap policy, with `trigger = mcp`; returns the operation ID. The plan's existing hooks and secret sources run as they would for any other trigger |
| `wait_operation` | Waits until an operation finishes or a timeout passes, with progress notifications. Cancelling the call stops the wait, not the operation |
| `cancel_operation` | Cancels a queued or running backup started by `run_plan` or a schedule. Maintenance operations can't be cancelled from MCP, so a loop can't keep a repository from being pruned |
| `test_repository` | Tests a configured repository's connection |
| `preview_forget` | The dry run of forget for a repository this agent owns: what would be deleted, per group (§7.3) |
| `preview_import` | Takes a rustic profile's TOML as `content`, never a path, and returns the proposed plans and notices without applying them. `use-profiles` is reported as a notice rather than read from disk, and parse errors are redacted |
| `export_profile` | A plan as a rustic profile (§6.2). Fails with `inline-secret` when the repository's password is stored inline, rather than writing it out |

There is deliberately no tool that restores, downloads, applies forget or prune, adopts, edits config or pairs: each needs a human confirmation or could change what commands run (§10.5). They can be added later only with a confirmation that MCP can carry honestly.

**Hub.** M3 drives one agent. Connect mode against a hub, with an `agent` argument on the tools that need one (plan IDs are unique per agent only), arrives with remote control in M4b.

**Testing.** `features/automation/mcp.feature` runs as `@e2e` scenarios. The runner speaks JSON-RPC to `ladderback mcp --scratch`, and to `ladderback mcp --connect` against `ladderback serve --listen 127.0.0.1:0`, reading the chosen port from the `listening on` line on its stderr. Each tool's input and output schemas are checked against example calls. The I4 canary test (§10.2) scans every tool result, including `list_repositories`, `export_profile` and `preview_import` with inline secrets.
