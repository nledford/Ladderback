## 2. Background: what we build on

### 2.1 `rustic_core` (library, Apache-2.0 OR MIT)

Ladderback pins `rustic_core` and `rustic_backend` to **one git revision of `rustic-rs/rustic_core` `main`**, because the per-file error reporting it needs (`Progress::error`, `SnapshotSummary.error_count`) is not in the latest release, 0.13.0. It moves to crates.io as soon as a release (expected 0.14) contains them. The table was checked against `main` at `01fe284` (2026-11-05):

| Need | API |
|---|---|
| Open repo | `Repository::new_with_progress(&RepositoryOptions, &RepositoryBackends, pb)?.open(&Credentials)?` |
| Repo identity | `repo.config().id`: a random ID created at `init`, the same on every machine |
| Backends | `rustic_backend::BackendOptions { repository, repo_hot, options, options_hot, options_cold }.to_backends()` (local, REST, rclone, OpenDAL: S3, B2, SFTP, …) |
| Backup | `repo.to_indexed_ids()?.backup(&BackupOptions, &PathList, SnapshotFile)` |
| Snapshots | `get_all_snapshots()`, `Grouped::from_items(snapshots, SnapshotGroupCriterion)`, `delete_snapshots(&[SnapshotId])` |
| Retention | `KeepOptions` (all `keep-*` and `keep-within-*`, incl. quarter/half-yearly) and `ForgetGroups::from_grouped_snapshots_with_retention(grouped, &KeepOptions, &Zoned)` |
| Prune | `prune_plan(&PruneOptions)` then `prune(&opts, plan)` |
| Check | `check(CheckOptions) -> CheckResults` |
| Browse / restore | `node_from_snapshot_path("SNAP[:PATH]", filter)`, `ls(&Node, &LsOptions)`, `prepare_restore` and `restore`, `dump(&Node, &mut impl Write)` |
| Stats | `infos_files()`, `infos_index()` |
| Progress | `trait ProgressBars { fn progress(ProgressType, prefix) -> Progress }`. A progress object has `set_length`/`inc`/`set_title`/`finish`/`error(item, during, message)` (`error` is on `main` only) |

Facts that shape the design:

- **The API is synchronous and uses rayon internally.** Nothing in it is `async`.
- **The OpenDAL and REST backends (and so rclone) each create their own Tokio runtime** and call `block_on` on it. Calling them from inside a Tokio task panics with "Cannot start a runtime from within a runtime". `spawn_blocking` avoids the panic; the worker process (§4.1) avoids it too, and exists mainly for the other reasons listed there.
- **There is no cancellation API.** A running backup or prune cannot be stopped cooperatively.
- **`dump` handles files only.** A directory download is built by walking `ls` and dumping each file into a tar or zip stream. `dump` and `prepare_restore` need `to_indexed()`, which holds the full index in memory.
- **Most option structs are `serde`-deserializable with `kebab-case` and `deny_unknown_fields`**: `KeepOptions`, `Excludes` (`globs`, `iglobs`, `glob-files`, `iglob-files`), `LocalSourceFilterOptions` (`git-ignore`, `exclude-if-present`, …), `SnapshotOptions`, `ParentOptions`, `BackupOptions`, `BackendOptions`, `CredentialOptions`. They match the TOML keys rustic profiles use. **`PruneOptions`, `CheckOptions`, `RestoreOptions` and `LsOptions` have no serde support**; Ladderback defines its own config structs for them and maps them onto the core types. `CheckResults` isn't `Serialize` either, so the worker reports a mirror type.
- **`deny_unknown_fields` with `#[serde(flatten)]`** works for flat types: `Excludes`, `LocalSourceFilterOptions`, `SnapshotOptions` and `KeepOptions` flattened together still reject unknown keys. Flattening `BackupOptions`, which has flattens of its own, next to other strict types is expected to fail (serde's documented limitation; not tested). See §6.1.
- **The default snapshot grouping is `host,label,paths`**. `keep-within-*` is measured from the newest snapshot in each group, not from now, so a machine that stops backing up keeps its last snapshots.
- **`rustic_core` does not build for `wasm32`** (`errno`, `getrandom`, `xattr`). The UI cannot share `rustic_core` types (§8.3).
- **`rustic_config` is empty.** The profile structs (`[repository]`, `[backup]`, `[[backup.snapshots]]`, `[forget]`) live in the `rustic` CLI crate (`rustic-rs`). It is on crates.io, but the fields Ladderback needs are private, so we mirror that layout ourselves with Ladderback-owned types that the engine adapter maps onto the core option types (§3.2, §6.1).

### 2.2 Why lock-free matters

`rustic prune` does not delete unreferenced pack files right away. It records them in a `packs_to_delete` list in a new index file, stamped with the time the prune plan was made, and only deletes them on a **later** prune once that stamp is at least `keep-delete` old (rustic's default is **23 h**; Ladderback defaults to **6 days**, §7.3). If a snapshot written in the meantime uses a marked pack, the next prune recovers it.

A backup never uses blobs from packs that are already marked: it loads only the live pack list. The race is a backup that **loaded the index before a prune marked the pack** and is still running when a later prune deletes it. The old index files are only removed at the end of the marking prune, so a backup that loads the index while that prune runs is also exposed. The safe condition is therefore:

> every backup finishes within `keep-delete` − the duration of the prune that marked the pack.

Ladderback enforces this with a worker deadline (I3, §7.3). Two rules are absolute: never `instant-delete`, never `early-delete-index` (I2). rustic's FAQ also notes that concurrent forget/prune runs are untested and that `check` can trip over snapshots a concurrent forget just removed, so maintenance on one repo is serialized (I7).

### 2.3 Leptos

- Stable **0.8.23**. **0.9.0-rc1** is on `main`. Target 0.8, and move to 0.9 once it is stable.
- We use **client-side rendering (CSR)**, built with `trunk` to WASM. The build output is embedded in the daemon binary (`rust-embed`) and served by axum. Rationale in §8.1.
