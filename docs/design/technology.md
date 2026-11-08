## 8. Technology choices

### 8.1 Frontend: Leptos CSR with an explicit REST API

| Option | Pros | Cons |
|---|---|---|
| **CSR with explicit REST + WebSocket API** (chosen) | Single `trunk` build output that's easy to embed. The API is a real, versioned contract that the hub, the CLI and a future Tauri shell can all reuse. Shared types via an `api` crate | A bit more boilerplate than server functions |
| SSR + hydration with server functions | Less API code. Fast first paint | Couples UI and server build (`cargo-leptos`, two targets). Server fns aren't a stable contract for agents or a CLI. SEO and first paint don't matter for a localhost dashboard |

UI kit: plain Leptos with Tailwind (with the `trunk` Tailwind hook) for styling, and Steading's own small component set for every widget, built to the accessibility contracts in §4.5. Tailwind arrives with that component set in M1b; M0's hello page uses one plain stylesheet with the color, focus and motion tokens. Charts (snapshot sizes over time, durations) use SVG rendered in Leptos, with no JS chart library.

### 8.2 Backend crates

| Concern | Crate |
|---|---|
| Async runtime / HTTP | `tokio`, `axum 0.8`, `tower-http` |
| Engine | `rustic_core`, `rustic_backend` (one pinned git revision until a release has `Progress::error`; wrapped in our `engine` crate) |
| DB | `sqlx 0.8` (SQLite, migrations; runtime-checked queries with bound parameters, because compile-time-checked ones need `sqlx-cli` and committed query data in CI). 0.9 needs a newer compiler than our MSRV, so we stay on 0.8 |
| Logging | `log` facade; `env_logger` in the daemon, writing to stderr at `info` unless `RUST_LOG` says otherwise |
| Scheduling | `croner` (with its `jiff` feature, not `chrono`) |
| Embedding UI | `rust-embed` |
| Auth | `argon2`, `axum-login` or a small cookie-session layer |
| Crypto / identity | `ed25519-dalek`, `rustls` |
| Secrets | `keyring` (macOS Keychain, Secret Service on Linux) |
| Notifications | `reqwest` (webhooks, ntfy, Gotify, healthchecks.io), `lettre` (SMTP) |
| Localization | `fluent-bundle`, `fluent-langneg`, `unic-langid`; `icu` (ICU4X) for dates and numbers in daemon-rendered text (§8.4) |
| CLI | `clap` |

The git dependency means Steading can't be published to crates.io until it moves to a release. Distribution doesn't depend on crates.io (§12).

### 8.3 Workspace layout

The crates follow §3.2: two inner rings, one crate per adapter, and a composition root.

```
steading/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ domain/                 # aggregates, value objects, domain services, events; no I/O; builds for wasm32
│  ├─ app/                    # use cases, runtime loops, port traits, in-memory fakes and port contract suites
│  │                          # (feature "fakes"); tests/acceptance.rs runs non-@e2e scenarios
│  ├─ api/                    # wasm-safe DTOs: REST, UiEvent, sync protocol (the published language)
│  ├─ testkit/                # test support, a dev-dependency only: `skip!` (§13.5)
│  ├─ web/                    # Leptos CSR app (trunk); inbound adapter in the browser
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
│     ├─ interop/             # rustic profile import/export (anti-corruption layer)
│     ├─ secrets/             # command, keyring, file, env sources; `${…}` substitution (redaction: domain `Redactor`)
│     ├─ notify/              # webhook, ntfy, Gotify, Discord/Slack, SMTP, healthchecks
│     ├─ mcp/                 # `steading mcp`: JSON-RPC 2.0 over stdio; in-process and /api/v1 backends (§4.6)
│     └─ platform/            # clock, power, network, identity key, service install (systemd/launchd)
├─ features/                  # Gherkin scenarios, one folder per bounded context (§13.2)
├─ tests/e2e.rs               # cucumber-rs runner for @e2e scenarios against the real binary
├─ ui-tests/                  # Node: @cucumber/cucumber + Playwright + axe-core for @ui scenarios and the
│                             #   component gallery (§4.5)
├─ src/main.rs                # composition root; `steading` binary: serve | worker | mcp | pair | import-profile
│                             #   | export-profile | install-service | db rebuild
├─ scripts/                   # CI checks behind the Justfile: banned crates (§3.2), red-first (§13.1), skip log (§13.5)
├─ Justfile                   # every workflow, for people, agents and CI alike
├─ migrations/
└─ docs/
```

