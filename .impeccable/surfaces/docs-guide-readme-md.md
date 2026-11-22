---
version: 1
slug: "docs-guide-readme-md"
primary_target: "docs/guide/README.md"
related_targets: ["docs/guide"]
---

## Scope and mode

The user guide site built from `docs/guide/` with mdBook and published on GitHub Pages, in two parts: Using Ladderback (install, Getting started in the web UI) for everyone, then Advanced (other ways to install, the service, plans in `config.toml`, secrets, sign-in and remote access, API tokens and AI agents). Mode: **Read**. The visitor understands something, then goes and does it in a terminal or the web UI.

## Audience, job, task

- New users installing for the first time, often on a headless server, copying commands.
- Returning users looking up one exact answer (a flag, a permission, a fix for a refusal).

## Proof and content

`just docs-site` builds it into a local folder; serve that folder on a spare port to critique it. The published site is the same build from `main`.

## Constraints

- mdBook's theme with the project's small overrides in `docs/guide/theme/`; no custom framework.
- axe-core clean on every page (`just docs-site-a11y`); paragraphs at most 150 words; American English.
- Commands and options are copyable literals, never paraphrased.

## Unresolved decisions

- The guide is built from `main`, so it can describe features the latest release lacks; whether pages mark unreleased features.
