# Decision 82: How a page learns which step a running operation is in

**The engine adapter maps each of rustic's progress texts that it knows to a step, a typed code (`OperationStep`), and the text goes no further; the daemon tells a page of the bar the engine began last and still has open, with its step, in `operation-progress`'s `step` field; the page says the step through one catalog message per code, and says a step it doesn't know, or none, as working** (recorded 2026-11-12, §3.1, §4.1, §4.4, §4.5; #423). The Operations page said only "Running" while a first backup read the index for minutes, and drew the index's counter as the backup's percentage: `progress_ui` folded every bar into one unnamed event and dropped rustic's names for them.

Decided:

1. **The steps, and rustic's texts for them.** At the pinned `rustic_core` (the revision in `Cargo.lock`; places are in `crates/core/src/`):

   | Step (wire code) | rustic's text | Where | Ladderback's jobs |
   |---|---|---|---|
   | `reading-index` | `reading index...` | `index.rs:269` (a title on the counter of `repository.rs:1110`/`1170`), `commands/check.rs:459`, `commands/prune.rs:707` | backup, check, prune |
   | `getting-snapshots` | `getting snapshots...`, `getting snapshot...`, `reading snapshots...`, `getting latest snapshot...`, `getting latest~N snapshot...` | `repository.rs:830, 874, 944, 801, 1518`, `commands/prune.rs:1590`, `repofile/snapshotfile.rs:806, 808` | backup (its parent), check, prune, adoption |
   | `backing-up` | `backing up...` | `commands/backup.rs:287` | backup |
   | `listing-packs` | `listing packs...`, `listing packs in hot repo...`, `getting packs from repository...` | `commands/check.rs:512, 507`, `commands/prune.rs:729` | check, prune |
   | `checking-cache` | `checking Snapshot in cache...`, `checking Index in cache...`, `cleaning up packs from cache...`, `checking packs in cache...` | `commands/check.rs:245, 261, 276` | check |
   | `checking-hot-repository` | `checking Snapshot in hot repo...`, `checking Index in hot repo...` | `commands/check.rs:253` | check |
   | `checking-trees` | `checking trees...` | `commands/check.rs:628` | check |
   | `reading-pack-data` | `reading pack data...` | `commands/check.rs:301` | check with read-data |
   | `finding-used-data` | `finding used blobs...` | `commands/prune.rs:1608` | prune |
   | `marking-unused-packs` | `marking unneeded unindexed pack files for deletion...` | `commands/prune.rs:1238` | prune |
   | `repacking` | `repacking...` | `commands/prune.rs:1359` | prune |
   | `rebuilding-index` | `rebuilding index...` | `commands/prune.rs:1294` | prune |
   | `deleting-unused-data` | `removing unindexed packs...`, `removing old index files...`, `removing old data packs...`, `removing old tree packs...` | `commands/prune.rs:1234, 1275, 1437, 1442, 1447` | prune |
   | `deleting-snapshots` | `removing snapshots...` | `repository.rs:994` | forget, adoption |
   | `saving-snapshots` | `saving snapshots...` | `repository.rs:1012` | adoption |

   Identify, init, the snapshot listing and a prune's plan are no operations, and a page is told nothing of them (#308). A bar rustic begins before the worker's `Started` (the backup's own snapshot listing) never reports (§4.1). Saving a backup's snapshot is no step: rustic has no bar for it. Ladderback's own phases (hooks, the queue) are not the engine's and are out of this decision.
2. **The anti-corruption layer.** `crates/adapters/engine/src/step.rs` holds the table; the daemon's `translate` matches the text whole against it, as the worker sent it: masked (§4.1), which the daemon can't undo. `ProgressEvent::Begin` and `Title` carry `Option<OperationStep>` and no text, so rustic's English never reaches `app`, `api` or the page. What keeps a secret off the page (I4) is that only a code from a fixed table leaves the adapter: a text that carries a secret equals no entry, and nothing of it goes on. A text a mask changed names no step either (see Accepted). An unknown text, a newer rustic's, or the empty name of a counter rustic names later, is no step. The worker's protocol is unchanged.
3. **Which bar a page sees.** `app::progress::LiveProgress` keeps a run's open bars by key in the order they began, at most 64 (rustic leaves some open for good, such as the counter a first backup passes for its parent at `commands/backup.rs:109`: `SnapshotFile::latest` finds no snapshot and returns its error at `repofile/snapshotfile.rs:811`, before it finishes the counter at `:813`; the oldest goes). A page is shown the bar begun last that is still open: its `Begin` (at 0, no total), `Length`, `Advance` and `Title` each tell the page; a change to an older open bar tells nothing; `Finish` removes the bar and tells nothing, so the page keeps what it saw until the next bar begins or moves. A title names the bar's step and no longer removes the bar, which lost the index counter's length. The index's count is now shown as the index step's, and backing up shows no percentage until rustic has walked the source (#422).
4. **The wire.** `operation-progress` gains `step`: the code, or `null` for none. It reads as `null` when missing (`#[serde(default)]`), and a code the page doesn't know reads as `Unknown` (`#[serde(other)]`, as `WarningCode`), which the page shows as none. A page built before the field ignores it. A new code is a compatible change.
5. **The page.** The Status cell of a running row: with no percentage, the step's message stands beside the bar in place of `ui-progress-working`; with one, the step's message stands above the bar and the percentage beside it. Messages are `ui-step-<code>`, with the glossary words as Fluent terms (`-index`, `-snapshot`, `-pack`, `-repository`). Progress is never written to a live region (§4.5).
6. **For #428.** The step stays a code it can put in the bar's value text; the unit of a bar (bytes or a count) can join the same event as a field when it needs one.

Accepted:

- **A rustic upgrade that rewords a text** shows that step as working until the table is updated. The engine tests that run a real backup, check with read-data and prune (`crates/adapters/engine/tests/jobs.rs`) fail on such an upgrade for the steps they reach: reading the index, getting the snapshots, backing up, listing packs, checking trees, reading pack data and finding used data. The others (the cache and hot-repository checks, a prune's marking, repacking, rebuilding and deleting, forget's and adoption's steps) are checked by hand against the places in the table when `rustic_core` is upgraded.
- **A secret that is a piece of one of the fixed texts** turns that text's step into working, because the mask changed the text (a password `data` in `reading pack data...`). A reader watching the run learns only that the secret is a piece of fixed English, which only a weak secret can be. Matching before the mask would need the worker to map, which is rejected below.

Rejected:

- **Forwarding rustic's text, masked:** a library's English on the page (§8.4), and a channel for secrets to leak through (I4).
- **Matching a prefix or a pattern:** a masked or longer text could still name a step; only a whole known text does.
- **A step event of its own:** the bus drops progress oldest-first (§4.4), and a lost step event would leave a row naming the wrong step.
- **Showing every open bar:** rustic leaves bars open for good, and they would show for the rest of the run.
- **Mapping in the worker:** it changes the worker's protocol, and the daemon still has to mask what it is sent.
