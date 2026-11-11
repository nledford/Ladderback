# Decision 17: Internationalization

**Localization-ready from the first UI string; `en-US` only in v1.** Project Fluent via `fluent-bundle` in a shared `l10n` crate, with a per-message fallback chain of bundles; the core and the wire carry codes and typed arguments, never sentences; dates and numbers through browser `Intl` or ICU4X behind Fluent's formatter hook; machine-facing text never localized (§8.4).
