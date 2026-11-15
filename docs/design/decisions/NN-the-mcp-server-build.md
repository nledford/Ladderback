# Decision NN: The MCP server as built

**`ladderback mcp` is built on the official Rust SDK, `rmcp` 3.10, pinned, inside `adapters/mcp` alone; its tools' output schemas come from the `api` types through `schemars`; there is no `operation_log` tool for now; connect mode gets two new HTTP routes so it offers every tool; and a `read`-scope token may read the daemon's build** (recorded 2026-11-14, §4.6, §10.5; decided by the maintainer on #517). It amends decision 20, which planned the protocol and the tool list before revision `2026-07-28` existed, and decision 93, whose `GET /api/v1/session` was for a session only.

## Why

- **`rmcp`, not a protocol layer of our own.** MCP revision `2026-07-28` removed `initialize` and `ping`, added `server/discover`, a revision named in each request's `_meta`, a required `resultType` and new cache hints. Clients in use speak both eras, and `rmcp` 3.10 serves both: it negotiates `initialize` for `2025-06-18` and `2025-11-25`, answers `server/discover`, checks each request's revision against the ones we serve, and strips `resultType` for older peers. A layer of our own would be about a thousand lines that follow every revision by hand. It is the official SDK (Apache-2.0, MSRV 1.88) and adds four crates to the lockfile. The cost is churn: three majors in five months, so it is pinned exactly (`=3.10.0`) and upgraded in a change of its own. It logs through `tracing`, which nothing here subscribes to, so its own diagnostics are not seen.
- **Revisions `2025-06-18` to `2026-07-28`.** The oldest is the first with `structuredContent` and `outputSchema`, which every result relies on. A handshake asking for an older one is answered with the newest revision that has a handshake, as the protocol says, and the client then leaves; a request naming an older revision in its `_meta` is refused.
- **Output schemas from the `api` types.** `schemars` derives them, behind a feature of `api` that only `adapters/mcp` turns on, and a golden `tools/list` file makes every schema change visible in review. A schema written by hand would drift from the type it describes; no JSON-schema validator is needed, because the schema and the value come from the same type.
- **No `operation_log`.** No per-operation log exists: the "operation log" is the history database. `get_operation` carries the operation's failure `{code, args, detail}` and its warnings, which is what a log would mostly have been read for. A known limit, until such a log exists.
- **Two new routes, so both modes offer every tool.** `preview_import` and `export_profile` had no route: `POST /api/v1/profiles/preview` (scope `preview`) previews a profile's text without writing, and `GET /api/v1/plans/{id}/profile` (scope `read`) exports a plan. Without them connect mode would offer fewer tools than headless mode, and the parity suite would have to know which.
- **A `read` token reads the build.** `status` names the daemon's `version`, `commit`, `dirty` and `release` as fields of their own (decision 90), so an agent never parses `0.3.0+3f937cf`. Over `--connect` with sign-in on, that needs `GET /api/v1/session` open to a `read` token; the build says which fixes a daemon lacks, which a token holder may know, as a signed-in page may.

## Decided

1. **Placement:** one crate, `adapters/mcp`, an inbound adapter beside `http`. Its tools call the `Backend` trait the crate defines; connect mode's implementation is an HTTP client that depends on `api` alone (§3.2).
2. **Stdio:** an incoming message over 1 MiB is dropped unread and the session goes on; standard output carries protocol messages only; the session ends when standard input closes.
3. **`status`:** this program's build, then in connect mode the daemon's build, sign-in state and exposure, each plan with its next run and last operation, and the operations queued or running among the newest 200.
4. **No answer from the daemon,** or an answer that isn't the API's, is the tool error `daemon-unreachable`; a refusal from the daemon is handed on as its own `{code, args}`.
5. **`--connect`** takes an `http` or `https` address of a host, with no user name, password, query or fragment: a password there would be on the command line.

Left out: proposal tools (#518), the hub's `agent` argument (M5), and a per-operation log.