`adapters/engine` is the only crate that depends on `rustic_core`, which contains the impact of its pre-1.0 API churn. `api` compiles to `wasm32` and never depends on `rustic_core` or an adapter; where the UI needs a domain value it either reuses `domain` (also wasm-safe) or defines a DTO, with a round-trip test per DTO. `JobSpec` and `WorkerEvent` only travel between the daemon and the worker, both of which link `adapters/engine`, so they live in `engine::job`. They reuse the serde-capable core types and carry Steading's own serde mirrors of the rest (`PruneOptions`, `CheckOptions`, `RestoreOptions`, `LsOptions`, `CheckResults`). The engine adapter translates `WorkerEvent` into `app`'s `EngineEvent`.

### 8.4 Internationalization and localization (Project Fluent)

**Scope.** v1 ships **American English (`en-US`) only**, but it is localization-ready from the first user-facing string. Adding a language touches only the `l10n` crate: drop `.ftl` files into `locales/<tag>/`. Its `build.rs` regenerates the locale table and the embedded catalogs, and the ICU4X data, if trimmed to the shipped locales, is regenerated with them. Retrofitting this after the UI exists would mean touching every view, so it starts in M1b with the first real screen.

| Text | Localized? |
|---|---|
| Web UI: labels, messages, errors, `aria-*` text, page titles | Yes, in the browser's negotiated locale |
| Default notification text (ntfy, Gotify, Discord/Slack, email) | Yes, in the configured locale |
| Steading's own CLI messages and errors | Yes, from the POSIX locale. `clap`'s generated help and usage errors stay English |
| The static `index.html` shown before the wasm app loads | No: a minimal `lang="en"` loading text, replaced on startup |
| Logs, `config.toml` keys, API and sync field names, webhook JSON keys and event names, MCP tool names, arguments and results, exported rustic profiles | **Never.** These are machine-facing or grep targets |
| Text from rustic itself (warning and error details) | Shown verbatim, in English, inside a localized frame (`backup-failed = Backup failed: { $detail }`), marked `lang="en"` in the UI |
| User-written notification templates (§11) | Never; the user's own words |

**Errors cross the hexagon as codes, not sentences.**

- `domain` and `app` return typed errors and outcomes (`Failed { reason: DeadlinePassed }`, `ClaimRejected { owner }`). No English string is produced inside the core, so the core needs no locale and its tests never assert on wording.
- Adapters map them onto the published codes in `api`: closed enums `ErrorCode` and `WarningCode`, each with a `#[serde(other)] Unknown` variant.
- Arguments are typed: `MsgArg = Count(u64) | Bytes(u64) | DurationMs(u64) | Timestamp { utc_ms, zone: Option<IanaZone> } | Text(String)`. Identifiers, ports, years and hostnames are `Text`, so they are never digit-grouped.
- The REST API, the sync protocol, `WorkerEvent` (§4.1) and SQLite (§9) all carry `{ code, args, detail? }`, never rendered text. `detail` is rustic's own verbatim message.
- Codes are rendered by matching the enum, never by using a wire string as a message ID. An `Unknown` code, say from a newer agent, renders as `error-unknown = An unexpected error occurred.` followed by its detail. It has no `$code` to show: `#[serde(other)]` discards the wire string.
- **Redaction happens where codes are built:** `args` and `detail` pass through the `Redactor` before they are serialized, stored or synced (I4). `l10n` never sees a secret.

**The `l10n` crate.**

