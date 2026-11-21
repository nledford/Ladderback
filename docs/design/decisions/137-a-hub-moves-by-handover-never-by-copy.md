# Decision 137: A hub moves by handover, never by copy, and a hub that finds itself rewound recovers before it grants

**Each machine keeps, for every repository, the highest lease epoch it has seen, and refuses a grant below it as `epoch-lower`; the floor never resets, not even in a recovery. A hub that starts on a different machine, by its machine identifier, or that a heartbeat shows to be behind a floor, enters recovery mode: it serves status and grants nothing until a person confirms, and on a different machine only the recovery handover ends it. After any start, no repository changes owner until every machine using it has sent a heartbeat or 7 days have passed. Moving a hub is a handover, never a file copy, and no private key is copied. A restore doesn't raise the generation, and M9 has no signed export bundle** (recorded 2026-11-21, §7.3, §16; the maintainer's decision on #1039 and his answers of the same day).

## Why

- **The generation floor alone misses a same-generation restore.** Decision 120 reserves the hub's generation, and nothing raises it before a move. A hub restored from a backup keeps its generation but has older epochs, so it passes the floor and can grant a lease a machine already holds at a higher epoch: two maintenance owners, which I1 forbids.
- **The machine holds the floor, so a rewound hub can't rewrite it.** Every `RepoGrants` entry for a repository that a machine acts on raises its floor for that repository, whoever the entry names as owner; an entry at the floor is a renewal. A refusal applies to that entry only, so one stale entry stops no other lease. A refused entry leaves a lease as it was, to expire as any lease does (§7.3). Nothing a hub sends lowers the floor, and a recovery handover restarts the generation floor (decision 120) and leaves this one alone.
- **Recovery mode is the hub's answer to a rewind it can see.** The first trigger is the machine identifier that decision 120 stores beside an agent's ID (#296), the operating system's own value. The hub records it at its first start and compares it at every start, so a restore onto another machine is caught and a restore onto the same machine isn't. An identifier that can't be read now, or was recorded as unreadable, counts as different, the safe side. The second trigger catches the same-machine restore: a heartbeat reports a floor above the hub's own counter for a repository. Both put the hub in recovery mode, which it says on its pages and in its log.
- **A report counts only with its proof.** A heartbeat carries, for each repository, the floor and the hub-signed entry that set it. The hub acts only on a floor its own key chain signed and logs any other, so a machine that claims a huge floor can't hold the hub in recovery or leave it no epoch to issue.
- **Recovery grants nothing, and backups go on.** The hub serves status, runs no maintenance of its own and renews and grants no lease until it leaves recovery mode. Machines keep backing up, as decision 88 has them do when the hub is missing, and keep the leases they hold until those expire.
- **How it ends depends on the machine.** On the same machine, a person's confirmation with the step-up decision 120 asks for approving a machine is enough, and it asks whether the old hub is stopped or gone. On a different machine, or one whose identifier can't be read, only the recovery handover ends it, with the recovery seed (`hub-recover`), which asks the same and records this machine's identifier. It makes a fresh key that machines follow, so a hub copied with the old key files can't keep granting under the old key. A confirmation alone is never enough there.
- **After any start, an owner changes only once every machine has reported.** After a restore, a copy or an ordinary restart, and outside recovery mode as well, the hub grants no lease to a machine that doesn't report holding it, moves no repository to another owner, renews only a lease its holder reports and runs no maintenance of its own on the repository. That holds until every machine the registry listed as using it at the start has sent a heartbeat to this hub since the start, or 7 days, the lease length, have passed since the start. Removing a machine in the meantime doesn't end the wait. A restored hub can't see the rewind until a machine reports, and a lease the hub renewed before it can run that long. The epoch floor is the second line.
- **A recovered hub learns the epochs from the machines.** After it leaves recovery mode it issues epochs above the highest floor reported, and changes an owner only under the rule above (§7.3).
- **A move is a handover, so ownership never changes hands in it.** While the old hub lives, a paired machine is promoted: the old hub signs a handover to the new machine's own hub key, raising the generation by one as decision 120 reserved. Signing it retires the old hub, which grants and renews nothing after, so there is one arbiter. The promoted hub starts with the epoch counters of the hub it replaces, and records its own machine identifier at its first start. Leases keep their owners.
- **Recovery is the move when the old hub is gone.** A backup of the data folder leaves the key files out (decision 120), so the person recovers with the recovery seed, which signs the recovery handover to a fresh key. A hub made new after "What losing both costs" has counters behind every floor and enters recovery mode once. A repository removed and added again keeps its counter, so it only rises.
- **No private key is copied.** Every machine already follows the chain of handovers from the key it pinned, so none pairs again and the old key is trusted nowhere afterward. A file copy of the registry would carry no fence of its own and would rely wholly on the floor and recovery mode.
- **The hub accepts a key on disk that its chain reaches.** A hub whose recorded key fingerprint is older than the key file, as after the database is restored beside a newer key, accepts a key that its `hub-handovers` chain reaches from the recorded fingerprint. Any other key is `hub-key-mismatch`, as decision 120 has it.
- **No signed export or import bundle in M9.** It looks like the simple move but copies the registry, fences nothing and holds the move's safety to the two rules above. Promotion and recovery cover the moves.

## What is not decided here

The move's own steps, its freeze of the hub, its page and its command line are built with the move, in M9 or M10. They add no way to hand over without the rules above. #571's leases build on this decision. The generation, the machine identifier check, recovery mode, the epoch floor, and the heartbeat's reports of the floor and of the lease a machine holds (repository, epoch, expiry and the hub-signed entry) are in the first protocol that carries leases, since a field a machine relies on for safety can't be added later as an optional one (decision 88, item 9).

## Scenario

These scenarios join M9's `maintenance/ownership.feature`. The first is the issue's, adapted: a grant below a machine's floor still reaches it, and the floor is the second line after the start rule.

```gherkin
  Scenario: A machine refuses a grant from a hub restored to an older epoch
    Given a laptop holds the lease on repository R at epoch 7
    When the hub is restored from a backup where R's last epoch was 4, at the same generation
    And a grant of R at epoch 4 reaches the laptop
    Then the laptop refuses the grant as "epoch-lower"
    And the laptop keeps its lease until it expires
    When the laptop's heartbeat reports its floor of 7
    Then the restored hub enters recovery mode
```

```gherkin
  Scenario: A restarted hub moves no owner before the machines have reported
    Given a laptop holds the lease on repository R and a NAS also uses R
    When the hub is restored from a backup and starts
    And the hub is asked to grant R to the NAS
    Then the hub refuses until the laptop and the NAS have sent a heartbeat since the start, or 7 days have passed

  Scenario: The laptop's lease is still renewed after the restart
    Given a laptop holds the lease on repository R
    When the hub starts and the laptop's heartbeat reports that lease
    Then the hub renews it

  Scenario: A hub copied to another machine needs the recovery handover
    Given a hub copied with its key files to a machine with another machine identifier
    When the hub starts
    Then it enters recovery mode
    And a person's confirmation doesn't end it
    When the person recovers the hub with the recovery seed
    Then it leaves recovery mode under a fresh key and records this machine's identifier
    And the next start on this machine isn't recovery mode
```

## Rejected

- **The generation floor only:** it misses the same-generation restore above.
- **"A restore means rebuild and re-pair":** no fence holds while two copies of the hub run, and every machine's pairing is lost for it.
- **The machine identifier check alone:** it misses a restore onto the same machine.
- **Raising the generation on a restore:** a hub restored onto its own machine can't tell it was restored, so nothing would raise it. The epoch floor covers that restore.
- **Refusing only `/sync/v1` on a different machine,** as a missing key does today: the hub would still grant, which is the harm.
- **A person's confirmation alone on a different machine:** a copy with the key files would grant under the old key beside a running old hub.
- **A signed export and import bundle in M9:** see "Why".

## What it costs

- **A transfer of ownership after any hub start waits.** A repository moves to another owner only after every machine that used it has sent a heartbeat since the start, one heartbeat interval at best, or, with a machine offline, up to 7 days. The hub's own maintenance on a repository waits the same. Renewals of leases their holders report, and backups, go on.
- **A hub whose machine identifier can't be read** enters recovery mode at every start, so each restart needs the recovery handover. Each one makes a new recovery seed and Recovery Kit, voids pairing tokens and adds to `hub-recoveries`, which keeps 8 (decision 120). That is the price of the safe side.
- **A copy that is recovered while the old hub still runs** leaves two hubs for a time: machines that followed the recovery handover trust the new key and the rest the old one, so two owners are possible. Whoever recovers answers the question above.
- **A machine paired after the backup** is unknown to the restored registry and isn't waited for. It can hold a lease the restored hub doesn't know of until that lease lapses.
- **A leaked key** can sign a grant that raises a machine's floor to the maximum, which the hub then can't issue above for that repository. This is accepted: the floor never resets, and it fails safe, never two owners. Backups go on. The machine recovers with a new data folder, which pairs again as a new machine: the old record, which holds its hostname, is removed first and leaves a tombstone, and its snapshots are adopted (decision 120, "A copied data folder" and a lost data folder).

Amends decision 120's scope: a move of the hub and a restore are decided here, no longer open.
