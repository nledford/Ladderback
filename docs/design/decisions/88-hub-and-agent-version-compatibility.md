# Decision 88: How far hub and agent versions may differ

**Hub and agent compatibility is counted in sync protocol versions, not release versions. The agent's `Hello` names the protocols it speaks and the hub answers with the highest both speak; a release speaks at most two protocols, N and N−1; N−1 is kept for at least 6 months after N first ships, and dropping it is a breaking change, from 1.0 a major release. An agent with no protocol in common with the hub keeps backing up locally and says why** (recorded 2026-11-13, §7.2, §7.3, §12.1; approved by the maintainer). It answers open question 4 of §17, from the first hub release (M4a), before 1.0 too.

## Why

Machines are upgraded one at a time, and a hub on a NAS is often upgraded later than the laptops that talk to it, so neither side may assume it goes first. Counting releases would make every release a potential break; counting protocols lets most releases change nothing on the wire that matters, since new optional fields are ignored by an older peer. Two protocols in one release keep the code that speaks old protocols small, and 6 months is long enough for a machine that is rarely updated.

## Decided

1. **The window is counted in protocols.** Additive changes within a protocol (new optional message fields) never break and don't use up the window. A new protocol number is rare.
2. **Negotiation goes both ways.** `Hello` carries the range of protocols the agent speaks; the hub answers `Welcome` with the highest both speak and its own range (§7.2). A newer agent works with an older hub and an older agent with a newer hub. The handshake (nonce, signature, `Hello`, `Welcome`) never changes between protocols.
3. **A release speaks at most N and N−1.** A release that adds protocol N+1 therefore drops N−1.
4. **N−1 stays for at least 6 months** after protocol N first ships. Dropping it after that is a breaking change (§12.1) and from 1.0 needs a major release, so from 1.0 at most one new protocol ships per major release.
5. **It applies before 1.0**, from the first hub release (M4a).
6. **A mismatch never stops local backups.** An agent with no protocol in common keeps running its plans locally and shows whether the hub or the agent is out of date, with both sides' protocols; the hub lists the agent as needing an update (or itself, when it is the older side). Maintenance stays safe by the existing rules: without a hub connection the agent's lease lapses, so once it expires the agent runs no scheduled maintenance (I1, §7.3); and since the hub's `keep_delete` and `prune_margin` may change during a mismatch that can last months, the agent computes deadlines as if it had never received `RepoGrants`, rustic's 23 h `keep-delete` (I3, §7.3 layer 1).
7. **Advance warning.** When an agent or the hub is on the oldest protocol its peer speaks, hub and agent say so and name the release that will drop it; the release notes repeat it.
8. **CI tests the window** (#267): each release's hub and agent are tested against the last release of the previous protocol, in both directions; #267's recorded fixtures cover older releases within one protocol.
9. **A change in meaning is a new protocol.** Any change to what a lease, a grant, a deadline or ownership means in `RepoGrants`, `ReposSnapshot` or `Heartbeat` (§7.3) takes a new protocol number, never an optional field an older peer could ignore.

Left out: a longer window (three protocols or more), which would multiply the code and tests for old protocols, and requiring the hub to upgrade first, which the old draft of question 4 assumed.
