## 8. Technology choices

### 8.1 Frontend: Leptos CSR with an explicit REST API

| Option | Pros | Cons |
|---|---|---|
| **CSR with explicit REST + WebSocket API** (chosen) | Single `trunk` build output that's easy to embed. The API is a real, versioned contract that the hub, the CLI and a future Tauri shell can all reuse. Shared types via an `api` crate | A bit more boilerplate than server functions |
| SSR + hydration with server functions | Less API code. Fast first paint | Couples UI and server build (`cargo-leptos`, two targets). Server fns aren't a stable contract for agents or a CLI. SEO and first paint don't matter for a localhost dashboard |

UI kit: plain Leptos with Tailwind (with the `trunk` Tailwind hook) for styling, and Ladderback's own small component set for every widget, built to the accessibility contracts in §4.5. Tailwind arrives with that component set in M1b; M0's hello page uses one plain stylesheet with the color, focus and motion tokens. Charts (snapshot sizes over time, durations) use SVG rendered in Leptos, with no JS chart library.

Icons: Lucide's, from `lepticons-data`, which holds each icon's shapes without a renderer, with the default features off and only the categories the icons in use are in (§16, decision 103). Every icon of an enabled category is compiled in, used or not, so a new icon from another category turns that category on in the workspace `Cargo.toml` and is measured as below. *Size:* the status badges' seven icons (`notifications`, `time`, `cursors`, `shapes` and `math`, 267 icons) add 80,224 B (2.7%) to the release `web` wasm built with `--no-default-features`, 3,013,400 B before and 3,093,624 B after, and 17,110 B (2.4%) to its brotli, 711,271 B before and 728,381 B after. Measured on macOS arm64 at `7c4ac062` and after with `(cd crates/web && trunk build --release --no-default-features --dist <dir>)` and `brotli -q 11`.

