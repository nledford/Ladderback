## 10. Security

### 10.1 Web UI

- Binds to `127.0.0.1` by default.
- **Auth is optional**, as in Backrest (§16, decision 11). When enabled: a single admin user with an argon2id password hash, `HttpOnly`/`SameSite=Strict` session cookies lasting 24 hours (§4.5), and CSRF protection on mutating routes.
- **API tokens** (M3), for non-browser clients such as the CLI and MCP connect mode, when auth is enabled. A token is `stdg_<id>_<secret>`: the daemon looks it up by `id` and checks the secret against an argon2id hash (§9), so a request costs one hash. Each token carries scopes, `read`, `run` and `preview`, checked by the daemon per route. Restore, download, config, pairing and auth routes accept only a session (with step-up where §10.4 requires it), never a token. Tokens are created, listed and revoked in Settings, and the secret is shown once.
- **Request checks, always on**, auth or not. The UI can set hooks and `password-command`, so an unauthenticated API reachable from a web page would let any site run commands on the machine (DNS rebinding or cross-site POSTs). Therefore:
  - the `Host` header must be a configured name or a loopback address;
  - a request that carries an `Origin` header must match the UI's own origin, and mutating requests must use `Content-Type: application/json`, which forces a CORS preflight that the daemon refuses. Requests without `Origin` (curl, scripts, the `steading` CLI) aren't affected by this rule;
  - the `/api/v1/events` WebSocket handshake checks `Origin` the same way. `/sync/v1` is exempt: agents authenticate with their signed nonce (§7.1).
- **Local users.** Loopback is reachable by every user on the machine, so with auth disabled any local user can set hooks and run commands as the daemon's user. On multi-user machines, enable auth.
- **Startup rule.** With auth disabled, the daemon refuses to start unless it binds only to loopback, or proxy-header mode is on with an allowlist of trusted proxy addresses. The first run with auth disabled shows a warning in the UI and the log.
- Optional "trusted reverse-proxy header" mode for SSO behind Authelia or Authentik.

### 10.2 Repository secrets

Every secret-bearing field can come from any of these sources. That covers the repository password and backend credentials in `options`, such as S3/B2 keys or SFTP passwords:

| Source | Config form | Typical use |
|---|---|---|
| Command | `password-command = "security find-generic-password -s rustic-nas -w"` (rustic-compatible) | macOS Keychain, `secret-tool`, `pass`, 1Password/Bitwarden CLIs |
| OS keyring | `password-keyring = "steading/nas"` | Desktops. The UI can write the entry for you. Not available to system services before login or to a root worker reading a user's keyring |
| File | `password-file = "/run/secrets/nas_password"` (rustic-compatible) | **Docker/Podman secrets**, systemd `LoadCredential=` (`$CREDENTIALS_DIRECTORY/…`), Kubernetes secrets |
| Environment variable | `password-env = "NAS_PASSWORD"` | Containers, CI, systemd `Environment=`/`EnvironmentFile=` |
| Inline | `password = "…"` | Accepted (e.g. on import) with a warning and an offer to move it to the keyring |

**Substitution in repository fields.** Inspired by Backrest's `${VAR}` expansion, but stricter. `repository`, `repo-hot` and the values in `options`, `options-hot` and `options-cold` may reference `${VAR}` (an environment variable) or `${file:/path}` (a file's contents, trailing newline trimmed). Substitution is limited to these fields: in hooks it would put secrets into process arguments (I4), and in `label`, `sources` or `tags` it would silently change snapshot identity. For example:

```toml
[repositories.b2]
repository = "opendal:b2"
password-file = "/run/secrets/b2_repo_password"
options = { bucket = "my-backups", application_key_id = "${B2_KEY_ID}", application_key = "${file:/run/secrets/b2_app_key}" }
```

