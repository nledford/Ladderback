### 4.3 Hub

This section decides what the hub adds to what an agent has.

Everything an agent has, plus:

- **Agent registry**: agent ID, identity public key, hostname (unique), version, last seen, online status, clock offset.
- **Aggregated op log.** It receives operation records and summaries from agents. Raw logs stay on the agent and are fetched on demand.
- **Dashboard** (built in part; #569, #1440, [decision 149](../decisions/149-the-hubs-dashboard.md)). `GET /api/v1/hub/machines` answers each approved machine, oldest approved first: when it last connected, its time zone when it told one, how many of its reports the hub let go of past the cap, its 10 latest operations, newest first, folded from its 40 newest reports, its plans as it last reported them, and the latest progress of the operation it runs (§7.2). The hub tells its browsers when a machine starts waiting for approval, is approved, reports an operation, its plans or a progress (`AgentPending`, `AgentApproved`, `AgentReported`, `AgentPlans`, `AgentProgress` on `/api/v1/events`), only on a session's stream or the browser's with sign-in off, never an API token's or an AI agent's.
- **Plan registry.** Agents publish their plans (minus secrets), so the hub knows the retention policy for each `ladderback-plan` tag.
- **Repo ownership registry.** It maps each rustic repository ID to its single maintenance owner (I1, §7.3).
- **Maintenance runner** for **hub-managed repos** (§7.3): forget, prune and check, serialized per repo by `OperationStore::try_start` (I7).
- **Downloads.** For hub-managed repos, the hub opens the repository with its own secret and streams `dump` output to the browser. No file contents cross the sync protocol.
- **Notifications** (§11).
- **Remote control** (milestone 5): "run now", cancel, and restore into a new folder on agents that allow it (§10.4). Plans are **view-only from the hub** in v1. Editing them from the hub is a post-v1 feature, opt-in per agent (§16, decision 2).
