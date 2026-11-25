# Decision 161: A prune asks the daemon before it deletes, and deletes nothing without a yes

**A prune works out its plan, then asks the daemon, with a `ready_to_delete` line on its stdout, whether it may delete. The daemon answers with one line on the prune's stdin, which stays open for it, after checking again, as at the run's start, that this machine may maintain the repository. The prune deletes, with the plan it worked out, only on `delete`. On `delete_nothing`, a closed stdin, a line that is no answer, or no answer within 60 seconds, it deletes, marks and rewrites nothing and fails with `maintenance-lease-lapsed`.**

## Why

[Decision 151](151-a-paired-machine-maintains-only-under-its-hubs-lease.md) checks the lease when a run starts. A prune works out its plan for minutes on a large repository, so one started just before its lease ends could delete after the hub's hour of grace, when another machine may hold the repository (I1, #1482). The check before deleting closes that gap where deletion begins.

- **The daemon decides.** The worker knows nothing of hubs or leases. It asks, and the daemon answers from `MaintenanceLeases::permits`, the check a run's start makes: a paired machine may while it holds a live lease on the hub's time, and one that isn't paired always may.
- **Checked, not renewed.** The question changes no lease, so a prune never lengthens the time a machine holds a repository.
- **The plan isn't worked out again.** On yes, the prune goes on with the plan it worked out. On no, it ends; the next prune, under a new lease, plans afresh.
- **Fail closed.** Anything but `delete` deletes nothing, whether the answer is missing, late or malformed, or the stdin is closed. The worker reads one line of at most 64 bytes on a thread of its own and gives up after 60 seconds on the monotonic clock, so a daemon that stalls can't hold it, nor let it delete long after the check.
- **On the event stream.** The question is an `EngineEvent` that carries its answer, not a method of the job's control. Every consumer that doesn't know it drops it, and a question dropped unanswered answers no, so only the maintenance run, which checks the lease, lets a prune delete.
- **Only a prune asks.** The daemon keeps stdin open for a prune alone, and fails a worker that asks outside a prune, or twice, as one that broke the protocol. The job spec is one line, so what follows it is the answer.

## Consequences

- A prune whose lease lapsed while it planned fails with `maintenance-lease-lapsed`, its plan's work lost; it deletes nothing, so no other machine's backup loses a pack it needs.
- A deletion already begun finishes: the answer is given once, before the first delete. The gap between the check and the first delete is the time to write and read one line. I2's delayed deletion protects the packs the prune marks.
- The running-backup guard, the owner prune guard of §7.3's long-backup guard, still waits on the heartbeat feed (#573, #1463): this check stops a prune from outliving its lease, not from deleting packs a running backup still needs.
- An answer lost because the daemon stalled for over 60 seconds fails a prune that may have been allowed to delete. It runs again at the next schedule or request.
