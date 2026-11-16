---
name: Ladderback
description: Self-hosted backup manager for rustic; its web UI is a ruled record of every machine's backups.
colors:
  paper: "#ffffff"
  paper-dark: "#111418"
  graphite-ink: "#1b1f24"
  graphite-ink-dark: "#e8eaed"
  slate-note: "#4b5563"
  slate-note-dark: "#a7b0bc"
  ledger-gray: "#f3f4f6"
  ledger-gray-dark: "#1d2229"
  ladder-blue: "#1d4ed8"
  ladder-blue-dark: "#8ab4f8"
  loading-fg: "#172554"
  loading-bg: "#dbeafe"
  empty-fg: "#111827"
  empty-bg: "#e5e7eb"
  empty-bg-dark: "#26303f"
  error-fg: "#691717"
  error-bg: "#fee2e2"
  success-fg: "#052e16"
  success-bg: "#dcfce7"
  tick-green: "#15803d"
  tick-green-dark: "#4ade80"
  flag-amber: "#b45309"
  flag-amber-dark: "#fbbf24"
  red-ink: "#b91c1c"
  red-ink-dark: "#f87171"
typography:
  body:
    fontFamily: "system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: 1.5
  mono:
    fontFamily: "ui-monospace, monospace"
    fontSize: "1rem"
rounded:
  sm: "0.25rem"
  pill: "9999px"
spacing:
  xs: "0.25rem"
  sm: "0.5rem"
  md: "0.75rem"
  lg: "1rem"
components:
  badge-loading:
    backgroundColor: "{colors.loading-bg}"
    textColor: "{colors.loading-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  badge-empty:
    backgroundColor: "{colors.empty-bg}"
    textColor: "{colors.empty-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  badge-error:
    backgroundColor: "{colors.error-bg}"
    textColor: "{colors.error-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  badge-success:
    backgroundColor: "{colors.success-bg}"
    textColor: "{colors.success-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  button:
    typography: "{typography.body}"
    height: "2rem"
    padding: "0 0.75rem"
  field-input:
    typography: "{typography.body}"
    height: "2rem"
    width: "32rem"
    padding: "0.25rem 0.5rem"
  site-header:
    backgroundColor: "{colors.ledger-gray}"
    padding: "0.75rem 1rem"
  toast:
    backgroundColor: "{colors.ledger-gray}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 0.75rem"
  progress-track:
    backgroundColor: "{colors.ledger-gray}"
    rounded: "{rounded.pill}"
    height: "0.5rem"
    width: "16rem"
  spinner:
    # The drawing's color (its arc); the label keeps the text's color.
    textColor: "{colors.ladder-blue}"
    size: "1em"
---

# Design System: Ladderback

## Overview

**Creative North Star: "The Ledger"**

Ladderback's web UI is a ledger of backups: ruled rows of facts, each with its unit, read top to bottom. A page is a heading, a sentence that says where things stand, and a table or a form. The person comes to check that every machine is backed up, notice the one that isn't, and act; nothing on the page competes with that.

The look is the browser's own, on purpose. Buttons, headings, checkboxes and dialogs are the native controls, sized to the text around them, so they behave the way the reader's browser, zoom, high-contrast mode and screen reader expect. Surfaces are flat. One blue carries links, focus and progress, three status hues tell what went well, what needs a look and what failed, always beside words, and four quiet status pairs say what a run is doing. Everything else is text.

Accessibility shapes the layout rather than decorating it: every layout holds at 320 px wide with text at 200 %, in both directions of writing, in light, dark and forced colors.

**Key Characteristics:**

- Native controls, flat surfaces, no shadows.
- One accent, Ladder Blue, for action: links, the focus ring and progress. Three status hues beside it, never alone.
- State is a labeled badge with an icon, never color alone.
- Tables with one muted rule per row and columns that don't move as rows change.
- Logical properties only; layouts follow the page's writing direction.

## Colors

A white page (dark: near-black), graphite text, one blue, three status hues and four soft status pairs. Every color is a CSS custom property in `crates/web/style.css`, redefined under `prefers-color-scheme: dark`; `tailwind.css` maps the same properties as Tailwind colors, so `style.css` stays the one source. `just test-contrast` checks every pair the UI relies on.

### Primary

- **Ladder Blue** (`--accent`, `--focus`): links, the 3 px focus ring, a progress bar's fill, a spinner's arc, the skip link's border and a toast's edge. It is also the field of the logo's mark (`assets/brand/`). In dark mode it lightens to Ladder Blue Dark so it keeps its contrast.

### Neutral

- **Paper** (`--bg`): the page.
- **Graphite Ink** (`--fg`): all body text and headings.
- **Slate Note** (`--muted`): secondary text (captions, notes, the running version) and the rule under each table row and around the pause banner.
- **Ledger Gray** (`--surface`): the site header, toasts, the pause banner and a progress bar's empty track.

### Status

The status badges: each state is a text token on a background token, swapped between schemes rather than recolored: Loading (blue), Empty (gray), Error (dark red on pale red), Success (dark green on pale green). The error text color also draws the form error and the error summary's border. A badge's text keeps its pair; the status hues below are not drawn on a badge's background.

