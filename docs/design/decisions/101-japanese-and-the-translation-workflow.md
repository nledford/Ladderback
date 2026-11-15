# Decision 101: Japanese, and how a translation is kept

**Japanese (`ja`) is the first language after American English, in M4. An AI drafts its messages and the maintainer reviews them before each release. A language ships in a release only when every message is translated and none is stale; between releases, CI lists what is missing or stale without failing** (recorded 2026-11-15; decided by the maintainer; §8.4, §14).

- **Drafts and review.** An AI drafts each Japanese message. The pull request that adds or changes messages gives, for each one, an English back-translation and a note on any term or phrasing it was unsure of, so the review checks meaning as well as grammar. The maintainer, who reads and writes Japanese, reviews every message before it ships.
- **Terms first.** The glossary's terms (§3.1) get their Japanese form first, as Fluent terms (`-snapshot`, `-repository`, `-plan`, …), reviewed once and then used by every message. The voice is PRODUCT.md's, calm and exact, in the polite form (です・ます).
- **Missing and stale.** `locales/ja/` keeps, for each message, a hash of the `en-US` text it was translated from, its value and attributes together. A message whose `en-US` text has changed since is stale; one with no translation is missing. A `just` recipe lists both, and CI prints that list on every pull request without failing: an English change never waits for its Japanese.
- **Shipping.** A release offers only languages that are complete and current. The release pull request's checks (§12.1) refuse one that offers a language with a missing or stale message. A language not yet offered is still built, but negotiation skips it, so a release never mixes Japanese and English on one screen; development builds offer it, as they do the pseudolocales, for review.
- **Choosing a language.** The stored language choice §8.4 leaves unbuilt arrives with Japanese, in the UI's settings, ahead of the browser's languages.
- **Layout.** Japanese has no spaces between words, so the UI sets line breaking by CSS (`line-break: strict`, and `word-break: auto-phrase` where the browser has it) and uses the system's Japanese fonts. The `@ui` journeys run under `ja` as well as the pseudolocales, which test length and direction but not CJK text.

**Why:** batching per release keeps every English change free of a translation step, while the release gate keeps an incomplete translation from reaching anyone. The hash makes "stale" a fact a script can check rather than something a reviewer has to remember.

The CI listing and the release check are CI changes, so each lands in its own pull request.

Right-to-left languages, such as Arabic and Hebrew, come later under the same workflow; each needs a reviewer who reads it (#704).
