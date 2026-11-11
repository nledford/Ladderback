# Decision 20: MCP server

**`ladderback mcp`: newline-delimited JSON-RPC 2.0 over stdio (MCP `2025-06-18` or later), in one inbound adapter (`adapters/mcp`).** Connect mode drives a running agent through `/api/v1` (a hub from M4b) with a scoped API token; headless and scratch modes run their own in-process session under the data-dir lock. Tools read state, run and cancel backups, and preview forget and import. Nothing restores, deletes, edits config or adds a command: a model-driven client is not a human confirmation, and token scopes enforce this in the daemon (§4.6, §10.5).