The status hues: a color a part draws in to tell how something went, on the page or a surface, at 4.5:1 or more on both and lighter in dark mode. They are the Success, Warning and Error tones of the shared vocabulary (Components), and under forced colors each becomes the reader's text color.

- **Tick Green** (`--success`): something ended well.
- **Flag Amber** (`--warning`): something needs a look.
- **Red Ink** (`--error`): something failed.

### Named Rules

**The One Blue Rule.** Ladder Blue is the one accent for action. Beside it stand three status hues, Tick Green, Flag Amber and Red Ink, which never carry a meaning alone: a part drawn in one says it in words or an icon too. Any other new meaning gets words, weight or an icon, not a new hue.

**The Pair Rule.** A color is added as a text-and-background pair with a contrast check in `scripts/check-contrast.ts`, never as a lone value.

## Typography

**Body Font:** the system UI font (`system-ui`, with `sans-serif`)
**Mono Font:** `ui-monospace`, with `monospace`, only for text a person copies into a terminal, such as a new API token

**Character:** the reader's own interface font, at the reader's own size. Nothing is set in pixels, so text grows with the browser's setting and every control grows with it.

### Hierarchy

- **Headings** (`h1` to `h3`, bold, the browser's default sizes): one `h1` per page, `h2` per section. The error summary's `h2` is smaller, 1.125rem.
- **Body** (400, 1rem, line-height 1.5): sentences, table cells, notes.
- **Label** (600, 1rem): form labels, a group's legend, the brand, the pressed pause button and form errors.
- **Current and selected** (700): the current page in the navigation and the selected row of a snapshot's tree.

### Named Rules

**The Weight Not Color Rule.** Current, selected and pressed are said by weight (and an inset edge on a pressed button), never by color alone.

**The Reader's Size Rule.** Form controls inherit the page's font (`font: inherit`), so a button's label doubles when the reader doubles their text.

## Layout

One column, `main` centered at up to 60rem with 1rem padding. The site header spans the page: the brand on its own line, then the navigation, with the running version and, while signed in, Sign out at the end of the line. On a narrow screen they wrap under the navigation toward the line's end, together where they fit.

Spacing steps are 0.25, 0.5, 0.75 and 1rem; forms stack fields 1rem apart, and an `.actions` row puts its buttons 0.5rem apart, wrapping. Inputs fill their line up to 32rem (40rem for a token).

Tables that change while they are read (operations, repositories) have fixed column shares, so a run that starts and ends changes what a row says, not where the columns are. Above a container width (48rem for operations, 26rem for repositories) their widths switch to what each column holds, measured on the frame around the table. Padding inside these tables follows the screen's width (`min(0.5rem, 1.25vw)`) so they fit 320 px at 200 % text.

### Named Rules

**The 320 Rule.** Every page fits 320 px wide with 200 % text without scrolling sideways. Long words break (`overflow-wrap: anywhere`); a path, address or ID moves to the next line whole before it breaks inside.

**The Logical Rule.** Margins, padding, sizes and insets use logical properties (`inline-size`, `margin-block`, `inset-inline-start`), never left, right, width or height.

## Elevation & Depth

Flat. No element has a shadow. Depth is said with a Ledger Gray surface, a 1 px edge, or the browser's own dialog backdrop. A toast is "above its neighbors" by its surface and its Ladder Blue edge, not by lift.

### Named Rules

**The Flat Ledger Rule.** Surfaces never cast shadows. Separation is a fill, a rule or a heading.

## Shapes

Square by default, as the browser draws controls. Two radii exist: a full pill (`9999px`) for status badges and progress bars, and a small 0.25rem corner for toasts and the pause banner. Table rows are separated by one Slate Note rule under each row; there are no cell borders or zebra stripes.

## Components

Every component is plain and exact: a control looks like what the browser makes it, sized for the text it holds. The shared set lives in `crates/web/src/components.rs`; `/_gallery` (the `gallery` feature) shows each one.

### Shared Vocabulary

Every shared component takes the same three choices, defined once in `crates/web/src/variants.rs`, each a class of `style.css`, as it is built or reworked; the gallery's "Shared vocabulary" demo shows them all. The status badge keeps its states' own pairs for its text. A component's row in §4.5 names any tone it leaves out because it makes no sense there.

- **Tone:** Primary (Ladder Blue, the default), Secondary (Slate Note), Success, Warning and Error (the status hues), and Inherit, the color of the text around it, for a part inside another component, such as a spinner in a button. No Info: Primary covers it. A tone's class sets `--tone`, which the component draws with.
- **Size:** Small, Medium, Large and XLarge. Each component maps them to `em` lengths of its own, listed with it, so a size grows with the reader's text. There is no default size.
- **Placement:** Inline (the default: in the line of text, as wide as what it holds), then Start, Center and End, each on a line of its own at its start, middle or end, in the page's direction. The class goes on the line, an element of the component's own around its part, never on the part's own box, which it would stretch.

### Buttons

- **Shape and color:** the browser's native button. No custom fill, border or radius.
- **Size:** at least 2rem tall in action rows and in the header (Sign out), 0.75rem inline padding.
- **Disabled:** stays focusable (`aria-disabled="true"`), at 60 % opacity with a not-allowed cursor.
- **Pressed (toggle):** bold text and an inset border, with a banner that says it in words.
- **Focus:** the global 3 px Ladder Blue ring, 2 px offset; `CanvasText` under forced colors.

### Status Badges

- **Style:** a pill in its state's pair (see Colors), 0.125rem by 0.5rem padding.
- **Content:** an icon as tall as the text, stroked in the text's color, then the label. Where the two don't fit side by side, the label wraps whole below the icon.

### Inputs / Fields

- **Style:** native inputs, full line width up to 32rem, at least 2rem tall, label above in Label weight.
- **Checkbox:** before its label, never under 24 px square, growing with the text; its description sits under the label.
- **Error:** the message beside its field in error red and semibold; on submit, an error summary boxed in a 2 px error-red border, with its own heading, takes focus.

### Navigation

- **Style:** a wrapping row of plain links in the Ladder Blue link color on the Ledger Gray header, 1rem apart.
- **Current page:** bold, with `aria-current="page"`.
- **Brand:** the product's mark before its name, as tall as the name's letters, hidden from screen readers; under forced colors it takes `CanvasText` and `Canvas`.
- **Sign out:** a native button after the running version, outside the navigation's list, since it isn't a page. A failed sign-out is said once at the top of `main`, never in the header, so the header keeps its height.
- A skip link is the first focusable element and appears at the top of the page on focus.

### Tables

- **Style:** full width, collapsed borders, cells aligned to the start and top, one Slate Note rule under each row, captions in Slate Note.
- **Sorting:** a sortable header puts its sort button after its text, wrapping below it when there's no room.

### Progress Bar

A Ledger Gray pill track 0.5rem tall, filling the room it is given up to 16rem, with a Ladder Blue fill that eases to each report (0.4 s) and jumps under reduced motion. The current step sits on its own line above it.

### Spinner

- **Use:** a wait for something the page can't show yet. Where a spinner would start by itself beside content the reader can use and might turn past 5 s, the page shows the progress bar or words instead, since a spinner has no way to stop it (2.2.2); one the reader started, one over within 5 s, or one that is all the page shows while nothing can be used yet is fine. The technical design (§4.5) has the rule in full.
- **Shape:** a faint full-circle track and a quarter arc with round ends, the arc turning steadily once in 0.9 s; under reduced motion it stands still, and its label carries the meaning.
- **Size:** the shared sizes, in `em`, never pixels: Small (1em, as tall as the text, the default, inside a button or a line of text), Medium (1.5em), Large (2em) and XLarge (3em).
- **Tone:** any of the shared tones, Primary (Ladder Blue) by default, Inherit (the text's color) inside a button or a toast. The tone colors the drawing only; the label keeps the text's color, so a Success, Warning or Error spinner still says in words what it waits for. The track is the arc's color at a quarter of its strength. Under forced colors the arc is the text's own color, as every tone is, and the track `GrayText`.
- **Label:** always one, saying what the page waits for. Beside the spinner by default, where the drawing is hidden from a screen reader; where there's no room, the drawing is an image named by it.
- **Placement:** the shared placements, on the spinner's line: Inline (the default), in its line of text, the label on the line's baseline and the drawing centered on the text; Start, Center or End, on a line of its own, which follow the page's direction, a long label wrapping beside the drawing, never under it, the drawing at its first line.
- **The page's duties:** the spinner is no live region, so a wait the reader caused, and its end, are said through the announcer (4.1.3); the region it stands for is `aria-busy` until it is filled; a busy button takes `aria-disabled`, not `disabled`, so it keeps focus, and ignores presses until the work ends.

### Dialogs

The native `<dialog>`, as wide as its content up to 48rem. Review dialogs put Cancel first; a line that reports progress takes no room while it is empty.

### Snapshot Tree

A flat list of rows indented by level (capped so deep folders fit 320 px), each row at least 1.5rem tall; the selected row is bold.

## Do's and Don'ts

### Do:

- **Do** use the shared components in `crates/web/src/components.rs` and add new ones to `/_gallery`, each taking its color, size and placement from the shared vocabulary.
- **Do** take colors only from the custom properties in `style.css`, through `var()` or their Tailwind names (`text-muted`, `bg-surface`).
- **Do** give every state words and an icon or weight as well as its color.
- **Do** keep a changing table's columns still: fixed shares, or widths that don't depend on a cell's changing content.
- **Do** check a new layout at 320 px with 200 % text, right to left, in dark mode and under forced colors.

### Don't:

- **Don't** add dashboard chrome: cards, gradients, glows, shadows or big hero numbers.
- **Don't** repeat a failure in red across the page. It is stated once, as an error badge or one error message; no red banners, red rows or red headings.
- **Don't** restyle native controls with custom fills, radii or borders.
- **Don't** introduce a second accent color, a fourth status hue, or a color without a contrast-checked pair.
- **Don't** set text or control sizes in pixels, or use physical properties (`left`, `width`, `margin-top`).
- **Don't** animate anything that the reduced-motion rule can't turn off.
