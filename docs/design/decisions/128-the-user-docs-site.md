# Decision 128: What builds the user docs site, and from where

**mdBook, at a release the Justfile pins (0.5.4), builds the user docs site from `docs/guide/` where it is: `book.toml` and `SUMMARY.md` live in that folder, and `just docs-site` builds it into `docs/guide/book/`. The design documents stay off the site, and its pages link to them, and to any other file outside `docs/guide/`, on GitHub. A link that doesn't resolve fails the build, and `just lint` runs it. The site meets WCAG 2.2 AA like the product, with mdBook's theme fixed where axe-core found it short. A Pages workflow publishes it at `https://nledford.github.io/Ladderback/`, in a pull request of its own (#1177, #216).**

## Why

People running Ladderback need guides of their own: the design documents are for contributors. mdBook is Rust tooling, installed as one prebuilt binary with `cargo binstall` like the project's other tools; readers get static pages with search built in and no runtime of ours. Its pages are plain Markdown, so the same files still read well on GitHub, where the README, the released install script's error message and the Containerfile's label already point.

## Decided

- **The version is pinned** (`mdbook_version` in the Justfile): `just setup` installs it, CI reads the same line, and `just docs-site` refuses another release, since another can render pages and anchors differently.
- **In place, not moved.** The guide's pages stay in `docs/guide/`: `crates/adapters/config/tests/guide.rs` reads them, and every released `install.sh` carries a link to `docs/guide/install.md#by-hand` that a move would break. The maintainer's scope notes on #1177 (2026-11-20) settled this over the earlier `docs/user/` plan.
- **Links:** a link inside the site is relative, and links a folder (`./`) rather than its `README.md`, which mdBook builds as `index.html` but links as a `README.html` it never writes. A link to a design document or any other file outside `docs/guide/` is a full GitHub link to `main`, since the site doesn't publish its target. `scripts/check-site-links.ts` checks both offline, against the files git has: the page or file exists, a page is in `SUMMARY.md`, and an anchor is one of the target's headings, made the way mdBook makes them (GitHub makes the same for the headings the guides use). Links elsewhere aren't fetched.
- **Accessibility:** axe-core (WCAG 2.2 A and AA rules) is clean on every page in each of mdBook's five themes, at a wide and a narrow window, with the search results and the theme list open. `docs/guide/theme/` holds the fixes, each with its reason: link and code colors with too little contrast, links underlined in text, code that wraps rather than scrolls, 24px targets, a skip link, and the sidebar toggle made a keyboard-reachable button.

## Not decided here

- **The Pages workflow**, which builds the site on a pull request that touches it and publishes it from `main`: a CI change, in its own pull request, with job names the "Protect main" ruleset doesn't require (development.md §13.8).
- **Screenshots** regenerated from the `@ui` journeys (#1178), and how the site follows the UI's locales (§8.4); it is English only for now.

## Rejected

- **Moving `docs/guide/` into a new `docs/user/`:** it breaks the released install script's link and the guide's tests for no gain to readers.
- **Zola:** Rust too, but a docs layout, navigation and search come from a third-party theme the project would own.
- **MkDocs Material, Docusaurus, VitePress:** each adds a Python or Node toolchain to the contributor setup for the site alone, and the last two ship a JavaScript application to readers.
- **GitHub's own rendering of `docs/guide/`,** as until now: no navigation between pages, no search, and no landing page beyond a folder listing.
- **A link-check plugin for mdBook:** a second binary to pin and install, where a TypeScript check fits the existing tooling, its `node --test` tests and `just lint`.
