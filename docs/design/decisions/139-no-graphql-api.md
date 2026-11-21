# Decision 139: No GraphQL API

**Ladderback doesn't add a GraphQL API.** The web UI keeps the `/api/v1` routes and the events stream, typed by the wire types in `crates/api`. AI agents keep the MCP tools (decision 20), and machines and the hub keep the signed sync messages (decision 121). The question is reopened only when a real user needs a query those can't serve.

## Why

- **One first-party client, sharing its types with the server.** The web UI and the daemon compile against the same request and response types in `crates/api`, so a change to the contract fails the build. GraphQL would replace that with a schema the client checks only at run time, or with generated code. When a page needs a different shape of data, its route's response type changes in the same pull request.
- **Each other client already has the interface that suits it.** An AI agent expects MCP, and each tool is a reviewed slice of data behind a token permission (decision 127). The hub and its machines exchange signed messages, not queries. Live updates come through the events stream.
- **Every field would become reachable (I4).** A route returns only what its response type holds, so a review of the type is a review of what can leave the daemon. With GraphQL, each field and each path to it would need proof that it can't carry a resolved secret.
- **Permissions would move to every field.** A token's permissions, such as Snapshot contents, gate whole routes and tools today. Under GraphQL the same check would be needed on every field and nested path, where a missed one fails silently.
- **Queries are hostile input (§13.5).** Depth, breadth, aliases and batches all size the work a request causes. Each would need a limit, or one request could keep the daemon, which runs backups unattended, busy without bound.
- **Errors are typed codes (§8.4).** The core returns error and warning codes that `l10n` turns into text. GraphQL's error list carries free text and partial results, which fits that poorly.
- **A second API style to keep.** It would need a new dependency, and the routes, their tests and their documentation would exist twice while both ran.

## Rejected

- **GraphQL beside the routes:** every cost above, plus two contracts to keep in step, for no client that needs it.
- **GraphQL in place of the routes:** a rewrite of every route and its tests for the same data the UI reads today.
- **A read-only GraphQL endpoint on the hub for outside dashboards:** no user has asked for one. A few REST routes or MCP tools shaped to a real request cost less and keep the per-route review.

## What this doesn't cover

- **Outside integrations with the hub (M8 and later).** If people build dashboards across many machines and the routes and MCP tools can't express what they need, that evidence reopens this decision.
