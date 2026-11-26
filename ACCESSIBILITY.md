# Accessibility

Ladderback's web UI is built to meet [WCAG 2.2 AA](https://www.w3.org/TR/WCAG22/). Accessibility is a release requirement, not a later polish pass: a change that breaks it doesn't merge.

## What it means here

The promise is in [PRODUCT.md](PRODUCT.md#accessibility--inclusion), and the six rules that hold it, A1 to A6, are in [the technical design](docs/design/README.md#accessibility-invariants), each with how it is enforced and verified. In short: every workflow works with the keyboard alone, every control has a name, status is never shown by color alone, live changes reach screen readers without flooding them, pages hold at 320 px wide with text at 200 %, and destructive actions can be reviewed and canceled.

The supported assistive technologies are Orca on Linux, VoiceOver on macOS, and keyboard-only use.

## How it is checked

A change to the web UI runs axe-core over the pages and the shared components, keyboard-only browser journeys in Chromium, Firefox and WebKit, and a contrast check of the design tokens; a change to the guide runs axe-core over every page of the documentation site. The shared components each have an accessibility contract in [§4.5 of the design](docs/design/components/accessibility.md). Automated checks find only part of what a person meets, so a report from someone using assistive technology is worth more than any of them.

## Reporting a barrier

Open an issue with the **Accessibility barrier** form: say what you were trying to do, what you use (a screen reader, keyboard only, zoom, a high-contrast or reduced-motion setting), and what stopped you. A barrier is treated as a bug, labeled `accessibility`. The documentation site counts too.

What isn't fixed yet is in the [open issues labeled `accessibility`](https://github.com/nledford/Ladderback/issues?q=is%3Aissue%20is%3Aopen%20label%3Aaccessibility).
