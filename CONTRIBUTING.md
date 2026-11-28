# Contributing to Ladderback

Ladderback is built for its author's own machines first and published for other rustic users, without support promises. Questions, ideas, issues, design discussion and test reports are welcome; pull requests are welcome once an issue has agreed on the change.

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).

## Asking a question

Ask in the [Q&A category of Discussions](https://github.com/nledford/Ladderback/discussions/categories/q-a), not in an issue. Its form asks for the same version, system and installation as a bug report, and an answer marked there helps the next person who searches. An answer may take a while.

## Reporting a bug

Open an issue with the **Bug report** form. It asks for the version (the web UI's footer, or `ladderback --version`), the system, how Ladderback was installed, and the steps that show the problem.

Never paste a password, a key, an API token or a repository URL that holds credentials. Ladderback keeps secrets out of its own logs and messages, but a configuration file or a shell history may hold them, and a log may name paths and machines you'd rather not share.

A security problem goes through the [security policy](SECURITY.md), never a public issue. An accessibility barrier has a form of its own, **Accessibility barrier**; [ACCESSIBILITY.md](ACCESSIBILITY.md) says what Ladderback aims at.

## Asking for a feature

Open an issue with the **Feature request** form, and say what you are trying to do rather than only the change you have in mind. An idea you'd like to talk through first goes in the [Ideas category of Discussions](https://github.com/nledford/Ladderback/discussions/categories/ideas); one the maintainer agrees on becomes an issue. [PRODUCT.md](PRODUCT.md) says what Ladderback is for and what it leaves to rustic; [the roadmap](docs/design/roadmap.md) says what is planned.

## Testing on a real machine

Some setups haven't been tried on a real machine yet: other Linux distributions and desktops, keyrings, ARM, headless servers. Each [issue labeled `needs real-world test`](https://github.com/nledford/Ladderback/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22needs%20real-world%20test%22) lists the steps and what to report. Use a scratch repository and a made-up password, and report with the **Real-world test report** form.

## Pull requests

1. **Agree on the change in an issue first.** A pull request nobody asked for may be closed, however good it is, because each change has to fit the design.
2. **Follow the development method.** Behavior is described first as a Given/When/Then scenario, then built test-first: a pull request that changes behavior starts with a `test:` commit whose tests fail. The method is in [docs/design/development.md §13](docs/design/development.md#13-development-method), and the working rules, the same for people and AI tools, are in [AGENTS.md](AGENTS.md).
3. **Run the checks.** `just setup` installs the tools; `just quick-ci` runs the quick local checks, and CI runs the rest. `just` lists every recipe.
4. **Keep the safety invariants.** §0 of [the technical design](docs/design/README.md) lists them; a change that weakens one doesn't merge.
5. **Title the pull request as a Conventional Commit** (`feat:`, `fix:`, `docs:` …), written for users: it becomes the changelog entry. `just check-pr-title "<title>"` checks it.

Contributions are licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at the reader's option, as the project is, unless you say otherwise. No code may be copied, ported or paraphrased from Backrest, which is GPL-3.0.
