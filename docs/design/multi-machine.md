## 7. Multiple computers

### 7.1 Pairing and identity

1. On the hub: **Settings → Agents → New pairing token**. The token is single-use, with a TTL of 15 min by default.
2. On the agent: `ladderback pair https://hub:9899 <token>` (or the same in the agent's UI).
3. The agent generates an **Ed25519 keypair** and an **agent ID** (UUIDv7) once, and keeps both in its data dir, never in the database, so `db rebuild` can't change them. A standalone agent has them too, because its backups are tagged with the ID (§7.3). At pairing it sends its agent ID, public key, hostname and the token; the hub records the agent and discards the token. Re-pairing the same identity reuses the existing record. A hostname already registered to another agent is rejected, because the UI, the `host` filter and the `agents` table all identify machines by hostname.
4. Every later connection is a WebSocket over TLS. The hub sends a nonce, the agent signs it, and the hub checks the signature against the registered key. To revoke an agent, delete it on the hub.

Hub TLS can be either of these:
- **Built-in.** A self-signed cert whose SHA-256 fingerprint is pinned during pairing (shown in the hub UI and included in the token string).
- **External.** A reverse proxy such as Caddy or Traefik, using normal CA validation. Only `/sync/v1` needs to be reachable by agents. Keep `/` and `/api` on a trusted network.

Both modes serve UI, API and `/sync/v1` on one listener; the hub's default port is 9899.

### 7.2 Sync protocol (`/sync/v1`, WebSocket, JSON, versioned)

Agent → hub:
- `Hello { agent_version, hostname, protocol: 1, clock: Timestamp }`. The hub records the clock offset and warns above 5 min.
- `PlansSnapshot { plans: [PlanPublic] }`, sent on connect and on change. Secrets are never included.
- `ReposSnapshot { repos: [{ repo_id, alias, requested_maintenance }] }`, sent on connect and on change.
- `OpEvent { epoch, seq, op_id, event }`. `epoch` is a random ID created with the agent's outbox; `seq` increases within an epoch and is assigned when the event enters the persistent outbox, so the agent replays from the hub's last acknowledged `(epoch, seq)` after a disconnect. A rebuilt database starts a new epoch, so its events are never mistaken for already-acknowledged ones. Progress events are sent live but never stored or replayed.
- `Heartbeat { running_ops: [{ op_id, repo_id, kind, index_read_at, deadline }], next_runs }`, every 30 s.
- `RestoreResult { request_id, op_id | error }`

Hub → agent:
- `Ack { epoch, seq }`
- `RepoGrants { grants: [{ repo_id, owner, epoch, expires_at, keep_delete, prune_margin }] }`: the owner lease and the deadline inputs for each repo (§7.3). Sent on connect and renewed with each heartbeat reply. The agent caches them.
- `RunPlan { plan_id }`, `CancelOp { op_id }` (milestone 4b)
- `RestoreRequest { request_id, repo_id, snapshot_id, paths, requested_by }` (milestone 4b). Always restores into a new folder. Refused unless the agent sets `allow-hub-restore = true` (§10.4).
- `FetchLog { op_id }` → agent replies `LogChunk`
- `UpdatePlan { plan }`: **reserved for post-v1.** Opt-in per agent ("allow hub to edit my plans"). The message type is reserved in protocol v1 so that adding it later isn't a breaking change

The browser never talks to agents directly. When looking at a remote machine, the hub UI calls hub APIs, and the hub forwards requests like "run now" or "fetch log" over the agent's socket.

### 7.3 Repository maintenance: exactly one owner

**Repository identity.** A repository is identified by its rustic repository ID (`repo.config().id`), read when the repo is connected. The `[repositories.<name>]` key is only a local alias, and different machines may reach the same repository through different `repository` strings (an SFTP URL on the laptop, a local path on the NAS).

**One alias per repository** (decision 49). An agent's configuration names a repository once, so its plans have one history and one maintenance request. Connecting or initialising records the ID Ladderback read as `repository-id` in the entry (§6.1); an entry written by hand or imported from a profile has none, since Ladderback never opened it, and only an identical `repository` string catches a second alias for it. Two entries at one `repository` string, or with one `repository-id`, are one repository, and `AgentConfig::new` refuses the configuration (`SameRepository`, at the second entry's key). Connecting or initialising under a new alias a repository the configuration already holds, at the same location or with the same ID, is refused with `repository-already-configured` (HTTP 409, naming the alias it has); an alias already taken is refused first, with `repository-alias-taken`. A location written another way (an SFTP URL beside a mounted path) is caught only when both entries know their ID. A job opens the repository and, when the entry has an ID, compares it before it reads or writes anything: a location that now holds another repository fails the operation with `repository-replaced`.

