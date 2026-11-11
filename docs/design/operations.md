## 11. Notifications

Configured on the hub (or on a standalone agent). Each rule is **event → channel**:

- Events: `backup.started`, `backup.succeeded`, `backup.failed`, `backup.warning`, `backup.missed` (no success within N × interval), `backup.retrying`, `forget.failed`, `prune.failed`, `prune.refused` (`keep-delete` too small for the margin, §7.3), `check.failed`, `check.errors`, `agent.offline` (> N min), `repo.unmanaged_groups`, `repo.claim_rejected`, `repo.maintenance_paused` (lease not renewed, repo now shared, or not configured on the hub).
- Channels: generic webhook (JSON), ntfy, Gotify, Discord/Slack webhook, SMTP.
- **Templates.** Default message text comes from the `notify.ftl` catalog in the configured locale (§8.4). Users may override it with their own templates, which are not localized. Webhook bodies are JSON with stable, unlocalized keys, event names and error codes, plus a localized `message` field. Templates never receive secrets (I4).
- **healthchecks.io-style pings per plan**: `start`, `success` and `fail` URLs. These are the most robust "it silently stopped" detector, because they work even when the whole Ladderback system is down.

---

## 12. Packaging and deployment

- **Linux:** an `x86_64`/`aarch64` binary that needs glibc 2.35 or newer (Debian 12, Ubuntu 22.04 and later) and no library outside glibc and `libgcc_s`: TLS is rustls and D-Bus is built in (decision 54). `ladderback install-service [--user|--system]` writes a systemd unit (a user unit on desktops, a system unit on servers).
  - On **Bazzite and other atomic Fedora** systems, put it in `~/.local/bin` (or `/usr/local/bin`, which is writable) with a user unit. No layering needed.