Rules:
- `$${…}` escapes a literal `${`.
- Substitution happens **at job start** in the daemon, not when the config is loaded. Rotated secrets are picked up without a restart, and resolved values never get written back to `config.toml` or shown in the UI. The UI shows the reference (`${B2_KEY_ID}`), not the value.
- A missing variable or file fails the job with a clear error naming the reference. It never silently becomes an empty string (unlike Backrest).
- File permissions: Steading warns if a secret file is group- or world-readable, except for files under `/run/secrets/`, `$CREDENTIALS_DIRECTORY` and `/var/run/secrets/`, where container runtimes control permissions.
- rustic's own environment variables (`RUSTIC_PASSWORD`, `RUSTIC_PASSWORD_FILE`, `RUSTIC_PASSWORD_COMMAND`, `RUSTIC_REPOSITORY`, `RUSTIC_REPO_OPT_*`) are recognized on import. They're **not** read implicitly at runtime, so one stray variable can't redirect every plan. Use `password-env` explicitly instead.

Resolved secrets are passed to workers through the `JobSpec` on stdin, never as argv or environment variables. That holds even when a secret was *sourced* from an environment variable: workers and hooks get a scrubbed environment. The daemon and worker replace every resolved secret value with `***` in logs, warnings and errors before they are stored or forwarded, and hook and notification templates never receive secrets (Backrest leaks them this way, [#1170](https://github.com/garethgeorge/backrest/issues/1170)). **Secrets never leave the agent.** Hub maintenance and hub downloads need the repo password, so for hub-managed repos the **hub has its own secret entry** for that repo, configured on the hub. Agents don't push passwords.

### 10.3 Agent ↔ hub

Ed25519 identities, TLS (pinned or CA), and single-use short-lived pairing tokens. The protocol carries metadata, logs and restore requests, never file contents or secrets.

### 10.4 Restores

Restores are the most dangerous action because they can overwrite files.
- **Step-up confirmation** before any restore or download started from a web UI, whether local or on the hub. With auth enabled, re-enter your password; this is required even with a valid session, so an unattended open tab isn't enough. Password managers and paste work (A5). With auth disabled or in proxy-header mode there is no password to re-enter, so the user types or pastes the snapshot's short ID instead. That only guards against misclicks, not against someone at the keyboard.
- **Default destination is a new folder** on the target machine: `~/steading-restore/<host>-<snapshot-short-id>-<timestamp>/` (configurable per agent). Existing files are never touched in this mode.
- **From the hub**, only new-folder restores are possible, and only to agents that set `allow-hub-restore = true` (off by default). The agent runs the restore itself; the hub sends only the request.
- **Restoring in place** (original paths) is offered only in the agent's own UI. Its confirmation dialog states how many files would be overwritten or deleted, using a dry-run `prepare_restore` plan first.
- Every restore is recorded in the op log with who started it and from where (local UI or hub).

### 10.5 MCP server

An MCP client is usually a language model acting on text it read elsewhere, so `steading mcp` treats every call as untrusted input with no human behind it. The aim is to bound what a confused or prompt-injected model can do, not to stop a local user, who already has the API.

- **No step-up, so no restore.** Restores, downloads, applying forget or prune, adoption, config edits, hub pairing and auth settings have no tool (§4.6). A tool can't stand in for the confirmation in §10.4.
- **No new commands.** Hooks, `password-command` and secret sources can't be set or changed through MCP, so a prompt injection can't add a command run as the daemon's user (I4). Running a plan does run the hooks and secret sources a human already configured.
- **Enforced by the daemon, not the client.** With auth enabled, connect mode authenticates with an API token whose scopes (`read`, `run`, `preview`) the daemon checks per route (§10.1). A leaked token can't restore or edit config even through raw `/api/v1` calls. With auth disabled, the loopback API is open to local processes anyway, as it is to curl.
- **Token handling.** Read from `--token-file` (default `<config>/mcp-token`; a file readable by group or others is refused, as for `password-file` in §10.2), never from argv, and sent only over `https` or loopback.
- **Secrets stay references.** Results show secret references, never values. The `Redactor` is seeded with inline config secrets at load, not only with the values a job resolves, so `list_repositories`, `preview_import` and logs can't echo them. `export_profile` refuses inline passwords (§4.6). The I4 canary test covers every tool.
- **Headless isolation.** Headless mode takes the data-dir lock (§9). `--scratch` uses a fresh mode-0700 temporary directory, never the user's config or data.
- **Residual risk: untrusted output.** Logs, paths, tags, hostnames and rustic's error details are text a model reads, and an attacker can influence it. Steading doesn't sanitize it; the mitigation is that no tool can turn such text into a destructive action. Result size caps (§4.6) limit how much reaches the model at once.
