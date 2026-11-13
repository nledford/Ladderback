# Decision 86: Trying a repository's locations at the same time

**When a job's location is chosen (decision 80, point 5), every location whose rule holds and that makes the server prove who it is before it is sent a credential, or sends none, is probed at once, at the start of the choice: an `https` REST location whose certificate is checked, or a local path. Every other location, a plain-HTTP one with credentials above all, is probed only in its turn, as before. The order still decides: the first location in order whose probe finds the repository is used, and a later location that answers sooner waits for every earlier one to answer or to fail. A probe that loses is stopped, and the job opens the chosen location again in a worker of its own** (recorded 2026-11-13, §5, §6.1, §10.6; #455, point 3, approved by the maintainer on 2026-11-13).

The term is in the glossary first (§3.1): an **early probe** is a probe started before the location's turn.

## Why

The recommended setup of decision 80 (point 7) is a LAN location first, over HTTPS with no rule, then the public address. Away from home the LAN name usually fails at once, but a LAN address that the network drops (a private address on another subnet goes to the gateway) is silent until the probe's bound, 8 s, and only then is the public address tried. Trying both at once costs the wait of the slowest location that has to be waited for, not the sum. It is safe only where trying a location early sends nothing an impostor could read: a server that must present a certificate the machine checks before a request carries the user and password (#455's own condition), or a location that sends no credential at all.

## Decided

### 1. Which locations are probed early

A pure function in `domain::planning` (`RepositoryConfig::early_probes`, over the entry's locations and each rule's verdict) says, for each location in order, whether it is probed early. A location is, when both hold:

- **Its rule holds** (`RuleVerdict::Holds`, or it has none). The rules are evaluated first, as before (decision 80, point 3): a location whose rule isn't met or can't be evaluated is never probed, early or in its turn.
- **It makes the server prove who it is before it is sent a credential, or sends none** (`Location::checks_server_first`): every address it has, `repository` and `repo-hot` alike, is either
  - a **local path**, written absolute (`/…`, or `local:/…`), which sends nothing to a server; or
  - **`rest:https://…`**, whose TLS handshake checks the server's certificate before the first request carries the user and password,

  and no option set turns the certificate check off for an `https` address: with `http-insecure-tls` true in `options`, `options-hot` or `options-cold`, whichever half the address is, any certificate is accepted (`unchecked-certificate`), so the location waits for its turn. One set saying `true` is enough, since the cold half reads `options` with `options-cold` and the hot half `options` with `options-hot`; the option means nothing to a local path.

The check is made twice: on the location **as written**, before anything is resolved, and on the location **as resolved**, just before its early probe starts. A location whose address is a reference (`${LAN_URL}`) isn't known to be either kind as written, so it waits for its turn and its references are read only then; a location that is `rest:https://` as written but whose resolved options turn `http-insecure-tls` on, or a local path written `/srv/${DIR}` whose reference holds a `:` (which rustic would read as a scheme), waits for its turn too.

**Every other location waits for its turn:** `rest:http://` (with credentials, never early; without any, also in its turn, so the rule a person checks is one line: early only over HTTPS or on a local path), OpenDAL, `sftp`, `rclone:`. *Why OpenDAL, `sftp` and rclone wait, though most of them use TLS or SSH:* whether they check the server before a credential leaves depends on settings Ladderback doesn't see: an OpenDAL service's default endpoint, the credentials a service reads from the machine's own files, an SSH host-key policy, rclone's own configuration file (`no_check_certificate`). Widening the list to one of them is a later decision that names how the check is known to happen.

### 2. What counts as answering, and the order

- **"Answers" is the probe of decision 80, point 5, unchanged**: open the location with its own credentials and the repository's password, with no cache, and read the repository's ID; the location is usable when that ID is the one the entry records (decisions 57 and 80). A probe started early is the same probe, bounded the same way.
- **The first location in order whose probe returns `used` is used.** A location that answers sooner than an earlier one is held until every earlier location has its outcome (an answer, a failure, its bound run out, its rule or secret skipping it). A fast public address never wins over a slower LAN that answers.
- **A location's turn** comes when every earlier location has its outcome and none was used. A location not probed early is resolved and probed then, as before; one probed early is waited for then, if it hasn't answered yet.

### 3. How long earlier locations are waited for

