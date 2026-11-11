# Decision 14: Development method

**Behaviour first and test first, for all production code.** Gherkin scenarios run by cucumber-rs against use cases with fakes, `@e2e` scenarios against the real binary; red-first check and `cargo mutants --in-diff` in CI instead of a coverage gate (§13).
