# Decision 134: How an AI agent reads the daemon's log

**The MCP tool `read_log` reads the recent log (§3.1) newest first, a page at a time: at most 200 lines and 32 KiB of text a call, each line numbered since the daemon started, so a page reads on from the last line of the one before. It narrows the lines by level, as the Log page does, and by a time range, an operation or a plan they name. Through a daemon it reads `GET /api/v1/log`, so it needs the permission Daemon log (decision 127); a session of its own reads its own log. The server filters and pages; the API route is unchanged but for the number.**

## Why

When a backup fails, an agent helping someone needs the log around it (#957). The recent log holds up to 1,000 lines and 1 MiB, most of a model's context at once, and its lines can be long, so a read needs a cap on both. A page cut short has to say so, or the agent takes a partial answer for the whole. The Log page already reads the log at the permission Daemon log, and its lines are masked as the log masks them (I4), so the tool adds no new way in.

## Decided

1. **The gate.** In connect mode the tool reads `GET /api/v1/log`, whose permission is Daemon log at read (decision 127). A token without it is refused `403 api-token-permission` naming `daemon-log`, handed on as the tool's error; with sign-in off, the `[without-token]` table decides, and its defaults leave the log closed. Headless and scratch sessions read their own recent log, fed by their own logger from their start, and have no gate, as their other tools have none.
2. **The lines.** Each line is rendered by `app::recent_log::line_dto`, the function the API answers with, so a line reads the same through the tool, the API and the Log page.
3. **The caps.** `limit` defaults to 50 and is clamped to 1..200, as the `list_*` tools' paging is. A page holds at most 32 KiB of source and message text (`domain::daemon_log::READ_MAX_BYTES`), about 8,000 tokens, more than 200 lines of usual length take, so this cap binds only when the lines are long. The answer is larger: its JSON adds each line's time, level and number, about 80 bytes a line, escapes control characters, and the result carries the page twice, as text and as structured content. A line is at most 4 KiB of message and 256 bytes of source, so a page always holds the first line it keeps.
4. **What a page says.** `stopped` is `lines` or `bytes`, the cap that cut it short, or null on the last page; `next` is then the number of its last line, and null on the last page.
5. **The cursor.** `LogLineDto` gains `number`: the line's place among the lines the recent log has kept since the daemon started, from 1. `before` reads the lines numbered below it. New lines don't shift a page, as an offset would. A line the log has let go of is gone, and a restart starts the numbers again.
6. **The filters.** `level` keeps that level or worse (`error`, `warn`, `info`, `debug`, `trace`), as the Log page's choices do: its "Warnings and errors" is `warn`. `since` and `until` are RFC 3339 instants, each end included. `operation` keeps the lines whose message holds the operation's ID. `plan` keeps those saying `plan <id>` with the ID ending there: at the message's end or before a character no plan ID holds, so `home` doesn't match `plan home-2`. A value that can't be one is `invalid-arguments` naming its argument, never echoing it.
7. **Where it is decided.** `domain::daemon_log` holds the filter (`LogFilter`) and the read (`read`), the numbering on `RecentLog`. The MCP server reads the backend's whole recent log and applies them, so both modes read alike, and the HTTP route keeps no query of its own.

## Cost

- The Log page keeps its own level filter in `crates/web`, which may depend only on `api` and `l10n` and so can't call the domain's. The two rules are a match of five levels, each tested where it lives; an `@e2e` scenario compares a `warn` read with the API's error and warning lines, the rule the page's "Warnings and errors" states, not the page's code.
- In connect mode each call reads the whole recent log, at most 1 MiB, to answer one page.
- `plan` matches text, not a plan's record: a line naming a plan in other words than `plan <id>` isn't found, and a line whose words happen to follow "plan " is found for a plan of that ID, as "A plan dry run's walk failed." is for a plan `dry`.

## Rejected

- **Query parameters on `GET /api/v1/log`:** a second filter beside the Log page's, and headless sessions would still need one in the MCP server.
- **An offset as the cursor:** the newest lines come first, so each new line would shift every page after it.
- **A cap on lines alone:** 200 lines of 4 KiB each would be most of a megabyte.
- **A permission of its own:** decision 127 already names the daemon log among what a permission grants, and the Log page reads it at that permission.