**Owner.** Each repository has one maintenance owner (I1):

- `hub`: the hub runs forget, prune and check. Required for repos several agents use.
- `agent:<id>`: one specific agent runs them. Only for repos no other agent uses.
- `none`: nothing in Ladderback prunes. Use this if an existing cron job keeps doing it during migration. Set explicitly on the hub's repo page, or by a standalone agent's config.

**Requests and grants.** An agent's `maintenance` setting is a **request**: `local` asks for `agent:<self>`, `hub` asks for `hub`, and `none` makes no claim; a repository with no `maintenance` asks for `none`. Which request a saved repository gets (decision 48): one created under a name asks for `local`, since nothing else can know it yet; one connected asks for `local` only when the person turns on **Maintain this repository from this agent**, and for nothing otherwise, since another agent or the hub may already maintain it; an imported profile asks for `local` only when its `[forget]` has `prune = true` (§6.2). Changing the request of a saved repository is an edit of `config.toml`. With a hub configured, the agent reports it in `ReposSnapshot`, and the hub decides:

- The first claim for a repository ID is granted; conflicting claims are rejected and shown on both machines (`repo.claim_rejected`, §11).
- `agent:<id>` is granted only while no other agent reports the repo and the repo holds no other agent's tagged snapshots. When a second agent later reports it, the hub stops renewing that lease and raises an alert asking the user to configure the repo on the hub. Until then nobody prunes, which is the safe direction.
- `hub` is granted only once the hub has its own repository entry and secret for that repository ID. Until then the claim shows "pending: not configured on hub", with an alert, and nobody prunes.

A grant is a **lease** with an `epoch` and an `expires_at` (7 days, renewed with every heartbeat). An agent runs scheduled maintenance only while it holds an unexpired lease, and records the epoch on each maintenance operation. To move ownership, the hub stops renewing the old lease and grants the new owner only after the old owner acknowledges the release or the lease expires, so two owners never overlap even when the old owner is offline or deleted.

Without a hub, `local` runs directly; the agent then warns if the repository contains snapshots from other hosts, because a second standalone owner can't be detected. `hub` and `none` run nothing on this agent: a forget, prune, check or adoption of such a repository, and the preview of one, is refused with `maintenance-not-owner` before the repository is opened, and the Snapshots page shows its history with no maintenance buttons and a sentence that says why and how to ask for `local`. The repository list's `maintained-here` tells the page which is which (§4.2).

**Snapshot ownership.** Every Ladderback backup carries the tags `ladderback-plan:<plan-id>` and `ladderback-agent:<agent-id>` alongside the user's tags, label and host. The host is the plan's snapshot host (§3.1): the plan's `host`, else the agent's `hostname`, else the machine's own name, which rustic chooses (`Plan::snapshot_host`). Plan IDs must be unique within an agent's config; the importer adds a suffix on slug collisions. A backup's parent is the newest snapshot with the same two tags, passed explicitly in `ParentOptions`; when none exists yet (the first backup, or history imported from plain rustic), it falls back to rustic's default choice of the newest snapshot with the same host, label and paths, so a plan that names the host its imported history records continues that history. Changing a plan's sources, label or the machine's hostname therefore keeps its history attached to the plan.

**Forget algorithm (owner):**