The shared `Icon` component (§4.5, #778), which the status badges now draw with, turns on no category. The release `web` wasm built with `--no-default-features` grows 2,266 B (0.06%), 3,527,913 B before and 3,530,179 B after, and its brotli shrinks 2,211 B (0.3%), 818,918 B before and 816,707 B after; the gallery that shows every icon size and tone isn't in that build. Measured as above, at `690f5f70` and after.

The shared text field's Show password button (§4.5, #782) draws an eye and a crossed-out eye, which turn on `accessibility`: 34 icons, 29 of them in no category already on, 296 in all. That category alone adds 7,982 B (0.2%) to the release `web` wasm built with `--no-default-features`, 3,587,011 B before and 3,594,993 B after, and 2,035 B (0.2%) to its brotli, 827,306 B before and 829,341 B after. Measured as above, at `4eacb452` with only the category added.

The pages' buttons (§4.5, #780) turn on `arrows`, `multimedia` and `files` (713 icons in all) for their icons. The three categories alone add 127,262 B (3.5%) to the release `web` wasm built with `--no-default-features`, 3,667,922 B before and 3,795,184 B after, and 22,216 B (2.6%) to its brotli, 840,164 B before and 862,380 B after; with the pages moved onto `Button` it is 3,814,082 B, 146,160 B (4.0%) over the build before, and its brotli 867,611 B, 27,447 B (3.3%) over. Measured as above, at `e765acd8` (the categories turned on in a copy of it) and after.

### 8.2 Backend crates

| Concern | Crate |
|---|---|
| Async runtime / HTTP | `tokio`, `axum 0.8`, `tower-http` |
| Engine | `rustic_core`, `rustic_backend` (their crates.io releases, decision 7; wrapped in our `engine` crate) |
| DB | `sqlx 0.9` (SQLite, bundled, migrations; runtime-checked queries with bound parameters, because compile-time-checked ones need `sqlx-cli` and committed query data in CI) |
| Logging | `log` facade; `env_logger` in the daemon, writing to stderr at `info` unless `RUST_LOG` says otherwise |
| Scheduling | `croner` (with its `jiff` feature, not `chrono`) |
| Time | `jiff` with `tzdb-bundle-always`: the build carries the IANA time-zone database for a host that has none (§3.2, §16 decision 30) |
| Embedding UI | `rust-embed` |
| Auth | `argon2` (RustCrypto), `subtle` for comparing tokens in constant time, and a small cookie-session layer in the HTTP adapter (§10.1, decision 70) |
| Crypto / identity | `aws-lc-rs` (Ed25519, `adapters/signing`), `rustls` and `tokio-rustls` (TLS 1.3 over aws-lc-rs, the one provider) and `rcgen` (the hub's self-signed certificate), both in `adapters/tls` (§16, decision 120) |
| Secrets | `keyring` (macOS Keychain, Secret Service on Linux); `zeroize` wipes a resolved secret's bytes when it is dropped (§10.2) |
| Notifications | `reqwest` (webhooks, ntfy, Gotify, healthchecks.io), `lettre` (SMTP) |
| Localization | `fluent-bundle`, `fluent-langneg`, `unic-langid`; `icu` (ICU4X) for dates and numbers in daemon-rendered text (§8.4) |
| CLI | `clap` |

The minimum supported Rust version (MSRV) is **1.100.0**, the workspace's `rust-version`. Every crate inherits it. It is the exact version `rust-toolchain.toml` pins, which development, CI and the release builds use, and the two move together (§13.12, §16 decision 78).

Ladderback isn't published to crates.io: #207 decided it isn't worth publishing about 20 crates and a trunk-built UI every release. Distribution is §12.

### 8.3 Workspace layout

The crates follow §3.2: two inner rings, one crate per adapter, and a composition root.

```
ladderback/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ domain/                 # aggregates, value objects, domain services, events; no I/O; builds for wasm32
│  ├─ app/                    # use cases, runtime loops, port traits, in-memory fakes and port contract suites
│  │                          # (feature "fakes"); tests/acceptance.rs runs non-@e2e scenarios
│  ├─ api/                    # wasm-safe DTOs: REST, UiEvent, sync protocol (the published language)
│  ├─ testkit/                # test support, a dev-dependency only: `skip!`, `daemon::isolate` (§13.5)
│  ├─ spawn-gate/             # the spawn gate: one child process starts at a time (decision 58); shared by every
│  │                          # adapter that starts a child, the HTTP server's accept on macOS (decision 66) and the @e2e runner
│  ├─ web/                    # Leptos CSR app (trunk); inbound adapter in the browser
│  │  ├─ src/<page>.rs        # one module per page: what the page decides, tested on the host; `<page>_page.rs`
│  │  │                       # is the glue that draws it (`plans.rs`, `plans_page.rs`); older pages still mix the two (#739)
│  │  ├─ src/routes.rs        # what the pages share, each path's title and `NotFound`; the route table is
│  │  │                       # `main.rs`'s
│  │  └─ src/components/      # the shared component set (§4.5): one file per component, its host-tested
│  │                          # decisions in a child module, `<component>/rules.rs`
│  ├─ l10n/                   # wasm-safe: Fluent bundle chain, locale negotiation, formatters (features icu,
│  │                          # browser-intl, pseudo); build.rs generates message IDs from en-US
│  │  └─ locales/<tag>/       # common.ftl, ui.ftl, notify.ftl, cli.ftl (en-US is the source locale)
│  └─ adapters/
│     ├─ engine/              # rustic_core + rustic_backend: worker main, engine::job (JobSpec, WorkerEvent),
│     │                       # EventProgressBars, Retention via ForgetGroups, option mapping + parity tests
│     ├─ http/                # axum: REST, UI events WebSocket, embedded UI, request checks, auth
│     ├─ sync/                # /sync/v1 server (hub) and client (agent)
│     ├─ store/               # sqlx SQLite: operations, catalog, outbox, ownership, registry
│     ├─ config/              # versioned file formats, parser, migrations on load, version history
│     ├─ interop/             # rustic profile import/export (anti-corruption layer; the only code that knows the format, §6.2):
│     │                       # read = `locate` (spanned TOML), `keys` + `tables` + `parse` (strict key lists, parity tests
│     │                       # against the pinned `rustic_core`), `part` (rustic's merge), `resolve` (`use-profiles`, caps),
│     │                       # `map` (to domain types, notices), `escape` (`${` → `$${`); write = `write` (hand-written TOML);
│     │                       # ports and use cases: `app::ports::profiles`, `app::interop`; contract suite `contract::profiles`
│     ├─ secrets/             # command, keyring, file, env sources; `${…}` substitution (redaction: domain `Redactor`)
│     ├─ notify/              # webhook, ntfy, Gotify, Discord/Slack, SMTP, healthchecks
│     ├─ releases/            # Ladderback's newest release from GitHub Releases (`ReleaseFeed` port, §10.7)
│     ├─ signing/             # Ed25519 over aws-lc-rs (`Signing` port; §16, decision 120)
│     ├─ tls/                 # the hub's built-in TLS 1.3 listener and certificate, the agent's pinned client
│     │                       # (`TlsKeys` port, `app::hub_tls`; §16, decision 120)
│     ├─ mcp/                 # `ladderback mcp`: JSON-RPC 2.0 over stdio; in-process and /api/v1 backends (§4.6)
│     └─ platform/            # clock, power, network, service install (systemd/launchd); key files (`Identity`
│                             # port, feature `hub` only; §16, decisions 120 and 122)
├─ features/                  # Gherkin scenarios, one folder per bounded context (§13.2)
├─ tests/e2e.rs               # cucumber-rs runner for @e2e scenarios against the real binary
├─ ui-tests/                  # Node: @cucumber/cucumber + Playwright + axe-core for @ui scenarios and the
│                             #   component gallery (§4.5)
├─ src/main.rs                # composition root; `ladderback` binary: serve | worker | mcp | pair | import-profile
│                             #   | export-profile | install-service | db rebuild
├─ scripts/                   # CI checks behind the Justfile: banned crates (§3.2), red-first (§13.1), skip log (§13.5)
├─ Justfile                   # every workflow, for people, agents and CI alike
├─ migrations/
└─ docs/
```

`adapters/engine` is the only crate that depends on `rustic_core`, which contains the impact of its pre-1.0 API churn. `api` compiles to `wasm32` and never depends on `rustic_core` or an adapter; where the UI needs a domain value it either reuses `domain` (also wasm-safe) or defines a DTO, with a round-trip test per DTO. `JobSpec` and `WorkerEvent` only travel between the daemon and the worker, both of which link `adapters/engine`, so they live in `engine::job`. They reuse the serde-capable core types and carry Ladderback's own serde mirrors of the rest (`PruneOptions`, `CheckOptions`, `RestoreOptions`, `LsOptions`, `CheckResults`). The engine adapter translates `WorkerEvent` into `app`'s `EngineEvent`.

### 8.4 Internationalization and localization (Project Fluent)

**Scope.** Until M7, releases ship **American English (`en-US`) only**, but Ladderback is localization-ready from the first user-facing string. Adding a language touches only the `l10n` crate: drop `.ftl` files into `locales/<tag>/`. Its `build.rs` regenerates the locale table and the embedded catalogs. ICU4X's data is compiled in for every locale, so a new language needs nothing there (its size is under *Host formatters*). Retrofitting this after the UI exists would mean touching every view, so it starts in M1b with the first real screen.

**Japanese** (`ja`) is the first language after it, in M7 (§16, decision 101). An AI drafts the messages and the maintainer, who is learning to read and write Japanese, reviews them before each release. A release offers only the languages listed in `crates/l10n/offered-locales.txt`, and the release pull request's checks refuse a listed language with a message that is missing or stale (translated from `en-US` text that has changed since). Default builds offer every catalog, for review; builds without the default features, the ones a release is, offer only the listed ones.

| Text | Localized? |
|---|---|
| Web UI: labels, messages, errors, `aria-*` text, page titles | Yes, in the browser's negotiated locale |
| Default notification text (ntfy, Gotify, Discord/Slack, email) | Yes, in the configured locale |
| Ladderback's own CLI messages and errors | Yes, from the POSIX locale, and so are the help's descriptions of the program, its commands and their options. `clap`'s own words in the help and its usage errors stay English |
| The sentence an MCP refusal gives after its `{code, args}`, and the one `ladderback mcp call` says on stderr for a refused call (§4.6) | Yes, from the POSIX locale of the environment the server runs in, as the CLI's own messages: an MCP client sends no language |
| The static `index.html` shown before the wasm app loads | No: a minimal `lang="en"` loading text, replaced on startup |
| Logs, `config.toml` keys, API and sync field names, webhook JSON keys and event names, MCP tool names, arguments and results (a refusal's `{code, args}` included), the CLI's `--json` documents, exported rustic profiles | **Never.** These are machine-facing or grep targets |
| Text from rustic itself (warning and error details) | Shown verbatim, in English, inside a localized frame (`backup-failed = Backup failed: { $detail }`), marked `lang="en"` in the UI |
| User-written notification templates (§11) | Never; the user's own words |

**Errors cross the hexagon as codes, not sentences.**

- `domain` and `app` return typed errors and outcomes (`Failed { reason: DeadlinePassed }`, `ClaimRejected { owner }`). No English string is produced inside the core, so the core needs no locale and its tests never assert on wording.
- Adapters map them onto the published codes in `api`: closed enums `ErrorCode` and `WarningCode`, each with a `#[serde(other)] Unknown` variant.
- Arguments are typed: `MsgArg = Count(u64) | Bytes(u64) | DurationMs(u64) | Timestamp { utc_ms, zone: Option<IanaZone> } | Text(String)`. Identifiers, ports, years and hostnames are `Text`, so they are never digit-grouped.
- The REST API, the sync protocol, `WorkerEvent` (§4.1), SQLite (§9) and the CLI's `--json` documents (§4.5) all carry `{ code, args, detail? }`, never rendered text. `detail` is rustic's own verbatim message.
- Codes are rendered by matching the enum, never by using a wire string as a message ID. An `Unknown` code, say from a newer agent, renders as `error-unknown = An unexpected error occurred.` followed by its detail. It has no `$code` to show: `#[serde(other)]` discards the wire string.
- **Redaction happens where codes are built:** `args` and `detail` pass through the `Redactor` before they are serialized, stored or synced (I4). `l10n` never sees a secret.

**The `l10n` crate.**

- **Placement.** It is a presentation library: neither a ring nor an adapter. It depends only on `api` (for the codes), the Fluent and formatter crates, and `jiff` (for the plain UTC date and, under `icu`, an instant's wall clock in its zone). `web`, `adapters/notify`, `adapters/mcp` and the CLI in the composition root use it; `domain` and `app` must not (§3.2 banned crates).
- **Fallback chain.** One `FluentBundle` holds one locale's messages; its plural rules come from that locale. The negotiated chain (always ending in `en-US`) is therefore an ordered `Vec` of bundles, one per locale, and a lookup falls through per message. The browser uses the single-threaded `FluentBundle`; the daemon uses `fluent_bundle::concurrent::FluentBundle`, shared across tasks.
- **Generated catalog table.** `build.rs` scans `locales/`, embeds every file with `include_str!`, and generates the message-ID constants in `l10n::msg` from `en-US`. A typo is a compile error, and the constants can't drift from the catalog. There are no runtime-loaded or remote catalogs.
- **Locale negotiation:** `fluent-langneg`, filtering strategy, default `en-US`, over the languages the build offers (*Scope*, above): a release build ignores a request for a catalog it carries but doesn't offer, in all three places below.
  - UI: the user's language choice, kept in the browser (Settings' Language section), then `navigator.languages`.
  - Notifications: `[agent] locale` on the hub or standalone agent, then the service's POSIX locale, then `en-US`. Services under systemd or launchd usually have no `LANG`.
  - CLI: `LC_ALL`, `LC_MESSAGES` and `LANG`, with `LANGUAGE` (a list) ahead of the locale; the rules are under *Core as built*. POSIX values are normalized before parsing: strip the codeset and modifier, turn `_` into `-`, and ignore `C` and `POSIX`. `ladderback mcp` says its refusals by the same rule, read once when the session starts (§4.6).
  - Tags are parsed with `unic-langid`. An invalid tag is ignored; tags never reach a file path, because catalogs are compiled in.
- **Numbers.** `fluent-bundle`'s built-in `NUMBER` keeps a value numeric, so plural selectors work, but it formats without locale-aware grouping. Ladderback keeps the built-in and does locale-aware output in the bundle's `set_formatter` hook. That hook is a plain `fn`, so it reaches the host formatter through the bundle's memoizer (`intl_memoizer::Memoizable`, keyed by locale).
- **Dates.** `fluent-bundle` has no `DATETIME`. Ladderback registers one that returns a `FluentValue::Custom` value carrying the instant and its style. The same hook writes it through the memoized host formatter; the value's own text is the plain one (*Plain fallbacks*, below).
- **Host formatters,** each behind a feature of `l10n`. A host is a pair of formatters, one for numbers and one for instants, which the bundle's memoizer builds per locale and configuration (a number formatter per set of digit options, an instant formatter per `DATETIME` style) and keeps for the bundle's life. A host can decline, when it has no data for the locale or can't write a value, and then the text is the plain one, so a host never has to be total. No input makes formatting fail, panic or allocate without limit: digit counts are capped at ECMA-402's limits (100 fraction digits, 21 integer digits) before a host sees them, and an instant beyond the years ±9999 is declined. What a host honors is a closed vocabulary, so every host can honor all of it:
  - `NUMBER`: `minimumIntegerDigits`, `minimumFractionDigits`, `maximumFractionDigits` and `useGrouping` (`"false"` only). Any other option is ignored, as Fluent's own formatter ignores it. Without `maximumFractionDigits` every digit of the value is shown, not `Intl`'s default of three. A value is rounded half away from zero on its shortest decimal digits, so `1.005` at two digits is `1.01`, as ICU4X and `Intl` both write it.
  - `DATETIME`: `dateStyle` (`short`, `medium`, `long`) and `timeStyle` (`short`, `medium`), alone or together; with no options, a medium date and a short time. `timeZoneName: "short"` alone writes the zone's short name (`CET`, or `GMT+1` where the locale has none); browsers refuse it together with a style, so it is never combined. Anything else (`dateStyle: "full"`, `timeStyle: "long"`, an unknown option or value) is an error that Fluent shows as `DATETIME()`.
  - `icu`: ICU4X (`icu_decimal`, `icu_datetime`, `icu_time`) for the daemon and CLI, with the data for every locale compiled in. Only the root crate turns it on: `web` takes `l10n` without it, so no locale data reaches the browser, and `scripts/check-architecture.sh` fails if `l10n` or `web` (default features) build a crate that formats or carries it, or if `ladderback` lacks `icu_decimal` or `icu_datetime`. It adds 1,109,288 B (4%) to the stripped release `ladderback`: 28,014,136 B before, 29,123,424 B after, measured on macOS arm64 with `(cd crates/web && trunk build --release)`, then `cargo build --release --locked --package ladderback`, `strip -o <out> target/release/ladderback` and `stat -f %z <out>`. At that size v1 doesn't trim the data to the shipped locales.
  - `browser-intl`: `Intl.NumberFormat` and `Intl.DateTimeFormat` (`crates/l10n/src/browser.rs`) for `web`, which turns the feature on; no locale data ships to the browser.
    - *Selection.* The host is chosen by `cfg`, not at run time: `format::Active` is the browser's where `browser-intl` is on and the target is wasm32, ICU4X's where `icu` is on otherwise, and none where neither is. The feature does nothing off wasm32, so a workspace build that unifies it with the daemon's `icu` still formats with ICU4X. `l10n`'s own tests turn `icu` on, so Cargo builds the ICU4X crates for the wasm lane too; `mod icu` is skipped there, so none of it is linked into the test.
    - *`Send` and `Sync`.* The host traits and the memoizer need `Send + Sync`, and the bundle stays the concurrent one the daemon uses. That holds because wasm-bindgen makes `JsValue` (and the `js_sys::Intl` types over it) `Send + Sync` unless the target has the `atomics` feature (`wasm-bindgen` 0.2.130, `src/lib.rs`), which the web UI's plain `wasm32-unknown-unknown` build, with `trunk`, doesn't have. Built with `atomics` the host fails to compile on that bound, and never shares a `JsValue` between threads. So there is no second memoizer or bundle type and no `unsafe`.
    - *Decline, never throw.* A formatter is built with `Reflect::construct`, so a `RangeError` (a digit count or a time zone `Intl` refuses) is `None`, as is a missing `Intl` constructor. A locale `Intl` has no data for (`supportedLocalesOf` is empty) is declined too: `Intl` would write it in the runtime's own locale, and the plain text is better than another locale's. Nothing in the host panics or lets an exception through.
    - *Numbers.* A number goes to `Intl` as the shortest decimal string `f64`'s `Display` writes, which rounds half away from zero on those digits. Without `maximumFractionDigits` the host passes 100, so every digit shows. A value that isn't finite is declined.
    - *Instants.* A time zone is a constructor option in `Intl`, so an instant formatter keeps one `DateTimeFormat` per zone behind a `Mutex`, at most 64: zone names are input, and a zone beyond the cap is built for that call.
      - No zone, and a zone the browser doesn't have, are read as UTC, never as the viewer's, and the zone-name style then writes `UTC`. That is the host's reading, as the `icu` host's, and the rule for every caller (§16, decision 35): the formatter never reads a zone from its environment, and a page that wants an instant in the viewer's zone passes it.
      - The web UI always does, with the browser's own, `Intl.DateTimeFormat().resolvedOptions().timeZone` (`l10n::viewer_zone`), wherever it shows an instant (the Snapshots and Operations pages today).
      - The zone name is the `timeZoneName` part of `formatToParts`. An instant beyond the years ±9999 is declined (`jiff`'s bound), well inside the range a `Date` takes. The host doesn't fold U+202F and U+00A0, which `Intl` writes as it writes them; only the tests fold.
    - *Size.* It adds 9,731 B (2.1%) to the release `web` wasm built with `--no-default-features`, 463,817 B before and 473,548 B after, and 2,723 B (1.9%) to its brotli, 142,812 B before and 145,535 B after; with `pseudo`, 14,666 B (1.0%) and 4,239 B to brotli, 1,448,635 B and 371,034 B before. Measured on macOS arm64 at `d3bf248` and after with `(cd crates/web && trunk build --release [--no-default-features] --dist <dir>)` and `brotli -q 11`. The 446,878 B below is that build before later changes landed.
    - *Tests.* `just test-wasm` runs the host under `wasm-bindgen-test` in Node (`wasm-bindgen-test-runner` at the version of `Cargo.lock`'s `wasm-bindgen`, which `just setup` installs): the host's own tests, the table of *Tests* below through the public `Localizer`, and `web`'s bundle writing `NUMBER` and `DATETIME`. It runs with `TZ=America/New_York`, so the browser's own zone is not UTC and a host that read no zone as the viewer's fails the table's UTC cases, while `viewer_zone` and the web UI's instants are tested against that zone. CI doesn't run it yet.
- **Sizes and durations.** Byte sizes pick the IEC unit in code (KiB, MiB, GiB) and format the number through `NUMBER`, with the unit label in the catalog. Durations are catalog messages over numeric parts (`{ $hours } h { $minutes } min`), so word order stays with the translator.
- **Time zones.** Timestamps travel as UTC plus an optional IANA zone.
  - The UI shows times in the viewer's browser zone: the page passes it with every instant, as an instant with no zone is read as UTC on both hosts (§16, decision 35). A scheduled run whose plan has its own zone shows both ("02:00 Europe/Berlin, 03:00 your time").
  - A listed snapshot's time is the exception: the page shows what the clock that took it read, the day retention counts it on, with that clock's offset after it (`ui-snapshot-time-offset`, "UTC-06:00") only when the viewer's clock, as the browser's `Date.getTimezoneOffset` gives it for that instant, was at another offset. Where the host doesn't write the locale's instants (`Localizer::writes_instants`), the page writes the reading plainly with its offset always, never labelled UTC. A daemon older than the listing's `offset` sends none, and the time is shown in the viewer's zone (§16, decision 56).
  - Notification text uses the sending agent's local zone and names it.
  - *Zone rules.* The `icu` host takes the instant's offset and wall clock in its IANA zone from `jiff`, which the daemon already has; ICU4X only writes that time, in the locale's patterns, with the locale's name for the zone.
    - ICU4X's own zone data (CLDR's zone names and metazones, the mapping from IANA names, and the standard and daylight offsets of each period of a zone since 1970, which pick a name) has no transitions and never decides what the clock reads, so there is one source of zone rules, not a second time-zone database next to `jiff`'s.
    - The build carries a copy of the IANA time-zone database (the workspace turns on `jiff`'s `tzdb-bundle-always` feature, §16 decision 30), so zones resolve in a container image with no `tzdata`; the browser's build bundles it too, by `jiff`'s default on wasm.
    - `jiff` opens one database: the system's, which the operating system keeps current, when it finds one, and the bundled copy, which is as old as the release, only when it finds none. A system database that lacks a zone is not topped up from the bundle: that zone is unknown, as is a name that isn't an IANA zone.
    - No zone, or a zone `jiff` doesn't have, is read as UTC, and the zone-name style then writes `UTC`. The `browser-intl` host leaves the rules to the browser's `Intl`, and reads no zone, or one the browser doesn't have, as UTC in the same way.
- **Sorting.** Names users sort by (hosts, plans, repositories, paths in the snapshot tree) use the locale's collation with numeric ordering (`Intl.Collator`), so `host10` follows `host9`. Snapshot IDs and hashes sort by byte order. The API sorts only by stable keys (time, ID); name sorting happens in the UI.
- **Bidi.** Unicode isolation marks around placeables are a per-bundle option. They are on in the web UI only, and off for the CLI, notifications (plain-text sinks show the marks as boxes) and tests. The UI sets `lang` and `dir` on `<html>` from the negotiated locale and uses logical CSS properties only (`margin-inline-start`, never `margin-left`).
- **Core as built (M1b).** What the first implementation settled:
  - *Plain fallbacks.* Without a host formatter (a build with neither `icu` nor `browser-intl`) a `Count` is a Fluent number, a `Timestamp` is written `2026-11-08 13:30:45 UTC` (its zone is ignored; a style shows only its parts, `2026-11-08 UTC`, `13:30:45 UTC` or `UTC`; an instant no calendar date can show is its millisecond count), and `Bytes` and `DurationMs` go through the helpers below. A host plugs in once, when the bundle is built (`format::install` sets the formatter hook and adds `DATETIME`), and the features choose it, not a run-time switch, so no caller changes.
  - *Helpers.* `size` and `duration` are catalog messages, found from the locale that renders the message and then down the chain. Code splits the value. A size takes the largest IEC unit that keeps it at 1 or more, rounded to one decimal, and a value that would round to 1024 moves up a unit (1,048,575 B is `1 MiB`). A duration is its two largest units, never rounded up (119,999 ms is `1 min 59 s`).
  - *CLI messages.* The daemon prints `ladderback: ` and then the catalog message, in lowercase as before (`ladderback: listening on http://127.0.0.1:9898`), because scripts and `ui-tests` parse that line. The prefix is added in code. `clap`'s usage errors stay English.
  - *CLI help.* Every description `--help` gives, of the program, a command or an argument, is a catalog message: `cli-help-` and the command's names (`cli-help-mcp-call`), then the option's for an argument (`cli-help-serve-listen`). None is written in the code, not even as a doc comment `clap` would show. The catalog is read only when help is said, never on the start of the daemon or a worker. `clap`'s own words stay English: the headings, the usage line, `[default: …]`, and its `--help` and `--version` options and `help` command. So do the placeholders for a value (`<MINUTES>`, `<NAME>`), which the code names. Unit tests in `src/main.rs` walk the built command tree: no description is written in the code, and under `en-XA` none is missing or left in English. `features/cli/help.feature` reads each command's help the same way.
  - *CLI locale.* The locale is the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set and not empty. When it is `C`, `POSIX` or `C.UTF-8`, nothing is asked for, whatever `LANGUAGE` says (as in GNU gettext). Otherwise the entries of `LANGUAGE` come first and the locale last. When none of the three is set, `LANGUAGE` alone is honored, which is more lenient than gettext. A name over 64 bytes is ignored and at most 16 `LANGUAGE` entries are kept.
  - *Chain.* The negotiated chain ends at `en-US`, and a locale without a catalog is left out, as is one the build doesn't offer (*Scope*).
  - *Pseudolocales.* `en-XA` and `ar-XB` are asked for by their exact tag, in `?locale=` or among the preferences (`en`, `en-GB`, `ar` and `ar-XA` ask for none).
    - A pseudolocale is the `en-US` catalog, transformed, so it takes `en-US`'s place in the chain and goes ahead of it (`[de, ar-XB, en-US]`): it has every message, what is asked for after it is never reached, it never brings in the real language it imitates, and the first one asked for wins. Its bundle is `en-US`'s, so plural rules stay English.
    - Only text that comes out of the catalog changes. Variables, numbers and string literals don't, and neither does a message built in code, such as the operating system's own words in a daemon message. What a person types, such as an address, an option or a header, is a string literal in the catalog (`{ "--allowed-host" }`), so no transform or translation can alter it.
  - *Checking your text.* Open a development build of the UI at `?locale=en-XA` or `?locale=ar-XB` (the daemon: `LANG=en_US.UTF-8 LANGUAGE=en-XA`): text that stays plain English isn't from the catalog. Set a locale other than `C` together with `LANGUAGE`: under `C`, `POSIX` or `C.UTF-8`, which many shells and containers set, the daemon ignores `LANGUAGE` (*CLI locale*, above), so its text stays English and the check proves nothing. `LC_ALL` and `LC_MESSAGES` come before `LANG`, so unset them if they hold `C`.
  - *Offered languages.* `crates/l10n/offered-locales.txt`, one tag a line and `#` for a comment, lists the languages a release offers, `en-US` among them; `build.rs` refuses a list without `en-US`, a tag with no catalog and a tag listed twice. A build with the `pseudo` feature, a development one, offers every catalog it carries, so a translation can be reviewed in place; without it, `Localizer::new` negotiates only over the listed ones, and a request for any other is served in `en-US` (decision 101). `Localizer::offered` does the same over a test's own catalogs; `Localizer::from_catalogs` offers every catalog it is given.
  - *Translations.* A language's catalog is `locales/<tag>/*.ftl`, and its `sources.txt` records, for each message accepted after review, its ID and the SHA-256 of the `en-US` text it was translated from: the message's lines from its ID to its last line, without the spaces at their ends (decision 101). `just translations [tag…]` lists, for each language, the messages that are missing, translated but not reviewed, stale (with the English each was translated from, read from the git history, and the English now) or gone from `en-US`, and exits 0 whatever it finds. Check a translation in a development build, as under *Checking your text*, with its tag for `?locale=` or `LANGUAGE`.
    - After a review, `just accept-translations <tag> <id>…` writes the lines of the messages named, stale ones included. `--all` writes those of every translated message not reviewed yet (no line in `sources.txt`) and never a stale one, so marking a stale translation current is always a deliberate act, one message at a time: it names the stale messages it left out, says how many, and that each is accepted by naming its ID (#860).
  - *Page language.* `Localizer::language()` is the first locale of the chain, with `ar` for `ar-XB`, so that it runs right to left, and `en-XA` as it is. `Localizer::direction()` follows the script of that tag and never the text, so Arabic, Hebrew and the other right-to-left scripts are `rtl` and everything else is `ltr`: HTML's `dir` has no vertical value.
  - *Web locale.* The page's preferences are `?locale=` (any valid tag; anything else is ignored), then the language chosen in Settings, then `navigator.languages`. `BrowserLanguages` reads them within the POSIX environment's bounds: a name over 64 bytes is ignored, at most 16 of the browser's languages are kept besides the one in the address and the one chosen, and `und` is no preference. A pseudolocale stands in for `en-US` wherever it comes in the preferences, so when the address asks for a locale, no pseudolocale chosen or listed comes after it: `?locale=en-US` shows American English whatever was chosen. Before the first render, `crates/web/src/page.rs`, the only DOM glue, sets `lang` and `dir` on `<html>` (always, `ltr` too) and the document title from the localizer. `index.html` keeps a static `lang="en"` for the moment before the app loads, and the CSS is logical only.
  - *Language choice.* Settings' Language section (§4.4) offers "The browser's languages", the choice until another is made, then each language of `l10n::languages()`: the catalogs the build offers (*Offered languages*), then, in a development build, the pseudolocales, each named in itself by its catalog's `ui-language-name` ("English", "日本語"; `en-XA` and `ar-XB` transform "English"), its label marked with that language's `lang` and `dir` (WCAG 3.1.2). The choice is the browser's, not the daemon's: a person who reads two languages on two machines sees each in its own, and the CLI and the notifications keep their own rules above.
  - *Keeping the choice.* Saving it keeps the tag in the browser's `localStorage` (`ladderback-language`), or forgets it for the browser's languages, then loads the page again at `/settings#language`, so the one startup path negotiates the language and declares `lang`, `dir` and the title, and focus comes back to the section's heading rather than the top of the page (§4.5, "Route changes"). Loading it again drops anything else Settings was showing, such as a new API token's secret shown once, so the section's hint says that saving reloads the page. A browser that refuses to keep it is told so in an alert, and the page stays as it was.
  - *A kept tag.* It counts only as Settings keeps it, a tag the build offers written exactly as `l10n::languages()` gives it (`l10n::offers`), so the page and Settings never disagree: any other, such as one the build no longer offers or `EN-xa`, asks for nothing, and Settings shows the browser's languages chosen.
  - *Release builds.* `pseudo` is a feature of `l10n`, `ladderback` and `web`, on by default. It brings `fluent-pseudo` and, with it, a regex engine, so release packaging (§12.1) builds with `--no-default-features`, the Justfile's `release_flags` (`trunk build --release --no-default-features` for the UI). There `?locale=en-XA` asks for a language no catalog serves and gets `en-US`, and only the listed languages are offered (*Offered languages*). `just architecture` runs `cargo tree` for both builds of both crates and fails when `fluent-pseudo` is missing from a default build or present in a release one. The web UI's release wasm is 446,878 bytes without the feature and 1,434,409 with it, from 68,144 for the page before it had a catalog; the daemon's size is measured with ICU4X.
  - *Code messages.* `l10n::code` lists each code once, and that list makes both the exhaustive `match` and the table, so a variant without a line doesn't compile. A message is `error-<wire name>` or `warning-<wire name>`. `api` declares the arguments a code carries (`ErrorCode::args()`), and a test checks that the message reads exactly those variables, plus `$detail` in its `.with-detail` attribute. The detail is never an argument, so no argument can stand in for it.
  - *Optional arguments.* An argument a code declares optional (`ArgSpec::optional`, such as the `exit_code` of `engine-exited`, left out when no exit status is known) is always text. When the sender left it out it reaches the message as the text `none`, so the message selects on it (`{ $exit_code -> [none] … *[other] … }`). Every text of the message, the value and each attribute, in every locale, reads it only through a select that has a `[none]` arm: as its selector or in the arms other than `[none]`. A catalog test checks the source locale and every translation, and another checks that an optional argument is text.
  - *Counts and details.* A `Count` up to 2^53 is a Fluent number, which holds integers exactly up to there. A larger one is passed as its digits, which no host formats (so they are not grouped), and selects the `other` plural form, so it is never written wrongly. A detail that is empty or only whitespace counts as none, so a message never ends in a dangling colon.
  - *Terms and files.* Only `-brand-name` exists so far; a glossary term arrives with the first message that uses it, except in a translation, which settles all of the glossary's terms before its first message (decision 101). `common.ftl` holds the terms, the helpers and the messages of the codes, `cli.ftl` the daemon's own messages, `ui.ftl` the web UI's and `notify.ftl` the default text of each notification (§11), one message per event (`notify-backup-failed` for `backup.failed`). A notification's text is sent to a machine, so the notifier formats it without isolation marks.
  - *Release build's tests.* `just test-release` runs `cargo nextest run -p l10n -p ladderback --no-default-features`, the build a release is, which `just test` doesn't cover because it builds with `pseudo` on: the `l10n` release lane (`tests/lanes/release.rs`), the release half of `tests/lanes/offered.rs` and the daemon's no-pseudo test (`tests/cli_locale.rs`) run only there. CI runs it in the `Release-build tests (no default features)` job.

**Catalog rules.**

- IDs are semantic and kebab-case: `plan-run-now`, `op-status-failed`, `error-claim-rejected`.
- Every message is a complete sentence or label; code never concatenates fragments.
- Counts use selectors with CLDR categories and a `*[other]` default.
- The ubiquitous language (§3.1) becomes Fluent terms (`-snapshot`, `-repository`, `-plan`, `-agent`, `-hub`, `-brand-name`). UI copy uses the same words as the glossary, and translators translate each term once.
- One element's texts are grouped as attributes (`.aria-label`, `.placeholder`, `.tooltip`, and `.spoken` for a spoken form of abbreviated text, §4.5). A translation of a message must carry all of its attributes, because fallback happens per message.
- A translator comment precedes every message with variables, saying what each variable holds.
- Voice follows [PRODUCT.md](../../PRODUCT.md): calm and exact; a warning says what happened and what to do next.

**Tests** (test first, as everything else; §13):

- **Catalog lane:**
  - Every `.ftl` file of every locale parses and adds to a bundle without errors (syntax, duplicate IDs, missing default variants).
  - Every `l10n::msg` ID formats in `en-US` without errors, using a fixture of representative arguments that covers each selector branch.
  - Every `ErrorCode` and `WarningCode` variant has an `en-US` message.
  - In other locales, a message absent from `en-US` fails, as does a message missing one of its `en-US` attributes. Missing messages are allowed; they fall back.
  - Assertions run with isolation marks off, plus one test with them on.
- **Formatting lane:**
  - `NUMBER($n)` drives a plural selector correctly under the host formatter. This guards the trap of a formatter that returns text, which would send every count to `*[other]`.
  - `NUMBER`, `DATETIME`, sizes and durations format as expected in `en-US`, `de`, `ja`, and in `ar`, `bn`, `hi` (other digits), `fa` (Persian calendar) and `th` (Buddhist calendar). One table of cases (`crates/l10n/tests/expectations/formatting.json`, made from Node's `Intl` by `generate.ts`) is run through the public `Localizer` by each host's lane: the `icu` lane runs with the locale data the daemon is built with, and `browser-intl` runs the same table under `wasm-bindgen-test` in Node (`just test-wasm`), folding U+202F and U+00A0 to U+0020, which V8 writes differently from CLDR. Where Node's ICU (CLDR 48) and ICU4X's data (CLDR 49) write different words, the generator leaves the case out and lists it (`UNSETTLED`): the short zone names of `ar`, the name of UTC in `ar` and `bn`, the short date of `th` and the long date of `hi`.
  - Node may run another ICU than the one that wrote the table: an official build bundles its own, Homebrew's links the system's, and ICU 79.1 writes a Thai time `14:30 น.` where ICU 78.3 writes `14:30`.
    - So in the `browser-intl` lane an instant, in every locale, is held to two things: its text is what the running `Intl` writes for the case, built from the case's options and zone rather than from the localizer's; and the numbers in it, read whatever their digits, are the table's. The words, suffixes, separators and digits are the locale's data.
    - The numbers carry what Ladderback asks for (the locale and its calendar, the zone the instant is read in, which fields are shown) and the hour cycle the table was made with, so a 12-hour clock where the table has a 24-hour one fails too. That hour cycle, and how many digits a year has, are still CLDR's: a CLDR release that changed either for a table locale would fail the lane on correct code.
    - A short zone name has few numbers or none, so its zone is pinned by the date and time cases of the same zone. A number, count, size or duration is still the table's text.
- **Unused IDs:** a CI lint fails if a generated message ID is referenced nowhere in the workspace, so dead strings can't pile up.
- **Pseudolocales.** Built from `en-US` with `fluent-pseudo` (behind a `pseudo` feature, off in release builds) through the bundle's transform hook. Dev builds select them with `?locale=…`:
  - `en-XA`: accented Latin letters with doubled vowels.
  - `ar-XB`: upside-down Latin letters, rendered with `lang="ar"` and `dir="rtl"`. This tests right-to-left layout, not Arabic shaping. The bundle keeps the `en-US` locale so English plural rules still apply.

  From M1b, Playwright runs the UI journeys in both. A run fails on clipped or overflowing text, on layout that breaks right-to-left, and on any visible text node with no transformed character, unless the node is data: IDs, paths, numbers, hostnames, or verbatim rustic text marked `lang="en"`. A language's name written in itself, as Settings' languages show it, is its own catalog's, not the page's: a node marked with a catalog's tag (`lang="en-US"`) whose text is that catalog's `ui-language-name` ("English") isn't inspected. Any other text marked with a `lang` is.

**Security.** Translations are compiled in and rendered as text nodes; Leptos escapes them, and nothing passes translated text to `inner_html`. Fluent's markup and DOM-overlay features are not used. Arguments are redacted when codes are built (above); the I4 canary test covers the `args` and `detail` paths too.
