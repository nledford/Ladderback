# Decision 117: Tailwind first, custom CSS only when necessary

**The web UI styles with Tailwind utilities in the markup of its components and pages, and writes custom CSS in `crates/web/style.css` only for what utilities can't reasonably express** (recorded 2026-11-17, §4.5; decided by the maintainer, migration in #931).

## The rule

- **Utilities first.** A component or page sets its layout, spacing, type, color and state with Tailwind utilities and variants (`hover:`, `focus-visible:`, `dark:`, `forced-colors:`, container queries). Colors come from the tokens through their Tailwind names (`text-muted`, `bg-surface`), never a palette color or an arbitrary hex value.
- **Logical utilities only.** Use the utilities that follow the page's direction (`ms-`, `me-`, `ps-`, `pe-`, `inset-s-`, `inset-e-`, `border-s`, `rounded-s`, `text-start`), never the `left` and `right` ones, so a right-to-left page mirrors (§8.4).
- **Custom CSS is the exception.** `style.css` keeps the design tokens and their dark values, the base rules (no preflight, so `font: inherit` on form controls and similar), and the selectors or at-rules that no utility or variant covers. Each such rule carries a comment that says why a utility won't do.
- **The vocabulary moves with it.** A tone, size or placement of decision 108 becomes a set of utilities that `variants.rs` returns, as each component moves; until then its class stays in `style.css`.

## Why

- **One way to style.** Pages mixed `flex gap-4` with one-off classes such as `actions` (30 uses), with nothing saying which to choose. A single default ends the choice.
- **The styling sits where the markup is.** A component's look reads in its own file, not in a 1,600-line stylesheet that every component and page edits, which also made parallel component pull requests collide in one file.
- **The tokens stay the one source.** `tailwind.css` already maps every token as a Tailwind color (`@theme inline`), so utilities change nothing in the contrast gate or the dark scheme.

## Cost

- **Churn.** The existing custom CSS moves in #931, one area per pull request, each with before and after screenshots and its `@ui` scenarios in all three engines.
- **Long class lists.** A component with many states carries a long string. It lives in the component, or in `variants.rs` for a vocabulary value, never repeated across pages.
