# Decision 8: Linking snapshots to plans

**Tags `ladderback-plan:<id>` and `ladderback-agent:<id>`** on every backup. Untagged snapshots match by `(host, label, paths)` only for adoption (§7.3).

**Amended 2026-11-14 (the maintainer, #286):** a snapshot whose two `ladderback-agent:` tags or two `ladderback-plan:` tags carry different text belongs to no plan, whichever tag comes first and even when one names this agent. No plan's forget selects it; it is listed as orphaned for the person to decide (I6). Tags are compared exactly; the same tag twice is no disagreement.

**Amended by [decision 163](163-the-wire-and-stored-names-say-machine.md)** (2026-11-25, #1220): the tag that names the agent is `ladderback-machine:<id>`, where it was `ladderback-agent:<id>`; a snapshot that keeps the old tag is orphaned until it is retagged.
