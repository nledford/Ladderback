## 4. Components

This section decides how the work divides between the worker process, the daemon, the hub, the web UI and the MCP server, and how the web UI and the command line meet the accessibility invariants.

Each part is a file of its own:

- [4.1 Worker process (the engine boundary)](components/worker.md#41-worker-process-the-engine-boundary)
- [4.2 Daemon (agent)](components/daemon.md#42-daemon-agent)
- [4.3 Hub](components/hub.md#43-hub)
- [4.4 Web UI (Leptos CSR)](components/web-ui.md#44-web-ui-leptos-csr)
- [4.5 Accessibility](components/accessibility.md#45-accessibility), with each shared component's contract in a file of its own under `components/accessibility/`
- [4.6 MCP server](components/mcp.md#46-mcp-server)
