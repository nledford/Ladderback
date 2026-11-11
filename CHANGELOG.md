# Changelog

All notable changes to Ladderback are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Ladderback uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as described in
[docs/design/operations.md §12.1](docs/design/operations.md#121-versioning-and-releases).

## [Unreleased]

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
