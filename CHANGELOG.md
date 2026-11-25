# Changelog

All notable changes to Ladderback are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Ladderback uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as described in
[docs/design/operations.md §12.1](docs/design/operations.md#121-versioning-and-releases).

## [Unreleased]

### Breaking

- The names on the wire, in the data folder, in snapshot tags and in the database say machine, not agent: the sync protocol's `agent_*` fields, the signed labels, the `agent-*` error codes, the hub's events and API fields, the `agent` field of the maintenance API's orphaned groups and adoptions (now `machine`), a failed hook's `level` value `agent` (now `machine`), the snapshot tag `ladderback-agent:` and the files `agent-id` and `agent-key` ([#1220](https://github.com/nledford/Ladderback/issues/1220), decision 163). The hub's machine registry moves to `/api/v1/hub/machines` and its dashboard to `GET /api/v1/hub/dashboard`, and the Machines tab of Settings to `settings#machines`. Nothing accepts the old names, so upgrade the hub and every machine paired with it together. On each machine:
  1. Stop the service.
  2. In the data folder (on macOS `~/Library/Application Support/ladderback`), rename `agent-id` to `machine-id` and `agent-key` to `machine-key`, both together: a `machine-key` without its `machine-id` makes the daemon take a new machine ID with the old key. Then delete any leftovers an earlier build staged beside them (`agent-key.*`, `agent-id.*.tmp`).
  3. Retag its snapshots with rustic: `rustic tag --filter-tags ladderback-agent:<id> --add ladderback-machine:<id> --remove ladderback-agent:<id> --dry-run`, then again without `--dry-run`. A snapshot left with the old tag is orphaned, can't be adopted from the UI, and is no parent for the next backup, which then reads its sources in full. Until no recent snapshot in a repository keeps the old tag, the repository counts as shared, so the hub withholds its maintenance lease: retag every machine that backs up to it.
  4. Start the service. The database migrates itself.

  Export rustic profiles again: one exported before filters on `ladderback-agent:` and is refused on import.

## [0.5.0](https://github.com/nledford/Ladderback/compare/v0.4.0...v0.5.0) - 2026-11-20

### Added

- Add a shared tabs component ([#921](https://github.com/nledford/Ladderback/pull/921))
- Show what a proposal would forget, before and after, on the Pending changes page ([#915](https://github.com/nledford/Ladderback/pull/915))
- Say on the Repositories page when the keyring can't be started on demand ([#946](https://github.com/nledford/Ladderback/pull/946))
- Add a shared alert component ([#964](https://github.com/nledford/Ladderback/pull/964))
- Give Success, Warning and Error a color pair every component can use ([#966](https://github.com/nledford/Ladderback/pull/966))
- Add vertical and responsive orientations to tabs ([#965](https://github.com/nledford/Ladderback/pull/965))
- Approve a proposal that could delete snapshots from its forget preview ([#971](https://github.com/nledford/Ladderback/pull/971))
- Show the Log and Plans pages' refusals as error alerts ([#984](https://github.com/nledford/Ladderback/pull/984))
- Choose a narrow or full-width page in Settings ([#950](https://github.com/nledford/Ladderback/pull/950))
- Draw the progress bar in the shared tone and size ([#987](https://github.com/nledford/Ladderback/pull/987))
- Add a shared banner component that remembers its dismissal ([#1005](https://github.com/nledford/Ladderback/pull/1005))
- Add a shared badge component ([#990](https://github.com/nledford/Ladderback/pull/990))
- Move the toast onto the shared look, with a tone, an icon and a Dismiss button ([#1007](https://github.com/nledford/Ladderback/pull/1007))
- Show refusals on the Snapshots page and in a snapshot's restore and download dialogs as error alerts ([#1018](https://github.com/nledford/Ladderback/pull/1018))
- Show the Settings sections as tabs, each with an address ([#1031](https://github.com/nledford/Ladderback/pull/1031))
- Show an operation that succeeded with warnings in the Warning tone ([#1028](https://github.com/nledford/Ladderback/pull/1028))
- Draw the checkbox and radio group from the tokens ([#1027](https://github.com/nledford/Ladderback/pull/1027))
- Show the Pending changes page's refusals as error alerts ([#1032](https://github.com/nledford/Ladderback/pull/1032))
- Propose, read and withdraw setup changes from an AI agent ([#1049](https://github.com/nledford/Ladderback/pull/1049))
- Draw every page link with the shared link ([#1017](https://github.com/nledford/Ladderback/pull/1017))
- Draw every page-wide notice with the shared banner ([#1033](https://github.com/nledford/Ladderback/pull/1033))
- Dry_run_plan counts what a draft plan would back up, within folders a person allows ([#1064](https://github.com/nledford/Ladderback/pull/1064))
- Publish .deb and .rpm packages with each release ([#1085](https://github.com/nledford/Ladderback/pull/1085))
- Show each page's first load in the shape of what is coming ([#1089](https://github.com/nledford/Ladderback/pull/1089))
- Settings lists and edits the dry-run allowlist ([#1077](https://github.com/nledford/Ladderback/pull/1077))
- Answer your own actions with a toast where the page shows nothing else ([#1090](https://github.com/nledford/Ladderback/pull/1090))
- Warn of missing Full Disk Access on every page ([#1087](https://github.com/nledford/Ladderback/pull/1087))
- Switch the Settings tab when only the address's fragment changes ([#1105](https://github.com/nledford/Ladderback/pull/1105))
- Add a shared Steps component ([#1095](https://github.com/nledford/Ladderback/pull/1095))
- Every table scrolls in a frame of its own, words whole, times as their words ([#1119](https://github.com/nledford/Ladderback/pull/1119))
- Show every page's messages in the shared alert ([#1143](https://github.com/nledford/Ladderback/pull/1143))
- A status summary for AI agents, with overdue plans and unreadable repositories ([#1130](https://github.com/nledford/Ladderback/pull/1130))
- Warn when an rclone-command gives a flag a credential as text ([#1162](https://github.com/nledford/Ladderback/pull/1162))

### Fixed

- Name a restore's and a download's row on the Operations page by its repository and its kind ([#943](https://github.com/nledford/Ladderback/pull/943))
- Time out a hook that can't get the spawn gate in time ([#988](https://github.com/nledford/Ladderback/pull/988))
- Say a locked keyring is locked, with how to unlock it ([#1010](https://github.com/nledford/Ladderback/pull/1010))
- Wrap an alert's words whole at 320 px with 200 % text ([#1012](https://github.com/nledford/Ladderback/pull/1012))
- Read a forget preview's repositories at once, in the listings' turns ([#1009](https://github.com/nledford/Ladderback/pull/1009))
- Line up the words of tabs in a column at its start ([#1021](https://github.com/nledford/Ladderback/pull/1021))
- Say the keyring is locked when it couldn't keep a typed password ([#1034](https://github.com/nledford/Ladderback/pull/1034))
- Make Warning badges and toasts read at least as strongly as Success, with Error stronger still ([#1061](https://github.com/nledford/Ladderback/pull/1061))
- Answer 404 for an API path no route serves, not the web UI's page ([#1074](https://github.com/nledford/Ladderback/pull/1074))
- Restart the installed service after another program stops it (run install-service again) ([#1088](https://github.com/nledford/Ladderback/pull/1088))
- Keep a badge's and an alert's words beside their icon ([#1084](https://github.com/nledford/Ladderback/pull/1084))
- Show "Loading…" just before the busy region, outside it ([#1102](https://github.com/nledford/Ladderback/pull/1102))
- Install-restart replaces the installed binary only once the daemon is idle ([#1118](https://github.com/nledford/Ladderback/pull/1118))
- Keep a snapshot's ID on one line in narrow tables ([#1149](https://github.com/nledford/Ladderback/pull/1149))
- Give the systemd service a 0077 umask ([#1158](https://github.com/nledford/Ladderback/pull/1158))
- Warn about a secret file other users can change, and refuse a key file others can read or change ([#1154](https://github.com/nledford/Ladderback/pull/1154))
- Log when the session store keeps failing to write ([#1168](https://github.com/nledford/Ladderback/pull/1168))
- Use rustic_core 0.14.0, where a pack a snapshot uses again is recovered one prune later ([#1155](https://github.com/nledford/Ladderback/pull/1155))
- Show the mcp command's description in the reader's language ([#1173](https://github.com/nledford/Ladderback/pull/1173))
- Escape control characters in refusals the command line prints ([#1174](https://github.com/nledford/Ladderback/pull/1174))
- Log when an API token's last use can't be saved ([#1176](https://github.com/nledford/Ladderback/pull/1176))

## [0.4.0](https://github.com/nledford/Ladderback/compare/v0.3.1...v0.4.0) - 2026-11-17

### Breaking

- MCP tools refuse a bad argument as a result the agent reads, take an operation as `operation`, and narrow their input schemas ([#751](https://github.com/nledford/Ladderback/pull/751)): an MCP tool call with an argument the tool cannot take
is now a tool result marked as an error (invalid-arguments), not
JSON-RPC error -32602.

* fix!: the tools that take an operation take it as `operation`

`get_operation`, `wait_operation` and `cancel_operation` took the
operation's ID as `id`, while `run_plan` and `cancel_operation` return
it as `operation`: an agent passing back what a run gave it was refused.
Each now takes it under the name a run returns it.

### Added

- Install from the release archive on Linux, building from source only as the fallback ([#607](https://github.com/nledford/Ladderback/pull/607))
- Scoped API tokens, created and revoked in Settings ([#608](https://github.com/nledford/Ladderback/pull/608))
- Icons and a favicon, with unhashed files revalidated ([#629](https://github.com/nledford/Ladderback/pull/629))
- Say in Settings when an admin password waits for a restart to turn sign-in on ([#632](https://github.com/nledford/Ladderback/pull/632))
- Restart Ladderback from Settings ([#636](https://github.com/nledford/Ladderback/pull/636))
- Set and change the admin password in Settings ([#639](https://github.com/nledford/Ladderback/pull/639))
- Create, edit and delete plans on the Plans page ([#643](https://github.com/nledford/Ladderback/pull/643))
- Choose how long signing in lasts, and keep sessions across restarts ([#641](https://github.com/nledford/Ladderback/pull/641))
- Wipe resolved secrets from memory when they are dropped ([#702](https://github.com/nledford/Ladderback/pull/702))
- `ladderback mcp`, an MCP server through which AI agents check on, run and preview backups ([#640](https://github.com/nledford/Ladderback/pull/640))
- Snapshots name the Ladderback build that made them ([#720](https://github.com/nledford/Ladderback/pull/720))
- Show an icon for each state in status badges ([#717](https://github.com/nledford/Ladderback/pull/717))
- No core dumps or debugger attach for the daemon and its workers ([#701](https://github.com/nledford/Ladderback/pull/701))
- Browse a snapshot's folders and files in an accessible tree ([#729](https://github.com/nledford/Ladderback/pull/729))
- Show a repository's stats, read when asked and after each check ([#760](https://github.com/nledford/Ladderback/pull/760))
- Tell the user when a newer release is available ([#802](https://github.com/nledford/Ladderback/pull/802))
- Run MCP tools once from the command line with `ladderback mcp call` ([#803](https://github.com/nledford/Ladderback/pull/803))
- Give the shared components one vocabulary of tones, sizes and placements ([#807](https://github.com/nledford/Ladderback/pull/807))
- MCP refusals say what to do next in the catalog's words ([#808](https://github.com/nledford/Ladderback/pull/808))
- Add a shared spinner for work in progress ([#816](https://github.com/nledford/Ladderback/pull/816))
- Restore an entry of a snapshot from the web UI, into a new folder by default ([#819](https://github.com/nledford/Ladderback/pull/819))
- Show the daemon's recent log on a Log page of the web UI ([#823](https://github.com/nledford/Ladderback/pull/823))
- Let an AI agent propose plans for a person to confirm ([#820](https://github.com/nledford/Ladderback/pull/820))
- Sign macOS builds from source with a code-signing identity you name ([#828](https://github.com/nledford/Ladderback/pull/828))
- Keep a typed repository password in the system keyring ([#817](https://github.com/nledford/Ladderback/pull/817))
- Turn maintenance from this agent on or off from the Snapshots page ([#825](https://github.com/nledford/Ladderback/pull/825))
- Add a shared icon component ([#834](https://github.com/nledford/Ladderback/pull/834))
- Add a shared button component ([#848](https://github.com/nledford/Ladderback/pull/848))
- Detect a missing Full Disk Access grant on macOS and guide the person to grant it ([#852](https://github.com/nledford/Ladderback/pull/852))
- Add a shared tooltip component ([#855](https://github.com/nledford/Ladderback/pull/855))
- Download a file or folder of a snapshot from its page ([#845](https://github.com/nledford/Ladderback/pull/845))
- List stale translations and offer only the listed languages in a release ([#859](https://github.com/nledford/Ladderback/pull/859))
- Add a shared text field component ([#875](https://github.com/nledford/Ladderback/pull/875))
- Leave the commands out of a profile exported for an AI agent or an API token ([#879](https://github.com/nledford/Ladderback/pull/879))
- Draw every button with the shared button ([#883](https://github.com/nledford/Ladderback/pull/883))
- Wait for the keyring at login before a scheduled run gives up on it ([#885](https://github.com/nledford/Ladderback/pull/885))
- Review, approve and reject an AI agent's proposed plan changes on a Pending changes page ([#858](https://github.com/nledford/Ladderback/pull/858))
- Add a shared table component ([#890](https://github.com/nledford/Ladderback/pull/890))
- Accept only unreviewed translations with accept-translations --all, never stale ones ([#895](https://github.com/nledford/Ladderback/pull/895))
- Choose the web UI's language in Settings ([#899](https://github.com/nledford/Ladderback/pull/899))
- Draw the Pending changes page's buttons with the shared button, busy while the daemon answers ([#901](https://github.com/nledford/Ladderback/pull/901))
- Add a shared link component ([#905](https://github.com/nledford/Ladderback/pull/905))
- Read each keyring entry once with the new build before install-restart restarts ([#910](https://github.com/nledford/Ladderback/pull/910))
- Name the file or folder a restore or a download read on the Operations page ([#916](https://github.com/nledford/Ladderback/pull/916))
- Add a shared skeleton component ([#920](https://github.com/nledford/Ladderback/pull/920))

### Fixed

- A stop signal just after start stops the daemon in order instead of killing it ([#630](https://github.com/nledford/Ladderback/pull/630))
- A plan whose retention keeps everything forgets nothing instead of failing ([#635](https://github.com/nledford/Ladderback/pull/635))
- Say on Linux when a keyring password can't be read before login, and how to choose a Secret Service ([#644](https://github.com/nledford/Ladderback/pull/644))
- A location rule holds on a mixed IPv4 and IPv6 link only if it holds for both ([#646](https://github.com/nledford/Ladderback/pull/646))
- A snapshot whose owner tags disagree is orphaned, never forgotten by a plan ([#713](https://github.com/nledford/Ladderback/pull/713))
- Let just install-restart wait for operations and confirm the build while sign-in is on ([#709](https://github.com/nledford/Ladderback/pull/709))
- The focus ring shows under forced colors in Firefox, and the version wraps in a narrow Safari window ([#631](https://github.com/nledford/Ladderback/pull/631))
- Move focus to the sign-in page after Sign out everywhere in Safari ([#763](https://github.com/nledford/Ladderback/pull/763))
- Keep a repository's further locations with it when a save adds it to config.toml ([#765](https://github.com/nledford/Ladderback/pull/765))
- Tidy the header bar: Sign out's place, its size and the brand's mark ([#770](https://github.com/nledford/Ladderback/pull/770))
- Refuse core dumps and debugger attach in the MCP server ([#806](https://github.com/nledford/Ladderback/pull/806))
- Word the sign-in, token, operation and cursor errors so a program can act on them ([#827](https://github.com/nledford/Ladderback/pull/827))
- Name the right protocol and advice in a location's plain-credentials warning ([#830](https://github.com/nledford/Ladderback/pull/830))
- Tell a missing repository config, a refused hot/cold half and a listing's failure by their own codes ([#835](https://github.com/nledford/Ladderback/pull/835))
- Mask a secret written with TOML's escapes ([#857](https://github.com/nledford/Ladderback/pull/857))
- Keep the reader's place on the Plans page once a plan is saved ([#868](https://github.com/nledford/Ladderback/pull/868))
- Mask the run's secrets and proxy credentials in its hooks' output ([#870](https://github.com/nledford/Ladderback/pull/870))
- Mask the rest of a secret a TOML line-ending backslash pushes onto the next line ([#878](https://github.com/nledford/Ladderback/pull/878))
- Keep an rclone command line out of a failure's detail ([#881](https://github.com/nledford/Ladderback/pull/881))
- End a download in the history when its browser hangs up after its minute ([#887](https://github.com/nledford/Ladderback/pull/887))
- Keep a post-create or post-delete command line out of the daemon's log ([#889](https://github.com/nledford/Ladderback/pull/889))
- Move focus to the Plans heading when a form's opener went with the list ([#898](https://github.com/nledford/Ladderback/pull/898))
- Say when a keyring or password file doesn't answer in time, and how to answer a keychain prompt ([#900](https://github.com/nledford/Ladderback/pull/900))
- Keep Sign in again closed after Escape until the person makes a request ([#917](https://github.com/nledford/Ladderback/pull/917))
- Give back expired downloads' turns before writing their history ([#922](https://github.com/nledford/Ladderback/pull/922))

## [0.3.1](https://github.com/nledford/Ladderback/compare/v0.3.0...v0.3.1) - 2026-11-14

### Before you upgrade

- **Installing from source now needs Node.js 26 or newer.** `scripts/install.sh` refuses an older Node.js and says which version it found; install Node.js 26 first (on Debian and Ubuntu, from NodeSource). Linux means 64-bit x86_64 or aarch64 with glibc 2.35 or newer.
- **Linux release archives:** each release now attaches x86_64 and aarch64 archives and a `SHA256SUMS` file. The install guide's "Download a release (Linux)" section shows how to check and install one.
- **Stops:** an orderly stop (a restart, a reboot, `systemctl stop`) is now recorded as a daemon shutdown and isn't announced, and the daemon is down within 65 s. A crash or power cut is still announced when the daemon starts again.

### Added

- Publish the Linux release archives with checksums ([#599](https://github.com/nledford/Ladderback/pull/599))

### Fixed

- Notify of an operation the daemon stopped during when it starts again ([#588](https://github.com/nledford/Ladderback/pull/588))
- Name a warning's codes in notifications and title ntfy messages in words ([#591](https://github.com/nledford/Ladderback/pull/591))
- Install trunk from its checked prebuilt release instead of building it ([#586](https://github.com/nledford/Ladderback/pull/586))
- Hint at lingering only when it is off, and log secret-file warnings met before a failing secret ([#595](https://github.com/nledford/Ladderback/pull/595))
- Require Node.js 26 and bound the installer's downloads ([#596](https://github.com/nledford/Ladderback/pull/596))
- Link the header's product name to the start page ([#597](https://github.com/nledford/Ladderback/pull/597))
- Stop the daemon within 65 s and let a stopped backup wind down ([#589](https://github.com/nledford/Ladderback/pull/589))

## [0.3.0](https://github.com/nledford/Ladderback/compare/v0.2.0...v0.3.0) - 2026-11-14

Fixes and polish from running 0.2.0 every day on real machines: Linux servers, a Mac and a Raspberry Pi. Ladderback installs with one command; a repository can be reached at several locations, chosen by the network you are on; the Operations page says which step a run is in and why one failed; refusals name the password or credential that was refused; and the header names the running build. There is still no restore: use the `rustic` CLI, and try a restore before relying on the backups. Restore comes in 0.4.

### Upgrading

- **From a checkout of v0.2.0:** run `git checkout -- crates/web/tw-out.css` before `git pull`. Building 0.2.0 rewrote that file, which is no longer tracked, and git won't remove a changed file.
- **On macOS,** a locally built binary is signed ad hoc, so each install is a new program to macOS: it asks again for the keychain, and for Photos or Full Disk Access if Ladderback had them. Choose Always Allow and Allow, or the next backup reads less than it should (it says so with a warning).

### Breaking

- A repository password the repository refuses now answers `503` with the code `password-not-accepted`, where it answered `422` with `operation-failed`. A client that checks for the old code should check for the new one too ([#520](https://github.com/nledford/Ladderback/pull/520))

### Added

- Install Ladderback with one command ([#419](https://github.com/nledford/Ladderback/pull/419))
- Show which step a running operation is in ([#436](https://github.com/nledford/Ladderback/pull/436))
- Reach a repository at several locations, chosen by network ([#443](https://github.com/nledford/Ladderback/pull/443))
- Tell a wired network link on macOS ([#454](https://github.com/nledford/Ladderback/pull/454))
- Warn that a network rule can't keep a plain-HTTP location's credentials from an impostor ([#470](https://github.com/nledford/Ladderback/pull/470))
- Guide a first plan from the start page and the README ([#487](https://github.com/nledford/Ladderback/pull/487))
- Show why an operation failed on the Operations page ([#496](https://github.com/nledford/Ladderback/pull/496))
- Try a repository's HTTPS and local locations at the same time ([#500](https://github.com/nledford/Ladderback/pull/500))
- Keep a scheduled run refused for its secret as a failed backup ([#501](https://github.com/nledford/Ladderback/pull/501))
- Show the running version in the web UI's header ([#507](https://github.com/nledford/Ladderback/pull/507))
- Allow listening on the network with sign-in off, as Backrest does ([#509](https://github.com/nledford/Ladderback/pull/509))
- Name the build beside the version, with the commit between releases ([#541](https://github.com/nledford/Ladderback/pull/541))

### Fixed

- Build with the pinned Rust whatever RUSTUP_TOOLCHAIN says, and install it explicitly ([#427](https://github.com/nledford/Ladderback/pull/427))
- Give the first job of a newly added repository its cache and its repository ([#435](https://github.com/nledford/Ladderback/pull/435))
- Show a running operation of unknown size as working, not 0% ([#434](https://github.com/nledford/Ladderback/pull/434))
- Make the progress bar focusable and calm for screen readers ([#439](https://github.com/nledford/Ladderback/pull/439))
- Keep keyboard focus on the row when a run's Cancel button goes away ([#441](https://github.com/nledford/Ladderback/pull/441))
- Keep a Linux backup running when the thread that started its worker ends ([#449](https://github.com/nledford/Ladderback/pull/449))
- Warn about and mask a location's userinfo given by a ${file:…} reference ([#467](https://github.com/nledford/Ladderback/pull/467))
- Report a locked keyring as unavailable, not as a missing entry ([#491](https://github.com/nledford/Ladderback/pull/491))
- Say when a REST server refuses the repository's credentials ([#489](https://github.com/nledford/Ladderback/pull/489))
- Warn when an exclude with a slash in the middle can never match ([#511](https://github.com/nledford/Ladderback/pull/511))
- Warn about every plain-HTTP location that sends credentials ([#514](https://github.com/nledford/Ladderback/pull/514))
- Name a refused password or credential as such, and keep a resumed run's refusal ([#520](https://github.com/nledford/Ladderback/pull/520))
- Give the Result column room, keep addresses whole and name tables by the heading ([#523](https://github.com/nledford/Ladderback/pull/523))
- Name warnings on history rows, keep API answers out of caches, and warn of readable secret files and proxied hosts ([#540](https://github.com/nledford/Ladderback/pull/540))
- Reinstalling or removing the service on macOS waits for the old daemon to stop ([#545](https://github.com/nledford/Ladderback/pull/545))

## [0.2.0](https://github.com/nledford/Ladderback/compare/v0.1.1...v0.2.0) - 2026-11-12

The first release worth using on your own machines: plans run on their schedules, with catch-up after sleep, cron in a time zone, a choice of skipping or queuing a run while the last one is going, a limit on how many run at once, and retries of a failed scheduled backup. Notifications go to webhooks, ntfy and healthchecks. Ladderback installs as a systemd user service or launchd agent, and an optional sign-in protects the web UI. There is still no restore: use the `rustic` CLI, and try a restore before relying on the backups. Restore comes in 0.3.

### Breaking

- Run maintenance only on repositories this agent maintains ([#201](https://github.com/nledford/Ladderback/pull/201))
- Refuse a second name for a configured repository ([#213](https://github.com/nledford/Ladderback/pull/213))
- Refuse a relative glob file in a plan or an imported profile ([#242](https://github.com/nledford/Ladderback/pull/242))
- Refuse a location or option over 64 KiB and more than 64 options, and mask in linear time ([#251](https://github.com/nledford/Ladderback/pull/251)): a `config.toml` whose `repository` or `repo-hot`, or one of whose option values, is longer than 64 KiB, or whose `options`, `options-hot` or `options-cold` holds more than 64 options, no longer loads, and the repository routes refuse such settings with `invalid-body`. Shorten the location or value (keep credentials in a secret reference or rclone's own config), and split or drop options.
- Refuse a relative path a repository, a hook or a password command names ([#250](https://github.com/nledford/Ladderback/pull/250))

### Added

- Warn when a repository maintained here holds snapshots from other hosts ([#235](https://github.com/nledford/Ladderback/pull/235))
- Record a saved repository's ID the first time a job opens it ([#269](https://github.com/nledford/Ladderback/pull/269))
- Say when Linux has no keyring service to ask, and warn when an export's keyring command is missing ([#288](https://github.com/nledford/Ladderback/pull/288))
- Work out when a plan runs next, with catch-up, jitter and changes of the clocks ([#294](https://github.com/nledford/Ladderback/pull/294))
- A save keeps what a person wrote in config.toml around its change ([#335](https://github.com/nledford/Ladderback/pull/335))
- Read the machine's power source, network state and wakes for the conditions before a scheduled run ([#301](https://github.com/nledford/Ladderback/pull/301))
- Let a reader pause the Operations page's live updates ([#303](https://github.com/nledford/Ladderback/pull/303))
- Install and remove Ladderback as a user service ([#304](https://github.com/nledford/Ladderback/pull/304))
- Read and write a schedule's interval and time zone, and a plan's overlap policy, in config.toml ([#349](https://github.com/nledford/Ladderback/pull/349))
- Tidy the workers' rustic cache each time the daemon starts ([#295](https://github.com/nledford/Ladderback/pull/295))
- Show each listed snapshot's time as its own clock read it ([#311](https://github.com/nledford/Ladderback/pull/311))
- Run scheduled plans, and queued backups again after a restart ([#390](https://github.com/nledford/Ladderback/pull/390))
- Skip or queue a scheduled backup while its last run is going, and limit how many run at once ([#404](https://github.com/nledford/Ladderback/pull/404))
- Send notifications to webhooks, ntfy and healthchecks ([#405](https://github.com/nledford/Ladderback/pull/405))
- Optional sign-in with an admin password ([#395](https://github.com/nledford/Ladderback/pull/395))
- Retry a failed scheduled backup with a doubling back-off ([#414](https://github.com/nledford/Ladderback/pull/414))

### Fixed

- Keep the packs a cancelled backup finished ([#211](https://github.com/nledford/Ladderback/pull/211))
- Let go of the operation log's file when the store closes ([#210](https://github.com/nledford/Ladderback/pull/210))
- Show what a repository check found ([#225](https://github.com/nledford/Ladderback/pull/225))
- Close the operation log when the daemon stops ([#227](https://github.com/nledford/Ladderback/pull/227))
- Let a backup stopped at its deadline keep its finished packs ([#226](https://github.com/nledford/Ladderback/pull/226))
- Warn about credentials kept in a repository's location ([#230](https://github.com/nledford/Ladderback/pull/230))
- Mask output cut off by a timeout as cut text ([#231](https://github.com/nledford/Ladderback/pull/231))
- Mask credentials written in an rclone connection string ([#239](https://github.com/nledford/Ladderback/pull/239))
- Show what each operation did on the Operations page ([#241](https://github.com/nledford/Ladderback/pull/241))
- Mask cut output whoever cut it, and never quote a cut secret ([#238](https://github.com/nledford/Ladderback/pull/238))
- Create an imported repository from its row on the Repositories page ([#253](https://github.com/nledford/Ladderback/pull/253))
- Never use or quote against a secret command's stdout cut partway through a line ([#254](https://github.com/nledford/Ladderback/pull/254))
- Refuse a job whose secrets come to more than 256 KiB to mask ([#264](https://github.com/nledford/Ladderback/pull/264))
- Start every child process through one spawn gate ([#270](https://github.com/nledford/Ladderback/pull/270))
- Keep rustic's cache between jobs in the agent's cache folder ([#282](https://github.com/nledford/Ladderback/pull/282))
- Keep the Operations table's columns still and its sort button apart from the header's text ([#293](https://github.com/nledford/Ladderback/pull/293))
- Let button labels and other form controls grow with the text size ([#298](https://github.com/nledford/Ladderback/pull/298))
- Name the repository a first job opened when it starts ([#307](https://github.com/nledford/Ladderback/pull/307))
- Refuse a repository location a reference resolves to a relative path ([#313](https://github.com/nledford/Ladderback/pull/313))
- Open the e2e steps' HTTP connections only once they are connected ([#318](https://github.com/nledford/Ladderback/pull/318))
- Let a stopped backup wind down when its events went unread ([#322](https://github.com/nledford/Ladderback/pull/322))
- Refuse a backup start whose secret can't be resolved with the secret's code ([#327](https://github.com/nledford/Ladderback/pull/327))
- Keep repository credentials out of the daemon's log during retention ([#334](https://github.com/nledford/Ladderback/pull/334))
- Mask credentials where rclone quotes them as Go's %q does ([#336](https://github.com/nledford/Ladderback/pull/336))
- Refuse rclone locations and flags that name a relative path ([#323](https://github.com/nledford/Ladderback/pull/323))
- Hold the spawn gate only while rclone starts, not until it serves ([#329](https://github.com/nledford/Ladderback/pull/329))
- Announce a run's start only once the history has it running ([#332](https://github.com/nledford/Ladderback/pull/332))
- Stop an rclone that never serves at a start deadline, rather than leaving it running ([#348](https://github.com/nledford/Ladderback/pull/348))
- Warn when another agent recently wrote to a repository maintained here under this machine's name ([#297](https://github.com/nledford/Ladderback/pull/297))
- Keep a starting child from holding the server's connections on macOS ([#320](https://github.com/nledford/Ladderback/pull/320))
- Say where config.toml is wrong on every page and route that reads it ([#391](https://github.com/nledford/Ladderback/pull/391))
- Show the warnings a run recorded before the Operations page was listening ([#389](https://github.com/nledford/Ladderback/pull/389))
- Stop the Operations page's live updates when you leave it ([#392](https://github.com/nledford/Ladderback/pull/392))
- Mask secrets as rclone quotes them whatever Rust toolchain builds Ladderback ([#407](https://github.com/nledford/Ladderback/pull/407))

### Changed

- Building Ladderback from source needs Rust 1.100.0 or newer, and the checkout pins that version for rustup ([#411](https://github.com/nledford/Ladderback/pull/411))

## [0.1.1](https://github.com/nledford/Ladderback/compare/v0.1.0...v0.1.1) - 2026-11-11

### Fixed

- Apply a plan's source filter and host to its backups ([#197](https://github.com/nledford/Ladderback/pull/197))
- Name each plan's start backup button with its plan ([#203](https://github.com/nledford/Ladderback/pull/203))

### Changed

- Building Ladderback from source needs Rust 1.94 or newer ([#196](https://github.com/nledford/Ladderback/pull/196))

## [0.1.0](https://github.com/nledford/Ladderback/releases/tag/v0.1.0) - 2026-11-11

The first release: Ladderback on a single machine. Connect or initialize repositories, plan backups in `config.toml`, import and export rustic profiles, and run backups by hand with live progress in the web UI. Snapshots, unmanaged and orphaned groups (with adoption) and the operation history are listed. Forget, prune and check run by hand on this machine, the repository's one maintenance owner, and prune never deletes instantly. Secrets come from a command, the keyring, a file or the environment, and are masked wherever output appears, even where a cap cuts it. A second daemon on the same data folder is refused. Scheduling and notifications come in 0.2, restore in 0.3.

### Added

- Run backup, prune, check and identify jobs in a rustic worker (M1a) ([#2](https://github.com/nledford/Ladderback/pull/2))
- Refuse web requests that fail the request checks (§10.1) ([#7](https://github.com/nledford/Ladderback/pull/7))
- Record every operation in a SQLite operation log, and time out stuck workers ([#8](https://github.com/nledford/Ladderback/pull/8))
- Read repository secrets from files, commands and the keyring, and keep them out of everything reported ([#21](https://github.com/nledford/Ladderback/pull/21))
- Render every error code and daemon message through an en-US Fluent catalog ([#20](https://github.com/nledford/Ladderback/pull/20))
- Render the web UI through the catalog, and add the en-XA and ar-XB pseudolocales ([#42](https://github.com/nledford/Ladderback/pull/42))
- Pass a repository's hot and cold fields to rustic ([#41](https://github.com/nledford/Ladderback/pull/41))
- Serialize maintenance operations per repository (I7) ([#52](https://github.com/nledford/Ladderback/pull/52))
- Write numbers and instants per locale in daemon text with ICU4X ([#54](https://github.com/nledford/Ladderback/pull/54))
- The configuration model and its ConfigStore port ([#56](https://github.com/nledford/Ladderback/pull/56))
- Write numbers and instants per locale in the web UI with the browser's Intl ([#63](https://github.com/nledford/Ladderback/pull/63))
- Config.toml with a schema version, migrations and version history ([#64](https://github.com/nledford/Ladderback/pull/64))
- Routes with focus on arrival, and a live-region announcer ([#70](https://github.com/nledford/Ladderback/pull/70))
- Initialise, test and connect a repository ([#71](https://github.com/nledford/Ladderback/pull/71))
- The accessible component set, its gallery and a token contrast gate ([#72](https://github.com/nledford/Ladderback/pull/72))
- Run plan, repository and agent hooks around a backup ([#73](https://github.com/nledford/Ladderback/pull/73))
- List a repository's snapshots ([#77](https://github.com/nledford/Ladderback/pull/77))
- Wire the daemon and stream events to the UI ([#79](https://github.com/nledford/Ladderback/pull/79))
- The operation lifecycle over HTTP ([#81](https://github.com/nledford/Ladderback/pull/81))
- The UI runs and watches backups ([#82](https://github.com/nledford/Ladderback/pull/82))
- Forget, prune and check run manually on the local owner ([#84](https://github.com/nledford/Ladderback/pull/84))
- Adopt orphaned and unmanaged history into a plan ([#85](https://github.com/nledford/Ladderback/pull/85))
- A Repositories page that tests, connects and creates repositories ([#102](https://github.com/nledford/Ladderback/pull/102))
- Import and export rustic profiles ([#105](https://github.com/nledford/Ladderback/pull/105))
- Warn of a secret written in the config, in the log and on the repository's row ([#130](https://github.com/nledford/Ladderback/pull/130))
- Refuse each secret and template failure with a code of its own ([#133](https://github.com/nledford/Ladderback/pull/133))
- --plain and import-profile --json, and a quiet stop when nobody reads the output ([#137](https://github.com/nledford/Ladderback/pull/137))
- Review maintenance before it runs (previews, held selections, history groups) ([#155](https://github.com/nledford/Ladderback/pull/155))
