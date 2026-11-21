# Decision 137: A hub moves by handover, never by copy, and a hub that finds itself rewound recovers before it grants

**Each machine keeps, for every repository, the highest lease epoch it has seen, and refuses a grant below it as `epoch-lower`, keeping its own lease until it expires; the floor never resets, not even in a recovery. A hub that starts on a machine whose machine identifier differs from the one it recorded, or that a machine's heartbeat shows to be behind an epoch floor, enters recovery mode: it serves status and grants nothing until a person confirms, then issues epochs above the highest any machine reported. Moving a hub is a handover, never a file copy: a promotion while the old hub lives, the recovery handover when it is gone, and no private key is copied. A restore doesn't raise the generation, and M9 has no signed export bundle** (recorded 2026-11-21, §7.3, §16; the maintainer's decision on #1039).

## Why

- **The generation floor alone misses a same-generation restore.** Decision 120 reserves the hub's generation, and nothing raises it before a move. A hub restored from a backup keeps its generation but has older epochs, so it passes the floor and can grant a lease a machine already holds at a higher epoch: two maintenance owners, which I1 forbids.
- **The machine holds the floor, so a rewound hub can't rewrite it.** Every `RepoGrants` entry for a repository that a machine acts on raises its floor for that repository, whoever the entry names as owner; an entry at the floor is a renewal. A refusal applies to that entry only, so one stale entry stops no other lease. A refused entry leaves a lease as it was, to expire as any lease does (§7.3). Nothing a hub sends lowers the floor, and a recovery handover restarts the generation floor (decision 120) and leaves this one alone.
- **Recovery mode is the hub's answer to a rewind it can see.** The first trigger is the machine identifier that decision 120 stores beside an agent's ID (#296). The hub records it too, so a restore onto another machine is caught, and a restore onto the same machine isn't. The second trigger catches that one: a heartbeat reports a floor above the hub's own counter for a repository. Both put the hub in recovery mode, which it says on its pages and in its log.
- **A report counts only with its proof.** A heartbeat carries, for each repository, the floor and the hub-signed entry that set it. The hub acts only on a floor its own key chain signed and logs any other, so a machine that claims a huge floor can't hold the hub in recovery or leave it no epoch to issue.
- **Recovery grants nothing, and backups go on.** The hub serves status, runs no maintenance of its own and renews and grants no lease until a person confirms with the step-up decision 120 asks for approving a machine. The confirmation says that a copy of the hub's files looks the same as a restore and asks whether the old hub is stopped or gone. Machines keep backing up, as decision 88 has them do when the hub is missing, and keep the leases they hold until those expire.
- **A recovered hub learns the epochs from the machines.** After the confirmation it issues epochs above the highest floor reported. It grants a repository to a machine other than the one that reported holding it only after that machine's `ReleaseAck`, or 7 days after the hub entered recovery mode, since a lease the hub renewed before the rewind can run that long (§7.3).
- **A move is a handover, so ownership never changes hands in it.** While the old hub lives, a paired machine is promoted: the old hub signs a handover to the new machine's own hub key, raising the generation by one as decision 120 reserved. Signing it retires the old hub, which grants and renews nothing after, so there is one arbiter. The promoted hub starts with the epoch counters of the hub it replaces, and records its own machine identifier at its first start. Leases keep their owners.
- **Recovery is the move when the old hub is gone.** A backup of the data folder leaves the key files out (decision 120), so the person recovers with the recovery seed, which signs the recovery handover to a fresh key. A hub made new after "What losing both costs" has counters behind every floor and enters recovery mode once. A repository removed and added again keeps its counter, so it only rises.
- **No private key is copied.** Every machine already follows the chain of handovers from the key it pinned, so none pairs again and the old key is trusted nowhere afterward. A file copy of the registry would carry no fence of its own and would rely wholly on the floor and recovery mode.
- **The hub accepts a key on disk that its chain reaches.** A hub whose recorded key fingerprint is older than the key file, as after the database is restored beside a newer key, accepts a key that its `hub-handovers` chain reaches from the recorded fingerprint. Any other key is `hub-key-mismatch`, as decision 120 has it.
- **No signed export or import bundle in M9.** It looks like the simple move but copies the registry, fences nothing and holds the move's safety to the two rules above. Promotion and recovery cover the moves.

## What is not decided here

The move's own steps, its freeze of the hub, its page and its command line are built with the move, in M9 or M10. They add no way to hand over without the rules above. #571's leases build on this decision, and the generation, the machine identifier check, recovery mode, the epoch floor and the heartbeat field that reports it are in the first protocol that carries leases, since a field a machine relies on for safety can't be added later as an optional one (decision 88, item 9).

## Scenario

This scenario joins M9's `maintenance/ownership.feature`.

```gherkin
  Scenario: A machine refuses a grant from a hub restored to an older epoch
    Given a laptop holds the lease on repository R at epoch 7
    When the hub is restored from a backup where R's last epoch was 4, at the same generation
    And the hub grants R to the NAS at epoch 4
    Then the laptop refuses the grant as "epoch-lower"
    And the laptop keeps its lease until it expires
    When the laptop's heartbeat reports its floor of 7
    Then the restored hub enters recovery mode
```

## Rejected

- **The generation floor only:** it misses the same-generation restore above.
- **"A restore means rebuild and re-pair":** no fence holds while two copies of the hub run, and every machine's pairing is lost for it.
- **The machine identifier check alone:** it misses a restore onto the same machine.
- **Raising the generation on a restore:** a hub restored onto its own machine can't tell it was restored, so nothing would raise it. The epoch floor covers that restore.
- **Refusing only `/sync/v1` on a different machine,** as a missing key does today: the hub would still grant, which is the harm.
- **A signed export and import bundle in M9:** see "Why".

## What this doesn't close

- **A window after a same-machine restore.** The hub can't see the rewind until a machine reports a floor above its counter, and `RepoGrants` go out at connect, before any report. A grant it makes in that time to a machine that never held the repository, such as the NAS in the scenario, stands until its lease lapses, up to 7 days, beside the holder's. Recovery mode stops renewals and grants, not leases already given. Closing it needs a rule this decision doesn't make, such as no change of owner after any start until every machine using the repository has reported.
- **A copy that carries the key files** starts in recovery mode, and a person's confirmation is all that lets it grant. Whether a different machine should leave recovery only by the recovery handover is the maintainer's to decide.
- **A leaked key.** A grant it signs can raise a machine's floor to the maximum for a repository, and that machine then refuses the repository's leases for good: no recovery, reset or setting lowers a floor, so only a data folder made new, a new machine, starts without one. Its backups go on, and the hub can't issue above the maximum.

Amends decision 120's scope: a move of the hub and a restore are decided here, no longer open.
