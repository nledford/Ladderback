# Decision 89: How an AI agent proposes a backup setup for a person to confirm

**An AI agent (any API client with the `propose` scope, or any client when sign-in is off) can propose changes to an agent's configuration, as a list of typed configuration changes, and nothing changes until a person approves the whole proposal in the web UI.**

**Ladderback writes the plain-words summary and the exact `config.toml` diff from the changes itself. It shows separately:**

- **what the proposal could delete;**
- **which maintenance it claims;**
- **every command it would add, each command needing its own tick.**

**Approval needs step-up confirmation. The AI agent names secret sources, never values.**

**A proposal is checked at submission without resolving a secret, running a command or contacting a location; it is applied in one step under the configuration write lock, and it expires after 48 hours. There is still no MCP tool that approves anything. With sign-in off, approval is a convenience, not a safety boundary** (recorded 2026-11-14, §0, §3.1, §4.4, §4.6, §10.1, §10.4, §10.5; #518, approved by the maintainer).

It amends decision 20 ("nothing … edits config or adds a command") and I4 (one display of a generated password, point 11).

**Amended 2026-11-15 (#518, its first step):** proposals ship before anyone can approve them. The first step keeps them and opens `/api/v1/proposals` to make, list, read and withdraw them; approval, rejection and the "Pending changes" page come in a later step, so until then a proposal ends only withdrawn, expired or stale. In this step:

- **Four routes, no revise.** Create, list, get and withdraw (point 14); an AI agent revises by withdrawing and proposing again.
- **The changes are plans for now:** add, change or remove a plan, checked in order against the configuration as the changes before leave it, by `AgentConfig`'s rules. Repositories, hooks and maintenance claims (point 1) come with the steps that need their secret and command handling.
- **No hook changes yet.** A proposal gives no plan hooks of its own: a new plan with any hooks, or a changed plan whose hooks differ from those it has, removal included, is refused as `proposal-invalid` with field and problem `hooks`. Hook changes stay refused until the step that brings each command's own confirmation on the page (point 6), so nothing an AI agent proposes can add, change or take away a command before a person sees it verbatim.
- **Limits:** 50 changes and a 4000-character rationale per proposal (`proposal-too-large`), 20 pending per proposer (`proposal-limit`), and the 200 newest ended proposals kept as history, the oldest beyond them forgotten when a new one is kept. A proposer is one token, and every client with no token together. The pending limit counts the proposals still pending when the new one is made, by their expiry, so a proposer whose proposals expired unread can propose again without a read first; one gone stale that nothing has read yet still counts until a read marks it. An expired proposal joins the 200 ended once a read marks it, and a list marks every one it returns.
- **Who sees what.** A token sees and withdraws only the proposals it made, and another token's is `proposal-unknown`, as one never made is; a session, or any client while sign-in is off, sees them all.
- **A session may propose too,** recorded as proposed by no token, as a client with sign-in off is.
- **The base revision** is the SHA-256, in hex, of `config.toml`'s bytes, read before the configuration the changes are checked against is loaded.
- **Reading brings a pending proposal up to date:** `expired` from its expiry on, `stale` once a change of it no longer applies; the status change is the store's atomic one, and a read that loses the race answers the status the winner kept. Kept changes that can't be read back, as when a rule has tightened since they were kept, no longer apply: the proposal is answered with no changes and a pending one goes stale (or expired), rather than every read of the proposals failing. A proposal forgotten while a list is being read is left out of it.
- **Refusals,** each a code: `invalid-body` 400; `proposal-empty`, `proposal-invalid` (naming the change from 1 and its setting, never a value) and `proposal-too-large` 422; `proposal-limit` and `proposal-not-pending` 409; `proposal-unknown` 404; and `proposals-unavailable` 503, with no arguments, for a store, a change that can't be written to be kept or a random source that fails, the log saying which. The number of changes is checked before any change is read, so a proposal of more than 50 is `proposal-too-large` whatever a change beyond the limit holds.
- **Later steps:** the rate limits on proposals and rejections, and the submission warnings (point 5), come with the next step, as do approval, rejection, the page and the events.

**Amended 2026-11-16 (#518, its second step):** a person approves or rejects a proposal of plan changes, on the "Pending changes" page. In this step:

- **Three routes, a session's only** (point 7): `GET /api/v1/proposals/{id}/review` answers the proposal as the page shows it, `POST /api/v1/proposals/{id}/approve` and `POST /api/v1/proposals/{id}/reject` decide it. No token scope opens them.
- **Step-up** (point 6) is a restore's: the admin password, in the sign-in waits, with sign-in on; with it off, the proposal's short ID, the first 6 characters of its ID (or the whole ID). A wrong one is `step-up-failed` (403), and the waits answer `sign-in-wait` (429) and `sign-in-unavailable` (503). The proposal service is built again once sign-in is decided, as the restores' is. Step-up is checked first, before anything about the proposal is refused.
- **What an approval binds to.** The review answers the file's revision, keyed (below), for the file the diff was rebuilt against, and the SHA-256 of the diff as shown, masked, and an approval sends both back. Point 6's "the approval binds to the unmasked text" is replaced: the revision already changes with any change to the unmasked file, so it binds every byte the mask hides, and a hash of the unmasked diff would be a hash of the secrets, sent to the page (I4). Under the write lock the approval rebuilds the diff against the file as it is; a file or a diff other than the one shown is `proposal-changed` (409), and the proposal stays pending.
- **The revision is keyed** (I4). The file's own revision, its SHA-256, hashes its inline secrets too, so a page that held it could check a guess at one. A page is shown instead the SHA-256 of a key followed by that revision, in hex. The daemon draws the 32-byte key from the platform's random source once each start and holds it in memory only, never logged, kept or sent. A key of fixed length ahead of the revision is enough for a binding compared for equality. A restart draws another key, so an approval of a review left open is `proposal-changed` and the page reads the review again. With no random bytes from the platform, a review or an approval is unavailable. The plain revision's `Debug` shows `ConfigRevision([hidden])`, so no log of a value that holds it writes it out.
- **The apply** starts in the store's atomic `pending` to `applying`, and is saved only over the file the approval was rebuilt against: a hand edit made meanwhile is kept, and the proposal goes back to `pending` with `proposal-changed` (point 12). A write that fails leaves it `failed`; retrying a failed apply comes later. An applied proposal records the configuration history version its apply kept. The approval runs in a task of its own, so a browser that hangs up mid-apply doesn't leave it halfway. A saved file the store can't record as applied is still answered applied, and the daemon logs the store's error by its code; the store shows it `applying` until the next start. A proposal the daemon finds `applying` when it starts becomes `failed`, announced (point 12: with no secrets or repositories in this step, there is no step 2 to resume after).
- **The diff is masked by value** (I4): each string of the file is read as TOML reads it, escaped or over several lines, and one that holds a secret the configuration knows is written again masked; then every secret is masked wherever else it shows, such as a comment. Masking secrets written with TOML's escapes in other text is #839.
- **Not before the forget preview** (point 8). Until the forget preview ships, a proposal holding a change that could delete snapshots is only rejected: its approval is refused `proposal-needs-forget-preview` (409), after step-up, and the review says so. Such a change is every removal, a new plan with a retention policy, and a changed plan whose retention, repository or host is different; a change that no longer applies counts as one. A new plan counts by its retention policy, since only a read of the repository tells whether its ID matches an orphaned group. A change of a plan's ID, `group-by` or `keep-tags` is out of scope: no proposal can make one yet.
- **Reject** takes a reason per change (point 6), each at most 1000 characters, an empty one for a change given none; reasons that don't fit are `proposal-reasons` (422, `problem` `count` or `too-long`).
- **Rate limits** (point 6): a proposer makes at most 30 proposals an hour (`proposal-rate`) and none for 5 minutes after one of its proposals is rejected (`proposal-after-rejection`), both 429 with `Retry-After`. The review says how many of the proposer's kept proposals were rejected.
- **Warnings** (point 5): the answer to a created proposal lists `same-sources` (a plan left backing up the sources of another) and `glob-never-matches`; the proposal is kept all the same.
- **Up to date at once** (point 3): a plan saved by the daemon brings the proposals up to date after the save, so one the save made stale goes stale, announced, without waiting for a read.
- **Events** (point 14): each status change is a `proposal-changed` event, the proposal's ID and status only.
- **The page** is "Pending changes" (`/proposals`), linked after Plans: the pending proposals, each with a Review button, and a review with the summary, the rationale apart and isolated, whether the file changed since the proposal was drafted, the rejections before, the masked diff, Approve with the step-up field (or why only Reject is offered) and Reject with a field per change. With sign-in off it says that approval is then a convenience (point 7).
- **Later steps:** the forget preview and approving what needs it, the count in the navigation, commands with their ticks, secrets, maintenance claims and the retry of a failed apply.

**Amended 2026-11-16 (#518, the forget preview):** a person sees, on request, what a pending proposal would change about the snapshots retention forgets (point 8); approving a proposal that could delete snapshots stays refused `proposal-needs-forget-preview` until a later step binds an approval to the preview (the next amendment). In this step:

- **One route, a session's only** (point 7): `POST /api/v1/proposals/{id}/forget-preview`. A POST, since it reads the repositories: their secrets are resolved and their locations contacted, though it changes nothing. No token scope opens it, so an AI agent can't make the daemon read a repository on its behalf.
- **Every repository the proposal touches** (I6), in the order its changes name them: a plan's repository before and after a change, and a removed plan's. For each, what this agent's forget would remove under the configuration as it is and as the proposal makes it, each read as a forget's preview reads it (decision 44), and the difference by snapshot: what only after, and what only before, would forget, per plan. A new plan that takes the ID of an orphaned group's plan shows that group's history forgotten after, and a removal shows the removed plan's history no longer forgotten, since a forget leaves a group no plan owns alone.
- **A side this agent doesn't maintain** the repository under (decision 48) forgets nothing there, and says so: no forget of its runs there.
- **A side whose forget would be refused**, because the policy of a plan with history there keeps nothing (decision 25), forgets nothing there and says which plan; the other side is told all the same.
- **A repository that can't be read** is told as the maintenance routes tell it, each secret of its settings masked (I4); the others are answered all the same. A proposal that is no longer pending has no preview (`proposal-not-pending`).
- **The page** offers "Show what it would forget" beside the reason only Reject is offered, and shows each repository's lines and the snapshots of each difference in a table. Reject stays available while the preview reads, since rejecting deletes nothing and the read can take minutes; the preview isn't offered while a rejection waits.

**Amended 2026-11-17 (#518, approving with the forget preview):** a person approves a proposal that could delete snapshots from its forget preview, and the approval holds only while that preview is still true. In this step:

- **The preview's digest.** The forget preview answers a `digest`: the SHA-256 of the daemon's revision key, the proposal's ID and, for each repository in the preview's order, its alias and the snapshots whose fate the proposal changes, as the preview's difference shows them (each plan's IDs only the proposal would forget, then each plan's IDs it would no longer forget, each set in ID order). Keyed like the review's revision, so only this daemon since its start gives one, and a digest made for one proposal never holds for another. A preview that couldn't read a repository has no digest, and the page offers no Approve from it.
- **What the digest binds** (the maintainer's decision): what the person saw change, nothing more. A plan's count of snapshots, what is left alone, a snapshot both sides forget or neither does, and why a side forgets nothing are no part of it. So a backup that lands between the preview and the approval refuses the approval only if it changes those sets, as a backup of the plan whose retention changes does when it shifts what a count keeps; a backup of another plan in the same repository leaves it standing. Binding every count would refuse nearly every approval on a repository a plan backs up every few minutes.
- **Approval brings it.** The approve request carries `forget-digest`. A `forget-digest` that isn't a digest at all is refused `proposal-forget-preview-changed` before anything else is checked. Otherwise the review's revision and diff are checked first: a file changed since the review is refused `proposal-changed`, with a digest or without. Then, without one, a proposal that could delete snapshots is still refused `proposal-needs-forget-preview`. With one, the daemon reads the preview again and applies the proposal only if the digests match, compared in constant time. A preview that now forgets other snapshots than the person saw change, as after a repository replaced, or one no longer readable, is refused `proposal-forget-preview-changed` (409), the proposal left pending for a person to preview again. A snapshot a backup adds after the check is a later forget's to judge, under its own preview.
- **The lock while it reads.** Approval holds the configuration write lock while it reads the repositories again, as connect and init hold it across their repository read, so the configuration can't change between the check and the write. Meanwhile every other write of the configuration (a plan save, a change of maintainer, a connect, an init) and a maintenance run a person starts wait for it, up to the read limit (10 minutes) for each read it makes: each repository's listing, and each maintained plan's retention under each side. Backups and scheduled maintenance don't take the lock.
- **The page.** The "Pending changes" page shows Approve for such a proposal only once a forget preview has read every repository it touches, and sends that preview's digest. A preview that couldn't read one says so and offers only Reject until a preview reads them all. While an approval or a rejection waits, "Show what it would forget" is unavailable, and a new preview clears an earlier approval's refusal.
- **Smallest that holds.** The daemon keeps nothing between the preview and the approval: the digest is recomputed, never looked up, so a restart, a stale page or a forged digest gains nothing, and no preview needs to expire. Reading the repositories again costs a second read at approval, which a person waits for once.
- **Approving forgets nothing.** The apply only saves the configuration (I2, I6): a forget runs later, under the plans as approved, and its own preview and refusals hold as before.

The terms are in the glossary first (§3.1): an **AI agent**, a **proposal**, a **configuration change** and a **plan dry run** with its **dry-run allowlist**. An *agent*, alone, is still a machine running Ladderback.

## Why

People will expect to ask an AI agent to help them define, build and set up a backup strategy: which folders matter, what to leave out, where to send it, how long to keep it.

Decision 20 kept every config edit away from MCP because a model-driven client is not a human confirmation, and that reason stands. What changes is that the AI agent no longer edits: it proposes, and a person confirms in the one place the AI agent's host can't draw or approve automatically, the web UI.

The feature guards against a confused or prompt-injected AI agent using its MCP or API access. It does not guard against code execution: an AI agent with a shell as the daemon's user can edit `config.toml` directly and bypass all of it.

## Decided

1. **A proposal is a list of typed configuration changes**, parsed into domain types where the request enters (§3.1), never `config.toml` text or a patch. The changes are a closed set:
    - add, change or remove a repository (its locations, options and secret references), a plan (sources, source filter, schedule, retry policy, retention, label, tags, host, `group-by`) or a hook;
    - claim maintenance of a repository.

    A proposal can never change sign-in, API tokens, hub pairing, `allow-hub-restore`, notification channels and rules, the agent's own settings (`[agent]`), or the dry-run allowlist (point 13).

    Each configuration change is applied by a method of `AgentConfig`, which keeps its rules, and the proposal applies them all in one config edit. The web UI's plan editor sends the same change type, so there is one way to write the configuration.

    `Proposal` is an aggregate of the Planning context: an ID, who proposed it (its token's label, or "no token" with sign-in off), the configuration the changes were drafted against (a hash of `config.toml`), the changes, the AI agent's rationale, a status and an expiry.

2. **Lifecycle.** `pending` → `applying` → `applied` or `failed`; `applying` → `pending` again when a hand edit stops the apply (point 12); from `pending` also `rejected`, `withdrawn`, `expired` and `stale`; from `failed`, `applying` again on a retry.

    Approval starts the apply in the same atomic status change in the store (`pending` to `applying` only from `pending`), so nothing sits approved but unapplied, and of two tabs approving at once only one does. A proposal **expires 48 hours** after it was made, a fixed limit that a return to `pending` doesn't reset; ended proposals stay visible as history.

3. **`stale` is narrow.** When the page opens, and whenever the configuration is saved, the typed changes are applied again to the current file. Only a change that no longer applies (a plan removed meanwhile, an alias taken) makes the proposal `stale`, and the AI agent proposes again.

    A comment edit, a load migration or the daemon recording a repository ID doesn't; the person approves the diff rebuilt against the current file, and the page says when the configuration has changed since the proposal was drafted.

4. **What the AI agent writes and what the person reads.** The plain-words summary is generated by Ladderback from the changes, never written by the AI agent, so an AI agent or a prompt injection can't describe one thing and propose another. The AI agent's rationale is shown apart from it, labelled as the AI agent's words.

    Values the AI agent chose are rendered as code, with bidi isolation; labels, aliases, tags, paths, hosts and hook arguments holding format, bidi or confusable characters are refused at submission. A command is shown as its program as resolved on `PATH`, with its arguments as a list. A remote location the configuration has never held is marked "new destination". Notifications of a proposal carry facts only, never its rationale or labels.

5. **Checks at submission are pure:** parsing and the domain rules, nothing else. No secret is resolved, no command runs and no location is contacted: a connection check there would send a stored credential to an address the AI agent chose, run a `password-command` nobody ticked, and let the AI agent scan the network.

    What the checks find goes back to the AI agent, so it can fix its draft before a person sees it:
    - warnings (`glob-never-matches`, two plans over the same sources);
    - refusals (an inline secret, a credential sent over plain HTTP, and whatever the configuration already refuses, such as a retention policy that keeps nothing on a plan with history, decision 25).

6. **Approval is in the web UI only, and for the whole proposal.** A "Pending changes" page, with a count in the navigation, shows the summary, the exact diff and, separately, the forget preview (point 8), any maintenance claim (point 9) and each command verbatim (hooks, `password-command`), each with its own tick.

    The diff passes through the `Redactor`, so a secret the file already holds inline (a password, credentials in a location's address) shows as `***`; the approval binds to the unmasked text.

    **Approve** needs every command ticked and step-up confirmation (§10.4): the admin password, or with sign-in off the proposal's short ID. Step-up attempts share the sign-in waits (`SignInWaits`, §10.1), so a stolen session cookie can't guess the password through approve. The approval is bound to what was shown: a hash of the file the diff was rebuilt against, a hash of the diff shown and a hash of each ticked command.

    **Reject** takes a reason per change, which the AI agent can read and revise that part against. There is no partial approval: it would create combinations nobody validated, and the AI agent proposes the rest again instead. Proposals after a rejection are rate-limited, and the page shows how often a client's proposals were rejected.

7. **Everything the page does is a person's action.** Approve, reject, Generate (point 11), testing a new location, the forget preview that takes a typed password, choosing a secret source and ticking a command accept only a browser session, never a token, so an AI agent with the `propose` scope can't send a credential to an address it chose or read a generated password.

    **With sign-in off, approval is a convenience, not a safety boundary:** nothing then tells a browser from an AI agent's HTTP call, so an AI agent that can send HTTP requests to the daemon can approve its own proposal and do everything the page does. The design, the "Pending changes" page and the docs say so plainly, and every promise below that a person decides holds only with sign-in on.

8. **The forget preview covers every repository the proposal touches** (I6): the snapshots the retention policies would delete before the proposal and after it, and the difference.

    A retention change is only one way a proposal can delete: so are a new plan whose ID matches an orphaned group's plan, a change of a plan's ID, repository, `group-by`, `keep-tags` or host, every removal, and a maintenance claim on a repository plans already write to. For a repository being connected, the page asks for the password first, then computes the preview.

9. **A maintenance claim defaults to off** on the page and shows the evidence of another owner (I1; decision 62).

10. **Secrets are references.** The AI agent proposes only a `new` secret, or an `existing` one where the configuration already uses it: a repository's password for that repository, a location's credential for that location. A `password-file`, a `password-command`, an environment variable, or reuse of a secret anywhere else is chosen by the person on the page, which labels it "the AI agent may know this value".

    A `new` secret is stored in the system keyring (#476), where the page offers **Type it** or **Generate one**. Where the keyring isn't available (headless servers, system services), and for backend option credentials, which take only `${VAR}` and `${file:}` (§10.2), the person types it and the page writes it to a mode-`0600` secret file under the data folder and references it; "Generate one" is offered only for the keyring.

    The approve request carries a typed secret, so the redactor of the job that initialises the repository is seeded with it. With sign-in on, the AI agent only ever sees that a secret was stored, and a failed apply sends it codes only; masked details appear only in the web UI.

11. **A generated password is shown once (amends I4).** "Generate one" displays the password exactly once, in the response to the confirmation page's Generate request (sent with `Cache-Control: no-store`). Until approval the daemon holds it in memory only, for that proposal, for at most 15 minutes and never across a restart; generating again discards it, and a lost response is answered by generating again.

    Approval stores it in its keyring entry (point 12, step 2). It is never stored anywhere else, logged, sent as an event or readable again. The person must confirm "I have saved it" before approving, because a password kept only in one machine's keyring is lost with that machine, along with every backup it encrypts.

    A canary test proves the password appears in that one response and in no log, event, operation or proposal record, `config.toml` or its history, `get_proposal` or other MCP result, or later response.

12. **Applying.** Under the configuration write lock (decision 57), in order, stopping at the first failure:
    1. reload the file, rebuild its text from the typed changes and require it to match the diff the person approved;
    2. store the new secrets;
    3. connect to or initialise the repositories (`init` writes a repository's first files, harmless if a later step fails);
    4. write `config.toml` through decision 69's editor in one atomic save, only if the file's bytes are unchanged just before the rename, as recording a repository ID already does (`ChangedMeanwhile`);
    5. reload the scheduler.

    A hand edit made during the apply is never overwritten: the apply stops before the write, keeps what steps 2 and 3 did, and the proposal returns to `pending` with the rebuilt diff for a new approval.

    The proposal records each step that succeeded, the keyring items it created and the repositories it initialised, so a failure is reported exactly and a retry, started by a person under the same approval rules, reuses them (connect, not init). A proposal the daemon finds `applying` when it starts resumes if step 2 had finished, and otherwise becomes `failed`, since a typed secret never stored is gone. A proposal that ends without being applied after step 2 lists the keyring items and repositories it left, for the person to keep or remove.

    The config history entry the save makes (decision 29) links to the proposal, recording who approved what, when and from where. Undo is reverting to that history entry; keyring items and initialised repositories aren't undone, and the page says so.

13. **The plan dry run** (`dry_run_plan`) walks a draft plan's sources through its source filter and returns how many files and bytes it would back up and what each exclude removed: counts and sizes only, never paths. It never runs a hook or a secret source nobody approved and opens no repository, so it gives no "new data" figure.

    It runs one at a time outside the backup concurrency limit, gives way to scheduled backups, has a short time limit and a rate limit, and reads only within the **dry-run allowlist**: one list of folders, empty by default, set only on the Settings page. A source is checked against it after resolving symlinks and `..`, and the walk doesn't follow a link out of it. "Add home folder" there warns that the home folder holds `~/.ssh` and browser profiles. A count still tells whether files exist inside the allowlist; that is what the allowlist bounds.

14. **API, storage and MCP.** `/api/v1/proposals` (additive): create, list, get and withdraw with the new `propose` token scope; every action of point 7 accepts only a browser session.

    Pending proposals and a proposal's size are capped, and proposals, rejections and dry runs are rate-limited, per token, and with sign-in off, where there are no tokens, for all clients together; these are input-derived limits (§13.5), each refusal a code. The events WebSocket announces status changes.

    Proposals live in the daemon's SQLite store behind a new port with an atomic status change, are recovered at start (point 12), and keyring writing comes with #476; the source walk of the dry run is the engine adapter's, which owns rustic's source filter. Each new port, code and table follows its checklist in §13.7.

    MCP gains `propose_setup`, `check_proposal` (the submission checks only), `dry_run_plan`, `get_proposal` and `withdraw_proposal`, in connect mode against an agent (§4.6), with #517. There is no approve tool.

15. **Order of work.**
    1. First, plan writing as a use case with the plan editor in the web UI, valuable alone and the apply's last step;
    2. then the `Proposal` aggregate, its REST API and the "Pending changes" page, usable by REST clients before the MCP tools, and needing M3's scoped tokens and step-up confirmation and #476's keyring writing;
    3. then the dry run;
    4. then the MCP tools, with the MCP server (#517) in M3 (§14).

    Proposals made on the hub come later, with editing plans from the hub, through `UpdatePlan` (decision 2).

16. **Proof.** Scenarios:
    - a proposal changes nothing until approved;
    - a token can't approve, test a new location or generate a password;
    - a proposal with a command can't be approved until that command is ticked;
    - a change that no longer applies makes a proposal `stale`, and a comment edit doesn't;
    - the summary matches the changes even when the rationale claims otherwise;
    - no secret is resolved and no location contacted at submission;
    - a dry run runs no hook and reads nothing outside the allowlist;
    - the forget preview is shown for a retention change and for a new plan that reuses an orphaned plan's ID;
    - a hand edit made during an apply is kept;
    - approval with sign-in off works and the page says it is no safety boundary;
    - the I4 canary scans every proposal response, page response and MCP result, and finds a generated password in its one response and never again.

**Cut from v1:** a folder-size probe (the limited dry run covers it) and a `wait_proposal` tool (`get_proposal` and the events cover it).

## Rejected

- **A: the AI agent submits `config.toml` text or a patch.** The AI agent would write TOML, checks would come late, the plain-words summary would have to be worked out backwards from text, and it fights decision 69's keeping of the person's layout.
- **C: the AI agent writes directly with a scoped token, plus an undo window.** It breaks the decided web-UI confirmation, and a forget (I6) or a maintenance claim (I1) can't be undone.
- **D: the AI agent fills a wizard through MCP elicitation.** The AI agent's host draws the form and may approve it automatically, so it is no human confirmation (decision 20).
