# Decision 101: Japanese, and how a translation is kept

**Japanese (`ja`) is the first language after American English, in M4. An AI drafts its messages and the maintainer reviews them before each release. A release offers a language only when every message is translated and none is stale; between releases, CI lists what is missing or stale without failing** (recorded 2026-11-15; §8.4 holds the rules, §14 the milestone).

**The maintainer's choices** (2026-11-15):

- **Who:** an AI drafts each message, and the maintainer, who is learning to read and write Japanese, reviews every message before each release.
- **Shipping:** a release offers a language only when it is complete and current: every message translated, none stale. Otherwise that release keeps it hidden.
- **Keeping it in step:** batched before each release. CI lists missing and stale messages on every pull request without failing, and the release check requires none, so an English change never waits for its Japanese.
- **Staleness** is a fact in the repository: a hash, per translated message, of the `en-US` text it came from.
- **When:** M4.

**How it works** (proposed with the decision; the maintainer may change any of it):

- **Offered languages.** `crates/l10n/offered-locales.txt` lists the tags a release offers; `build.rs` reads it. Builds without the default features, the ones a release is, negotiate only over the listed tags, for the UI, the CLI and notifications alike, and ignore a request for any other catalog they carry. Default builds offer every catalog, as they do the pseudolocales, so a translation can be reviewed in place. The release pull request's checks (§12.1) refuse a listed language with a missing or stale message, so a release never mixes catalog text from two languages. rustic's own messages, and the operating system's words in a daemon message, stay English as everywhere (§8.4).
- **Hashes.** `locales/<tag>/sources.txt` holds one line per translated message: its ID and the SHA-256 of its `en-US` value and attributes together. A `just` recipe writes a message's line when its translation is accepted after review, so "current" also means "reviewed". The same recipe lists the missing messages and the stale ones, with the old and new English of each stale one.
- **Drafts.** The pull request that adds or changes messages gives, for each one, an English back-translation and a note on any term or phrasing the draft was unsure of, so the review checks meaning as well as grammar.
- **Terms first.** A translation settles the glossary's terms (§3.1) before its first message, as Fluent terms (`-snapshot`, `-repository`, `-plan`, …), reviewed once. Japanese uses the polite form (です・ます), in PRODUCT.md's calm and exact voice.
- **Choosing a language.** The stored language choice §8.4 leaves unbuilt arrives with Japanese, in the UI's settings, ahead of the browser's languages.
- **Layout.** Japanese has no spaces between words, so the UI sets line breaking by CSS (`line-break: strict`, and `word-break: auto-phrase` where the browser has it) and uses the system's Japanese fonts. The `@ui` journeys run under `ja` as well as the pseudolocales, which test length and direction but not CJK text.

**Why:** batching per release keeps every English change free of a translation step, while the release gate keeps an incomplete or unreviewed translation from reaching anyone. The hash makes "stale" a fact a script can check rather than something a reviewer has to remember.

The CI listing and the release check are CI changes, so each lands in its own pull request (#705). Right-to-left languages, such as Arabic and Hebrew, come later under the same workflow, once a reviewer who reads each one volunteers (#704).
