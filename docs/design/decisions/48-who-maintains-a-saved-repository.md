# Decision 48: Who maintains a saved repository

**A repository created under a name is saved with `maintenance = "local"`, and a connected one with `local` only when the person turns on "Maintain this repository from this agent", off by default; otherwise it is saved with no `maintenance`, which asks for nothing** (recorded 2026-11-11, §4.2, §7.3, I1).

**Amended 2026-11-15 (the maintainer, #619):** a saved repository's maintenance from this agent is turned on or off from the Snapshots page, after a dialog that says what that means, or with `PUT /api/v1/repositories/{alias}/maintainer` (`{"maintain-here": …}`, 204).

- **Where.** The switch is one button on the repository's Snapshots page, under its maintenance, whose words follow whether this agent maintains it.
- **What it writes.** On writes `local`, replacing a `hub` request too; off writes `none` over `local` and leaves `hub` as it is, since this agent doesn't maintain it either way.
- **No other-hosts check.** Turning it on looks for no other machine's snapshots first, as connecting doesn't: without a hub nothing can rule another maintainer out (§7.3), so the dialog says that only one place may maintain a repository.
- **The timing gap is closed.** Turning maintenance off while a forget, prune, check or adoption of the repository runs here is refused with `maintenance-busy`, naming it. The change runs under the configuration's write lock, and a run reads again under that lock, before it takes its slot, that this agent maintains the repository: a run assessed before maintenance was turned off ends with `maintenance-not-owner`, nothing stored (I1).

Only a repository that asks for `local` is maintained here: forget, prune, check and adoption, and the preview of each, are refused with `maintenance-not-owner` for any other.

- **Created here.** A repository created here can be maintained by nothing else yet, so asking it is only friction.
- **Connected.** A repository that already exists may be maintained by another agent or the hub, and only one place may (I1), so maintaining it from here is the person's explicit choice, never a default; the checkbox's hint says what maintaining means and why to leave it off.
- **The API.** `POST /api/v1/repositories` carries the choice as `maintain-here` (absent is `false`); `_init` needs none.
- **Imported profiles.** An imported profile keeps its own rule (`local` only with `[forget] prune = true`, §6.2).

There is no migration: a configuration v0.1.0 wrote has no `maintenance`, so after upgrading its repositories are not maintained here until maintenance from this agent is turned on for each, on the Snapshots page; the page's button and the refusal say where, and the release notes say so.

Asking for the hub from the UI waits for the hub (M4b).

Ownership is checked before a run reads the repository, and again under the configuration's write lock as the run takes its slot, so turning maintenance off can't fall between the two (amended above). The hub's maintenance lease (M4b) is a grant that can lapse, so it is checked at the slot too.

Rejected:

- saving every connection with `local` (two machines connecting one repository would both prune it, breaking I1; without a hub nothing can rule that out, and the agent can only warn, §7.3);
- changing the `Maintenance` default to `local` (a configuration edited by hand, or a repository nobody chose to maintain, would start pruning silently);
- migrating v0.1.0 configurations to `local` (the same risk for every repository that another machine already maintains).