- **Placement.** It is a presentation library: neither a ring nor an adapter. It depends only on `api` (for the codes), the Fluent and formatter crates, and `jiff` (for the plain UTC date). `web`, `adapters/notify` and the CLI in the composition root use it; `domain` and `app` must not (§3.2 banned crates).
- **Fallback chain.** One `FluentBundle` holds one locale's messages; its plural rules come from that locale. The negotiated chain (always ending in `en-US`) is therefore an ordered `Vec` of bundles, one per locale, and a lookup falls through per message. The browser uses the single-threaded `FluentBundle`; the daemon uses `fluent_bundle::concurrent::FluentBundle`, shared across tasks.
- **Generated catalog table.** `build.rs` scans `locales/`, embeds every file with `include_str!`, and generates the message-ID constants in `l10n::msg` from `en-US`. A typo is a compile error, and the constants can't drift from the catalog. There are no runtime-loaded or remote catalogs.
- **Locale negotiation:** `fluent-langneg`, filtering strategy, default `en-US`.
  - UI: the user's language choice (stored in the browser), then `navigator.languages`.
  - Notifications: `[agent] locale` on the hub or standalone agent, then the service's POSIX locale, then `en-US`. Services under systemd or launchd usually have no `LANG`.
  - CLI: `LC_ALL`, `LC_MESSAGES` and `LANG`, with `LANGUAGE` (a list) ahead of the locale; the rules are under *Core as built*. POSIX values are normalized before parsing: strip the codeset and modifier, turn `_` into `-`, and ignore `C` and `POSIX`.
  - Tags are parsed with `unic-langid`. An invalid tag is ignored; tags never reach a file path, because catalogs are compiled in.
- **Numbers.** `fluent-bundle`'s built-in `NUMBER` keeps a value numeric, so plural selectors work, but it formats without locale-aware grouping. Steading keeps the built-in and does locale-aware output in the bundle's `set_formatter` hook. That hook is a plain `fn`, so it reaches the host formatter through the bundle's memoizer (`intl_memoizer::Memoizable`, keyed by locale).
- **Dates.** `fluent-bundle` has no `DATETIME`. Steading registers one that returns a `FluentValue::Custom` value, which formats itself through the same memoized host formatter.
- **Host formatters,** each behind a feature of `l10n`:
  - `icu`: ICU4X (`icu_decimal`, `icu_datetime`) for the daemon and CLI. Binary size is measured in M1b; ICU4X brings a second time-zone database next to `jiff`'s.
  - `browser-intl`: `js_sys::Intl` for `web`, with formatters built per locale and cached in the memoizer, since a `JsValue` can't be shared across threads. No locale data ships to the browser.
- **Sizes and durations.** Byte sizes pick the IEC unit in code (KiB, MiB, GiB) and format the number through `NUMBER`, with the unit label in the catalog. Durations are catalog messages over numeric parts (`{ $hours } h { $minutes } min`), so word order stays with the translator.
- **Time zones.** Timestamps travel as UTC plus an optional IANA zone.
  - The UI shows times in the viewer's browser zone. A scheduled run whose plan has its own zone shows both ("02:00 Europe/Berlin, 03:00 your time").
  - Notification text uses the sending agent's local zone and names it.