Each probe keeps its own bound, `LOCATION_PROBE_SECONDS` (8 s), counted from its own start: no new timeout. An earlier location probed early is waited for until its own probe ends or its bound runs out; one probed in its turn, from its turn. With every location probed early, the wait before a job is that of the slowest location the choice has to wait for, at most 8 s, not the 32 s of four silent locations in a row; with plain-HTTP locations among them, each of those adds its own wait in its turn, as before, while the early ones run beside it.

### 4. A probe that loses, and a job that is stopped

- **Once a location is used**, every probe still running is stopped through its control (for a worker: SIGTERM, then SIGKILL, §4.1), and the choice waits until each stop returns before the job starts: no probe worker outlives the choice. A probe stopped this way has no outcome the operation records (point 6).
- **When the job is stopped while the choice runs** (a cancel), every running probe is stopped, and nothing starts after.
- **When the choice ends otherwise** (no location used, or secrets past what the daemon masks), the probes still running are stopped the same way before the choice returns.
- **Nothing is written through a probe**, won or lost: it reads `config`, the keys and the ID, and runs with no cache (decision 80, point 5). Its worker holds only the repository's password and its one location's secrets (I4), whether it is stopped or ends.
- **Each probe still starts through the spawn gate** (decision 58), one start at a time; up to four probe workers run at once.

### 5. No reuse of a probe's open

The job opens the chosen location again in its own worker, as before (decision 80, point 5: "a repository with several locations opens twice per job"). A probe that kept its open repository for the job would need the job to share the probe's process, which decision 80's kill rule forbids, and nothing in the engine port hands an open repository from one worker to another. The cost stays one more open per job.

### 6. What is recorded

The vocabulary of decision 80 (`LocationOutcome`) is unchanged; no value is added:

- **The operation:** a location before the one used keeps its own outcome; the used one is `used`; a location after it is `not-tried`, whether or not an early probe of it had started: the choice never needed its answer, so the answer, if it came, is neither waited for nor kept, and the stopped probe isn't a `silent`. `not-tried` now means "the choice didn't reach it", not "nothing was sent to it" (the cost, below).
- **A Test** (decision 80, point 8) probes every location, past the one a job would use, and records each one's own outcome, as before; the locations that may be probed early are probed from its start.
- **The log** and the `location-skipped` and `location-plain-http-credentials` warnings are unchanged: the warning about a location's credentials is given in its turn, as before, and a location probed early can't carry one (point 1).

### 7. Secrets

A location probed early has its references resolved at the start of the choice, in order, instead of in its turn: still once per job (`JobReferences`, §10.2), still only for a location whose rule holds. A reference that doesn't resolve skips it as `secret-unavailable` in its turn, as before; secrets past what the daemon masks (`secrets-too-large-to-mask`) fail the job only if its turn comes, as before, so a location after the one used never fails a run that would have succeeded. The early locations are resolved before the first location's turn, so a slow reference of one (a command, a keyring) delays that turn by as long as it takes to read, whatever kind the first location is; the probes already started run meanwhile.

## Cost

- **Extra connections.** A run may contact every early location, though it uses the first: with a LAN location and a public one over HTTPS, the public server sees a probe, a TLS handshake and an authenticated read of `config`, on every run made at home. A probe that loses may be cut off mid-request.
- **A lockout tool** (fail2ban and the like) on such a server sees those requests as logins with the right credentials, aborted at worst; it sees them only for locations that check the server first, never for a plain-HTTP location, which is contacted only in its turn, as before.
- **References read earlier.** A command or keyring reference of an early location runs on every run where its rule holds, even when an earlier location is used (point 7).
- **Up to four workers at once** for a few seconds, instead of one at a time.

## Rejected

- **Probing every location early, plain HTTP included.** It sends a plain-HTTP location's credentials on every network, to whatever answers there, which #455 names as the reason not to (decision 80, residual risk 1).
- **Using the first location to answer.** Decision 80 rejected it ("trying every location and using the fastest"); the order the person wrote decides.
- **Starting the next location only after a delay** (a staggered start, "happy eyeballs"). It saves connections when the first location answers quickly, and costs that delay whenever it doesn't; with at most four short probes, starting all at once is simpler and the extra connections are the ones the cost above names.
- **A new outcome for a location probed early whose answer wasn't needed.** The operation says where the run went and why earlier locations weren't used; a later location's answer is no reason for either, so `not-tried` keeps saying that.
- **Reusing the probe's open repository for the job.** Point 5.
