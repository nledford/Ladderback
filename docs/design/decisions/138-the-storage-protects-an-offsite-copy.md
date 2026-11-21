# Decision 138: The storage, not Ladderback, keeps an offsite copy that a machine that backs up can't delete

**The storage enforces it. For a REST repository the machines that back up use rest-server with `--append-only`, and a second endpoint without it is held only by the repository's maintenance owner (I1). For S3 or B2, Object Lock with versioning keeps the objects, in governance mode, which Ladderback recommends and doesn't set up. A repository's `protection` setting, `append-only`, `object-lock` or `none`, says which, and the UI shows "immutable" only once a delete probe confirms the storage refused a test delete. A machine whose chosen location refuses deletes is refused forget, prune and adoption as `maintenance-write-only` before any delete call. The unrestricted credentials come through §10.2's secret sources, rustic's own `append_only` isn't relied on, and the hub may not host the unrestricted endpoint of a repository it doesn't own** (recorded 2026-11-21, §7.3, §10.2, §16; the maintainer's decision on #1056).

## Why

- **A flag in the repository can't bind a machine that has the password.** rustic_core 0.14.0 refuses `delete_snapshots` and prune when the repository's config says `append_only = true`, but that file is the repository's own, so any client with the password can turn it off. Ladderback doesn't rely on it.
- **Only the storage can refuse a machine that backs up.** Credentials the storage doesn't hold to a role leave the decision with the client. rest-server's `--append-only` refuses at the server, and rustic's REST backend doesn't retry a 4xx, so the refusal fails fast.
- **Two endpoints, because rest-server has no per-user append-only** (restic/rest-server issue 74). The usual setup is two instances on one storage path, or a proxy route with its own credentials, the unrestricted one held by the maintenance owner alone. Prune still never deletes instantly (I2): nothing here offers `instant-delete`.
- **Object Lock keeps the data rather than refusing the call.** rustic deletes by name, with no version, so on a versioned bucket a delete only adds a marker (S3) or hides the file (B2) and succeeds, while the locked version stays until its lock ends. No machine is write-only there, and the owner's forget and prune run as usual. Space returns only when a lifecycle rule drops old versions after their locks lapse. The guide gives the settings the backend needs for such a bucket.
- **Governance mode, not compliance.** Compliance mode can't be undone, even by the account owner, so a wrong period or bucket would stay. Under governance, an identity with the bypass permission can delete a locked version with a header rustic doesn't send: that is for the person's own tools. Ladderback holds no credentials for bucket settings and doesn't change them.
- **A lock is time to notice and recover.** It protects an object for its period from its upload, so a pack that a long-kept snapshot still uses ages out of it. The guide says the period must outlast the keep window plus the prune interval, and that the backing-up machines' keys shouldn't be allowed to delete versions or bypass the lock.
- **A setting says what is meant; a delete probe says what is true.** The probe is new, apart from decision 80's probe of a location. For REST it deletes a made-up name under `snapshots/`: 403 is refused, and 404 or success means the machine can delete. It never uses `locks`, which rest-server lets any client delete. For object storage it tries to delete one version of a probe object it wrote outside rustic's folders. Any other answer is unknown and confirms nothing. Where the object lives is settled when built.
- **"Immutable" means the machines that back up were refused.** The 3-2-1 view (#1053) says whose probe it shows. The UI never shows "immutable" from the setting alone.
- **A refusal before any delete call, rather than a failure halfway.** A machine is write-only when the location its job chose (decision 80, both halves of a hot and cold location) refuses the delete probe, which runs at the start of a forget, prune or adoption on a repository of `protection = "append-only"`. Adoption removes the snapshots it rewrites. The refusal comes after `maintenance-not-owner`, is recorded and ended once as any refused operation is (decisions 41 and 42), and leaves a check running, since a check reads only.
- **The unrestricted credentials are secrets (I4).** They come through the sources of §10.2 on the maintenance owner and never cross the sync link. When a lease moves, the guide has the old owner's credentials revoked: a second party able to delete is a second owner (I1). The hub may not host the unrestricted endpoint of a repository it doesn't own; Ladderback can't see where an endpoint runs, so this is guidance, not a check.
- **A copy counts as protected only if no machine that backs up can reach its storage through the file system.** #1052's copy plans write the copy. A copy whose storage the hub, or any machine that backs up to it, can reach through the file system never counts as protected in the 3-2-1 view, whatever its endpoint mode: that machine can delete there directly, past any endpoint. The hub is a machine too, so a copy on the hub's own disk is not protected. Ladderback sees this in a machine's locations, a local path or a mounted folder. Where it can't (a REST endpoint served from a machine's own disk), the person says so, in a form settled with #1053.
- **rustic doesn't honor restic's lock** (§7.3, I5), so a `restic prune` against the same repository can delete live packs. The guide says so beside each setup.

## Scenario

This scenario joins M9's `maintenance/ownership.feature`.

```gherkin
  Scenario: A machine whose chosen location can't delete is refused a forget
    Given this machine maintains a repository with protection "append-only"
    And the only location this machine reaches it by is write-only
    When the person runs forget on this machine
    Then it is refused as "maintenance-write-only" before any delete call
    And no snapshot is removed
```

## Rejected

- **Credentials per role alone:** the client still decides what it deletes. The guide keeps them as the other half of Object Lock.
- **rustic's `append_only` flag:** it is client-side and a client can clear it.
- **Compliance mode as the recommendation:** it can't be undone, even by the account owner.
- **Ladderback setting up the bucket or the server:** it would need credentials that can change who may delete, which it should never hold.
- **The hub hosting the unrestricted endpoint for any repository:** see "Why".
- **Closing the issue:** it leaves the immutable copy of 3-2-1-1-0 unmet.

## What this doesn't cover

- **Snapshots added by a compromised machine.** It can add snapshots with a plan's tags and later times, so the owner's forget keeps them and lets the real ones go, and no delete is refused. restic's documentation of append-only mode describes it. The preview of each forget (§7.3) is the check.
- **A machine that backs up and also owns.** Its location for maintenance can delete, so the protection holds against the other machines only. Running forget and prune on the server, outside Ladderback, with `maintenance = "none"` everywhere, is the other route.
- **A machine that reaches the storage by file system.** The hub is not forbidden to hold the storage path: the copy just isn't counted as protected (see "Why"), and the guide says to keep a protected copy on storage none of the machines that back up can reach directly.

Cost: two endpoints to run and keep in step, and an owner that moves needs the unrestricted credentials set up on the new owner by hand: a machine granted the lease without them is write-only and refused. The setup guide for each option and a security review of the probe are not part of this decision, so #1056 stays open for them.
