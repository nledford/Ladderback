# Changelog

All notable changes to Ladderback are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Ladderback uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as described in
[docs/design/operations.md §12.1](docs/design/operations.md#121-versioning-and-releases).

## [Unreleased]

## [0.1.0](https://github.com/nledford/Ladderback/releases/tag/v0.1.0) - 2026-11-11

### Breaking

- Rename Steading to Ladderback ([#57](https://github.com/nledford/Ladderback/pull/57)): the package, binary, STEADING_* environment variables, config
and data paths, keyring service names and snapshot tags are now ladderback.

### Added

- Add the operation lifecycle
- Add the Clock and Engine ports and RunOperation
- Add the API types crate
- Run each operation in a worker process
- Add the system clock
- Add the steading binary with serve and worker
- Serve an embedded start page
- Add skip!, a visible test skip
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

### Fixed

- Cap worker protocol messages at 1 MiB
- Check src/main.rs for the no-panic lints and fail loudly
- Wait for the whole listening line before reading the URL ([#32](https://github.com/nledford/Ladderback/pull/32))
- Mask a rejected job's detail and proxy credentials in what a worker says ([#36](https://github.com/nledford/Ladderback/pull/36))
- Cap warning text and give stored failure arguments their own format ([#37](https://github.com/nledford/Ladderback/pull/37))
- Time out a worker that never sends its first event ([#39](https://github.com/nledford/Ladderback/pull/39))
- Read the debug build's web UI from the checkout it was built in ([#67](https://github.com/nledford/Ladderback/pull/67))
- Resolve time zones on hosts with no tzdata ([#87](https://github.com/nledford/Ladderback/pull/87))
- Saves keep a symlinked config.toml, and the history survives a clock set back ([#86](https://github.com/nledford/Ladderback/pull/86))
- The UI journeys run the binary cargo built, wherever its target directory is ([#91](https://github.com/nledford/Ladderback/pull/91))
- The unused-message scan skips the files test modules live in ([#88](https://github.com/nledford/Ladderback/pull/88))
- Harden secret sources and the redactor ([#94](https://github.com/nledford/Ladderback/pull/94))
- Spawned daemons use a scratch data folder, never the real one ([#95](https://github.com/nledford/Ladderback/pull/95))
- Keep blocking steps and a crowd of scenarios off the e2e runner's thread ([#97](https://github.com/nledford/Ladderback/pull/97))
- A cancel sent right after a backup starts is accepted ([#100](https://github.com/nledford/Ladderback/pull/100))
- Forget and list repositories of any size, and say when a snapshot's paths or tags were cut ([#99](https://github.com/nledford/Ladderback/pull/99))
- A maintenance cancel made before the engine is up wins over its ready result ([#103](https://github.com/nledford/Ladderback/pull/103))
- A stalled operation-log write no longer holds a run or delays its cancel ([#104](https://github.com/nledford/Ladderback/pull/104))
- Stop a timed-out password command with SIGTERM, then SIGKILL ([#108](https://github.com/nledford/Ladderback/pull/108))
- Stop a hook without signalling a reaped group, within a bound, and with its pipes open ([#109](https://github.com/nledford/Ladderback/pull/109))
- Bound request bodies, send security headers, say refused hosts from the catalog ([#111](https://github.com/nledford/Ladderback/pull/111))
- Stop a password command without blocking on its output, and reap a hook when it exits with its output held open ([#121](https://github.com/nledford/Ladderback/pull/121))
- Write a page's instants in the browser's own zone ([#122](https://github.com/nledford/Ladderback/pull/122))
- Stop waiting for a repository that never answers ([#125](https://github.com/nledford/Ladderback/pull/125))
- Keep the log writable after a write is cut short ([#136](https://github.com/nledford/Ladderback/pull/136))
- Keep keyboard focus on a running backup's controls while its row updates ([#139](https://github.com/nledford/Ladderback/pull/139))
- End the operations a crashed daemon left unfinished, and refuse a second daemon on one data dir ([#140](https://github.com/nledford/Ladderback/pull/140))
- Tell a page how an operation ended only once the log has it ([#142](https://github.com/nledford/Ladderback/pull/142))
- Log each write the operation log refuses, and keep a client's hang-up from leaving a repository held ([#152](https://github.com/nledford/Ladderback/pull/152))
- A prune preview reads the repository through its cache, as the prune does ([#162](https://github.com/nledford/Ladderback/pull/162))
- Mask a secret that an output cap cuts, wherever output is capped ([#164](https://github.com/nledford/Ladderback/pull/164))
- Closing the operation log waits for every write in flight ([#173](https://github.com/nledford/Ladderback/pull/173))