- **macOS:** an `aarch64` binary until cargo-dist can build a universal one (axodotdev/cargo-dist#77; decision 54), signed with a Developer ID and notarized, with `install-service` writing a launchd agent plist. The worker needs **Full Disk Access** (TCC) to read `~/Documents`, `~/Desktop`, Mail and so on.
  - TCC ties the grant to the code signature, so an ad-hoc-signed build loses it on every upgrade. Signing is required, not polish.
  - Some macOS versions refuse to add a bare CLI binary to Full Disk Access ([Backrest #986](https://github.com/garethgeorge/backrest/issues/986)); M2 tests this on the current release and documents the workaround.
  - The UI detects permission errors in warnings and links to System Settings → Privacy & Security.
- **Docker:** an image for hub deployments on servers or a NAS.
- Distribution: GitHub Releases (cargo-dist), a Homebrew tap, and possibly the AUR later.

### 12.1 Versioning and releases

Ladderback follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html). The **release unit is the product**: the `ladderback` binary, and from M5 its Docker image. Every crate inherits one version from `[workspace.package]` and none is published, so crate APIs are not a public contract.

**The public contract**, which decides the bump:

| Surface | Breaking (major from 1.0) | Compatible (minor) |
|---|---|---|
| CLI: commands, flags, exit codes | Removing or renaming one, changing a default's meaning | New command or flag |
| CLI `--json` documents (§4.5): keys, error and warning codes | Removing or renaming one | New keys or codes |
| `config.toml` (§6.1) | Dropping the migration from an old `schema`, so a file stops loading | New keys; a new `schema` whose migration runs on load |
| REST and WebSocket API `/api/v1` | Anything that needs `/api/v2` | New endpoints and optional fields |
| Sync protocol `/sync/v1` (§7.2) | A new `protocol` number that the previous release can't speak | New optional message fields |
| Snapshot tags `ladderback-plan:` / `ladderback-agent:` (§7.3) | Any change: existing repositories would lose their plan links | — |
| Webhook JSON: keys, event names, error codes (§11) | Removing or renaming one | New events, keys or codes |
| Exported rustic profiles (§6.2) | A profile that the pinned rustic CLI reads differently | — |
| Database and data directory (§9) | A migration with no automatic path | Automatic migrations |
| MCP tools (§4.6): names, input schemas, result fields | Removing or renaming a tool, a new required argument, a result field whose meaning changes | New tools, optional arguments and result fields |

Not part of the contract: UI layout and wording, log text, the CLI's sentences (a script reads `--json`; the daemon's `listening on` line stays as §8.4 says), the worker protocol (both ends ship in one binary), and Rust APIs. Downgrades are unsupported; the config history and the daily database copy are the way back.

**Bump rules.** A release takes the highest bump among its changes, judged by the table, not by commit labels:

- before 1.0.0: a breaking change or a feature bumps the minor version, a fix bumps the patch;
- from 1.0.0: breaking bumps the major, a feature the minor, a fix the patch;
- every breaking change gets an entry under **Breaking** in `CHANGELOG.md`, with the upgrade step.

**Release line.** Nothing is released before M1b; the workspace version stays `0.0.0` until then. The end of M1b releases `0.1.0`, and each later milestone a minor version. `1.0.0` follows M5, once the contract above is declared stable. Release candidates (`X.Y.Z-rc.N`) are tagged before a release that bumps the config `schema` or the sync `protocol`, and run on real machines first. The first release, release candidates and `1.0.0` are versions the commits can't imply, so the maintainer sets them on the release PR's branch with `release-plz set-version <version>`, which moves `[workspace.package] version`. The `ladderback@<version>` form would give that one package its own version and break the inheritance every crate shares.

**Tags.** `vX.Y.Z` on `main`, created only by the release workflow. A tag is never moved or deleted; a bad release is fixed by the next patch, and its GitHub release is marked as such.

**Process.** [release-plz](https://release-plz.dev) in git-only mode (`release-plz.toml`, `.github/workflows/release.yml`):

1. Running the Release workflow by hand opens or updates a draft **release PR**. It bumps `[workspace.package] version` from the Conventional Commits since the last tag (`feat` → minor; `fix` → patch; `!` or a `BREAKING CHANGE:` footer → breaking) and writes the `CHANGELOG.md` entry. `just release-preview` shows the same result locally, changing nothing.
2. The maintainer checks the bump against the contract table, since labels only navigate, and edits the notes. `just release-pr [version]` does steps 1 and 2: it runs the workflow, waits for the PR, sets the version if given, opens `CHANGELOG.md` in `$EDITOR`, then pushes to the PR's branch and retitles the PR. A later run of the workflow, the recipe's included, closes a release PR that carries such a commit and opens a new one from the commits alone, so a rerun needs the same version again and the notes redone.
3. Merging the PR to `main` triggers the tag and a GitHub release with that entry as its notes.

Until M5's release packaging, a release is source only. M5 adds the hosting: the cargo-dist archives attached to the same tag, the signed macOS build and the Docker image.

**Release builds.** The archives are built, and not published, from M2 on (§16, decision 54). `just release-build <target>` builds the web UI with `release_flags` and then runs cargo-dist's `dist build` (`dist-workspace.toml`; `scripts/install-dist.sh` installs the pinned dist) for one target: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` or `aarch64-apple-darwin`. The archive, its SHA-256 and dist's manifest land in `target/distrib`, dist's own folder, whatever `CARGO_TARGET_DIR` says. `just smoke-release <binary>` checks the binary (`scripts/smoke-release.ts`): its version; no pseudolocales in the binary, asked for or looked for, and neither pseudolocales nor the gallery in the wasm it serves; its dynamic libraries, which must be the base system's; on Linux its glibc floor, at most 2.35; then `serve` in scratch folders answering with the UI, a scratch repository initialised and one backup that leaves one snapshot. With `--container` it runs those in clean Debian 12, Ubuntu 22.04, Ubuntu 24.04 and Fedora containers (Docker, no network, images pinned by digest). The `Release builds` workflow (`.github/workflows/release-builds.yml`) runs both for each target every night, by hand, and on pull requests labelled `release`; the Linux builds run on Ubuntu 22.04, so the binary needs no newer glibc than 2.35 and runs on Debian 12 and Ubuntu 22.04. Each archive is a run artifact for 7 days, and a failed nightly run opens an issue. It is not a required check.

**Release build flags.** A release is built with `--no-default-features`, which leaves out the development pseudolocales (§8.4); the component gallery (§4.5) is off unless asked for, so the same build has none. The flags are one definition, `release_flags` in the `Justfile`, and `just --evaluate release_flags` prints them. `just architecture` checks a build made with them: no `fluent-pseudo`, no gallery. The release builds use them: `just web-release` passes them to `trunk build --release`, and cargo-dist, which can't read the `Justfile`, has the same in `dist-workspace.toml`, which `just architecture` checks against `release_flags`. The development recipes (`just web`, `just build`) don't use the flags. The Docker image M5 adds must, so the check and the artifact can't drift apart. A feature a release needs is added to `release_flags` and `dist-workspace.toml`, not to a workflow.