1. Load all snapshots. Group tagged snapshots by `(ladderback-agent, ladderback-plan)`.
2. For each group, apply the matching published plan's `KeepOptions`. A tagged group whose plan or agent no longer exists is left alone and listed as "orphaned"; the user can attach a policy, delete it explicitly, or **Adopt** it into another plan (for example after a reinstall that created a new agent ID).
3. **Untagged snapshots are never touched by default** (I6). They are grouped by the repo's `group-by` (default `host,label,paths`) and listed as "unmanaged groups". When a group's host, label and paths equal a plan's host, label and sources (after the same normalization rustic applies when it records paths), the UI offers **Adopt**: the owner rewrites those snapshot files with the plan's tags (new snapshot IDs, same trees). Examples of unmanaged groups are snapshots from an old cron job or a retired machine, and history imported from plain rustic.
4. **Preview first.** A forget and an adoption are previewed before they run: the owner evaluates the selection a run would make (steps 1 to 3) and says which snapshots of each plan the policy lets go, which it rewrites, and what is left alone (I6), without taking the maintenance slot or writing to the log. A person confirms from that review, and the start carries the preview's `selection`, a fingerprint of the exact snapshot IDs: a repository that changed in between (another backup, another machine's forget, an edited policy) refuses the run with `maintenance-selection-changed`, and the run is previewed again, rather than removing what nobody reviewed. A prune is previewed with the engine's own dry-run plan (the packs it would mark, delete and bring back, and the `keep-delete` wait before anything marked is deleted, I2), a check by whether it reads data. The optional approval threshold (more than N% of a group) is not built: every forget is confirmed by a person who saw its preview (§16, decision 44).

**Prune** runs after forget, on its own schedule (default weekly), with `keep-delete` ≥ 23 h (default **6 days**) and `max-unused` (default 5%). `instant-delete` and `early-delete-index` are never offered (I2). The default is just under the prune interval: a prune only deletes packs marked at least `keep-delete` earlier, so with a 7-day `keep-delete` a weekly prune that starts a few seconds early would skip last week's marks and reclaim space only after 14 days.

**Long-backup guard (I3).** Three layers:

1. **Worker deadline.** Every backup gets `deadline = index_read_at + keep_delete − prune_margin`, where `index_read_at` is taken just before the worker starts reading the index and `prune_margin` = 1 h + the longest prune duration recorded for that repo. The deadline is measured in wall-clock time, so time asleep counts. The worker's watchdog and the daemon enforce it (§4.1). An agent that has never received `RepoGrants` for a repo assumes rustic's 23 h `keep-delete`, which can only make the deadline earlier. The owner refuses to prune, and alerts, if `keep_delete` < `prune_margin` + 12 h, since backups would then get less than 12 h.
2. **Owner prune guard.** Before pruning, the owner waits for every unfinished backup on that repo that started reading the index before the previous prune **finished** (a backup that reads the index while a prune runs still sees the old index), as seen in heartbeats (online or offline agents) or, for an agent owner, its own running jobs. It waits until each one reports a result or passes its deadline. Only those backups can reference packs this prune may delete, so newer backups never delay it and frequent backups can't starve it. This layer is advisory; layer 1 is the guarantee.
3. **Duration warnings.** The owner warns when a plan's historical duration gets close to its deadline, and for a first backup that may take longer than `keep-delete` the UI suggests raising it until the backup finishes.

Backups made outside Ladderback (the rustic or restic CLI) are not covered by the guard.

**Check** runs weekly by default, with an optional `read-data-subset` such as `5%` for a rolling data check. Forget, prune and check on one repository are serialized (I7).

**Restic warning (I5).** If a restic client must still write to the repo, only its **backups** are safe: they take a non-exclusive lock, which rustic ignores, and restic simply doesn't see packs in `packs_to_delete`. **restic `prune`, `forget`, `rewrite` and `repair` must never run** on a repo Ladderback maintains. `restic prune` deletes every pack it doesn't index, including the ones rustic marked for delayed deletion, and leaves stale marks behind. `restic check` is safe but reports rustic's marked packs as "additional files" and suggests running `restic prune`; the repo page warns about exactly that suggestion.