- **Sorting.** Names users sort by (hosts, plans, repositories, paths in the snapshot tree) use the locale's collation with numeric ordering (`Intl.Collator`), so `host10` follows `host9`. Snapshot IDs and hashes sort by byte order. The API sorts only by stable keys (time, ID); name sorting happens in the UI.
- **Bidi.** Unicode isolation marks around placeables are a per-bundle option. They are on in the web UI only, and off for the CLI, notifications (plain-text sinks show the marks as boxes) and tests. The UI sets `lang` and `dir` on `<html>` from the negotiated locale and uses logical CSS properties only (`margin-inline-start`, never `margin-left`).
- **Core as built (M1b).** What the first implementation settled:
  - *Plain fallbacks.* Without a host formatter a `Count` is a Fluent number, a `Timestamp` is written `2026-11-08 13:30:45 UTC` (its zone is ignored until `icu` formats it), and `Bytes` and `DurationMs` go through the helpers below. The feature-gated formatters plug in at bundle construction (`set_formatter`, `DATETIME`) and at `format::instant`, so no caller changes.
  - *Helpers.* `size` and `duration` are catalog messages, found from the locale that renders the message and then down the chain. Code splits the value. A size takes the largest IEC unit that keeps it at 1 or more, rounded to one decimal, and a value that would round to 1024 moves up a unit (1,048,575 B is `1 MiB`). A duration is its two largest units, never rounded up (119,999 ms is `1 min 59 s`).
  - *CLI messages.* The daemon prints `steading: ` and then the catalog message, in lowercase as before (`steading: listening on http://127.0.0.1:9898`), because scripts and `ui-tests` parse that line. The prefix is added in code. `clap`'s own help and usage errors stay English.
  - *CLI locale.* The locale is the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set and not empty. When it is `C`, `POSIX` or `C.UTF-8`, nothing is asked for, whatever `LANGUAGE` says (as in GNU gettext). Otherwise the entries of `LANGUAGE` come first and the locale last. When none of the three is set, `LANGUAGE` alone is honored, which is more lenient than gettext. A name over 64 bytes is ignored and at most 16 `LANGUAGE` entries are kept.
  - *Chain.* The negotiated chain ends at `en-US`, and a locale without a catalog is left out.
  - *Code messages.* `l10n::code` lists each code once, and that list makes both the exhaustive `match` and the table, so a variant without a line doesn't compile. A message is `error-<wire name>` or `warning-<wire name>`. `api` declares the arguments a code carries (`ErrorCode::args()`), and a test checks that the message reads exactly those variables, plus `$detail` in its `.with-detail` attribute. The detail is never an argument, so no argument can stand in for it.
  - *Optional arguments.* An argument a code declares optional (`ArgSpec::optional`, such as the `exit_code` of `engine-exited`, left out when no exit status is known) is always text. When the sender left it out it reaches the message as the text `none`, so the message selects on it (`{ $exit_code -> [none] … *[other] … }`). Every text of the message, the value and each attribute, in every locale, reads it only through a select that has a `[none]` arm: as its selector or in the arms other than `[none]`. A catalog test checks the source locale and every translation, and another checks that an optional argument is text.
  - *Counts and details.* A `Count` up to 2^53 is a Fluent number, which holds integers exactly up to there. A larger one is passed as its digits and selects the `other` plural form, so it is never written wrongly. A detail that is empty or only whitespace counts as none, so a message never ends in a dangling colon.
  - *Terms and files.* Only `-brand-name` exists so far; a glossary term arrives with the first message that uses it. `notify.ftl` arrives with the notifier (M2).
  - *Not built yet.* ICU4X formatting and `DATETIME`, the `browser-intl` feature, the web UI's locale choice, `lang` and `dir`, pseudolocales, and the binary-size measurement.

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
  - `NUMBER`, `DATETIME`, sizes and durations format as expected in `en-US`, `de` and `ja`. The `icu` formatter runs with full locale data (a test-only feature); `browser-intl` runs the same cases under `wasm-bindgen-test` in a headless browser.
- **Unused IDs:** a CI lint fails if a generated message ID is referenced nowhere in the workspace, so dead strings can't pile up.
- **Pseudolocales.** Built from `en-US` with `fluent-pseudo` (behind a `pseudo` feature, off in release builds) through the bundle's transform hook. Dev builds select them with `?locale=…`:
  - `en-XA`: accented Latin letters with doubled vowels.
  - `ar-XB`: upside-down Latin letters, rendered with `lang="ar"` and `dir="rtl"`. This tests right-to-left layout, not Arabic shaping. The bundle keeps the `en-US` locale so English plural rules still apply.

  From M1b, Playwright runs the UI journeys in both. A run fails on clipped or overflowing text, on layout that breaks right-to-left, and on any visible text node with no transformed character, unless the node is data: IDs, paths, numbers, hostnames, or verbatim rustic text marked `lang="en"`.

**Security.** Translations are compiled in and rendered as text nodes; Leptos escapes them, and nothing passes translated text to `inner_html`. Fluent's markup and DOM-overlay features are not used. Arguments are redacted when codes are built (above); the I4 canary test covers the `args` and `detail` paths too.
