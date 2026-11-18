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
  success-fg: "#052e16"
  success-fg-dark: "#dcfce7"
  success-bg: "#dcfce7"
  success-bg-dark: "#052e16"
  warning-fg: "#451a03"
  warning-fg-dark: "#fef3c7"
  warning-bg: "#fffbeb"
  warning-bg-dark: "#451a03"
  error-fg: "#691717"
  error-fg-dark: "#fee2e2"
  error-bg: "#fecaca"
  error-bg-dark: "#7f1d1d"
  tick-green: "#15803d"
  tick-green-dark: "#4ade80"
  flag-amber: "#b45309"
  flag-amber-dark: "#fbbf24"
  red-ink: "#b91c1c"
  red-ink-dark: "#f87171"
  field-edge: "#6b7280"
  field-edge-dark: "#7b8594"
  pencil-gray: "#c8ccd2"
  pencil-gray-dark: "#353d4a"
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
  badge-primary:
    backgroundColor: "{colors.loading-bg}"
    textColor: "{colors.loading-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  badge-secondary:
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
  badge-warning:
    backgroundColor: "{colors.warning-bg}"
    textColor: "{colors.warning-fg}"
    rounded: "{rounded.pill}"
    padding: "0.125rem 0.5rem"
  button-filled:
    # A row's one main action, in Primary; its label is the page's own
    # background color, `--on-fill`, Paper Dark on the dark scheme's fill.
    backgroundColor: "{colors.ladder-blue}"
    textColor: "{colors.paper}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    height: "2em"
    padding: "0 0.75em"
  button-outlined:
    # The default, in Secondary: an edge and a label in its tone.
    textColor: "{colors.slate-note}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    height: "2em"
    padding: "0 0.75em"
  button-ghost:
    # The label alone, for a row with many actions.
    textColor: "{colors.slate-note}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    height: "2em"
    padding: "0 0.75em"
  field-input:
    # A text field at the default size, a Medium button's height; its edge
    # is Field Edge, 1 px.
    backgroundColor: "{colors.paper}"
    textColor: "{colors.graphite-ink}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    height: "2em"
    width: "32rem"
    padding: "0 0.75em"
  site-header:
    backgroundColor: "{colors.ledger-gray}"
    padding: "0.75rem 1rem"
  toast:
    # Information's; a status tone's lies on its status pair. Edged 1 px in
    # its tone.
    backgroundColor: "{colors.ledger-gray}"
    rounded: "{rounded.sm}"
    padding: "0.5rem 0.75rem"
  progress-track:
    backgroundColor: "{colors.ledger-gray}"
    rounded: "{rounded.pill}"
    # Small, the default: half the shared scale's 1em.
    height: "0.5em"
    width: "16rem"
  spinner:
    # The drawing's color (its arc); the label keeps the text's color.
    textColor: "{colors.ladder-blue}"
    size: "1em"
  skeleton-bar:
    # A line of text 1em tall, or a block in rem; never moves.
    backgroundColor: "{colors.pencil-gray}"
    rounded: "{rounded.sm}"
---

# Design System: Ladderback

## Overview

**Creative North Star: "The Ledger"**

Ladderback's web UI is a ledger of backups: ruled rows of facts, each with its unit, read top to bottom. A page is a heading, a sentence that says where things stand, and a table or a form. The person comes to check that every machine is backed up, notice the one that isn't, and act; nothing on the page competes with that.

The look is plain, on purpose. Headings, checkboxes and dialogs are the browser's native controls, and a button is a native `<button>` drawn from the tokens, so it looks the same in every browser while it behaves the way the reader's browser, zoom, high-contrast mode and screen reader expect. All are sized to the text around them. Surfaces are flat. One blue carries links, focus, progress and a row's main action, three status hues tell what went well, what needs a look and what failed, always beside words, and quiet pairs, dark words on a pale fill (pale on deep in dark mode), say what a run is doing. Everything else is text.

Accessibility shapes the layout rather than decorating it: every layout holds at 320 px wide with text at 200 %, in both directions of writing, in light, dark and forced colors.

**Key Characteristics:**

- Native controls underneath, buttons drawn from the tokens, flat surfaces, no shadows.
- One accent, Ladder Blue, for action: links, the focus ring, progress and a row's one Filled button. Three status hues beside it, never alone.
- State is a labeled badge with an icon, never color alone.
- Tables with one muted rule per row and columns that don't move as rows change.
- Logical properties only; layouts follow the page's writing direction.

## Colors

A white page (dark: near-black), graphite text, one blue, three status hues, a soft pair for each of them and the badge's two own soft pairs. Every color is a CSS custom property in `crates/web/style.css`, redefined under `prefers-color-scheme: dark`; `tailwind.css` maps the same properties as Tailwind colors, so `style.css` stays the one source. `just test-contrast` checks every pair the UI relies on.

### Primary

- **Ladder Blue** (`--accent`, `--focus`): links, the 3 px focus ring, a progress bar's fill, a spinner's arc, the skip link's border, a toast's edge, an information alert's and an information banner's icon and edge (the Primary tone, which covers Info) and the selected tab's bar. It is also the field of the logo's mark (`assets/brand/`). In dark mode it lightens to Ladder Blue Dark so it keeps its contrast.

### Neutral

- **Paper** (`--bg`): the page.
- **Graphite Ink** (`--fg`): all body text and headings.
- **Slate Note** (`--muted`): secondary text (captions, notes, the running version), the rule under each table row and between a list of tabs and its panel, and the line around the pause banner.
- **Ledger Gray** (`--surface`): the site header, toasts, alerts, banners, the pause banner, a progress bar's empty track and an unavailable text field.
- **Field Edge** (`--field-edge`): a text field's 1 px edge, 3:1 or more on the page and on Ledger Gray; Field Edge Dark in dark mode.
- **Pencil Gray** (`--skeleton`): a skeleton's bars, 1.61:1 on the page and 1.47:1 on Ledger Gray (1.69:1 and 1.46:1 as Pencil Gray Dark), so they show on either. That is a floor for visibility, 1.4:1 in the contrast gate, not a WCAG minimum: a skeleton carries no information.

### Status

The status hues: a color a part draws in to tell how something went, on the page or a surface, at 4.5:1 or more on both and lighter in dark mode. They are the Success, Warning and Error tones of the shared vocabulary (Components), and under forced colors each becomes the reader's text color.

- **Tick Green** (`--success`): something ended well.
- **Flag Amber** (`--warning`): something needs a look.
- **Red Ink** (`--error`): something failed.

The status pairs: each status tone's soft pair, for a part tinted in it (a badge's pill, an alert's or a toast's box), its words on its fill, in Tailwind's palette of the same hue. Every shared component that tints a part by status draws from these, never from a pair of its own.

| Pair | Light: words on fill | Dark: words on fill |
|---|---|---|
| Success (`--success-fg`, `--success-bg`) | green-950 `#052e16` on green-100 `#dcfce7` | green-100 on green-950 |
| Warning (`--warning-fg`, `--warning-bg`) | amber-950 `#451a03` on amber-50 `#fffbeb` | amber-100 `#fef3c7` on amber-950 |
| Error (`--error-fg`, `--error-bg`) | `#691717` on red-200 `#fecaca` | red-100 `#fee2e2` on red-900 `#7f1d1d` |

The words are at 4.5:1 or more on their fill. Warning's and Error's fills stand at least 0.1 apart in OKLab lightness in both schemes, Error's the further from the page (darker in light, lighter in dark), so amber and red never read as one, for a reader who can't tell the hues apart either (decision 108). The Error pair's words also draw a form's errors and the error summary's border on the page. Under forced colors the system's colors replace both.

The badge's own pairs, for the tones that tell no result: Loading (dark blue on pale blue), the badge's Primary tone, which a status badge takes while a run is under way, and Empty (gray), its Secondary tone, which a status badge takes while a run waits, once it was cancelled, or with nothing to tell. A badge in the Success, Warning or Error tone, and a status badge for a run that succeeded or failed, draws in its status tone's pair; the status hues above are not drawn on a fill.

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
- **Current and selected** (700): the current page in the navigation, the selected row of a snapshot's tree and the selected tab.

### Named Rules

**The Weight Not Color Rule.** Current, selected and pressed are said by weight (and an inset edge on a pressed button, a bar beside a selected tab), never by color alone.

**The Reader's Size Rule.** Form controls inherit the page's font (`font: inherit`), so a button's label doubles when the reader doubles their text.

## Layout

One column, `main` with 1rem padding, as wide as Settings' Page width chooses in this browser: Narrow, the default, centers it at up to 60rem; Full width lets it use the window's width, while each paragraph and form in it stays at up to 70 characters (`70ch`), a readable line however wide the screen. The width is Tailwind utilities on `main` (`page_width.rs`), never a `style.css` rule. The site header spans the page: the brand on its own line, then the navigation, with the running version and, while signed in, Sign out at the end of the line. On a narrow screen they wrap under the navigation toward the line's end, together where they fit.

Spacing steps are 0.25, 0.5, 0.75 and 1rem; forms stack fields 1rem apart, and an `.actions` row puts its buttons 0.5rem apart, wrapping. Inputs fill their line up to 32rem (40rem for a token).

Tables that change while they are read (operations, repositories) have fixed column shares, so a run that starts and ends changes what a row says, not where the columns are. Above a container width (48rem for operations, 26rem for repositories) their widths switch to what each column holds, measured on the frame around the table. Padding inside these tables follows the screen's width (`min(0.5rem, 1.25vw)`) so they fit 320 px at 200 % text.

### Named Rules

**The 320 Rule.** Every page fits 320 px wide with 200 % text without scrolling sideways. Long words break (`overflow-wrap: anywhere`); a path, address or ID moves to the next line whole before it breaks inside. A data table wider than the page scrolls inside its own frame, never the page.

**The Logical Rule.** Margins, padding, sizes and insets use logical properties (`inline-size`, `margin-block`, `inset-inline-start`), never left, right, width or height.

## Elevation & Depth

Flat. No element has a shadow. Depth is said with a Ledger Gray surface, a 1 px edge, or the browser's own dialog backdrop. A toast is "above its neighbors" by its surface and its Ladder Blue edge, not by lift.

### Named Rules

**The Flat Ledger Rule.** Surfaces never cast shadows. Separation is a fill, a rule or a heading.

## Shapes

Square by default, as the browser draws controls. Two radii exist: a full pill (`9999px`) for status badges and progress bars, and a small 0.25rem corner (`--radius-small`) for buttons, toasts, alerts, the pause banner, tooltips and a skeleton's bars. Table rows are separated by one Slate Note rule under each row; there are no cell borders or zebra stripes. Each cell keeps its own rule (borders separate, with no spacing), so a pinned row header carries it as the table scrolls.

## Components

Every component is plain and exact: a control is the browser's own element, sized for the text it holds, and a button is drawn from the tokens. The shared set lives in `crates/web/src/components/`, one file per component; `/_gallery` (the `gallery` feature) shows each one.

### Shared Vocabulary

Every shared component takes the same choices, defined once in `crates/web/src/variants.rs`, as it is built or reworked. A tone, a size and a placement are each a set of Tailwind utilities that `variants.rs` returns, or, until its component moves (#931), a class of `style.css`; the gallery's "Shared vocabulary" demo shows them. A side is where the page places a part, which the "Tooltip" demo shows. A badge keeps the five status pairs for its text and its pill, each under a tone's name. A component's row in §4.5 names any tone it leaves out because it makes no sense there.

- **Tone:** Primary (Ladder Blue, the default, unless a component names its own, as the icon and the button do), Secondary (Slate Note), Success, Warning and Error (the status hues), and Inherit, the color of the text around it, for a part inside another component, such as a spinner in a button. No Info: Primary covers it. A tone's class sets `--tone`, which the component draws with.
- **Size:** Small, Medium, Large and XLarge, one scale every component shares, in `em`, so a size grows with the reader's text: Small 1em, Medium 1.5em, Large 2em, XLarge 3em. A size's class sets `--size`, which the component draws its length with, so a spinner and an icon of one size line up. A component that needs other lengths may deviate, saying why with it. The vocabulary has no default size: each component picks its own.
- **Placement:** Inline (the default: in the line of text, as wide as what it holds), then Start, Center and End, each on a line of its own at its start, middle or end, in the page's direction. The class goes on the line, an element of the component's own around its part, never on the part's own box, which it would stretch.
- **Side:** for a part that floats beside a control (a tooltip, later a popover): Above (the default), Below, Start and End, the last two in the page's direction. Without room on its side the part flips to the opposite one, then to the other axis. Not a placement, which aligns a part in its own line.

### Buttons

- **The component:** `Button`, a native `<button>` drawn from the tokens (decision 113). Every action a page offers is one (#780); none is the browser's own button.
- **Appearance:** Filled, a fill in its tone with its label in the page's background color, for an action row's one main action, in Primary; Outlined, an edge and a label in its tone, the default, for the others; Ghost, the label alone, for a row with many, a table header's sort control and the header's Sign out. Secondary (Slate Note) unless the page names another tone; a Filled button has no Inherit.
- **Tone:** Error for an action that deletes or takes away (Delete, Revoke, Forget, Prune, Sign out everywhere, Restart now), Filled only as a dialog's confirm; Primary for a main action and a dialog's confirm that keeps things.
- **Icon:** only where it says something the label doesn't: Plus to add, Pencil to change, Trash to delete or forget, Play to start, RotateCcw to restore, Pause and Play on the pause button, LogOut to sign out, an arrow for a sort's direction.
- **Shape:** one small corner (`rounded.sm`), flat: no shadow, no gradient, no pill.
- **Size:** its height, in `em`: Small 1.75em, Medium 2em (the default, 2rem at the page's text size), Large 2.5em, XLarge 3em, the inline padding growing with it from 0.5em to 1.25em. An icon or spinner inside is as tall as the text; an icon-only button is square. A table's row actions are Small, the whole column alike.
- **Hover:** a step darker, lighter in the dark scheme, each pair contrast-checked.
- **Unavailable and busy:** stay focusable (`aria-disabled="true"`), at 60 % opacity with a not-allowed cursor. A busy button's spinner takes its icon's place, or stands before words that have none; a button that must not move gets an icon. A button is busy only while the request its press sent runs.
- **Pressed (toggle):** bold text and an inset edge; the pause button adds a banner that says it in words.
- **Forced colors:** the reader's `ButtonFace`, its label and edge `ButtonText`, a pressed Ghost one's edge too, an unavailable one `GrayText`; a busy one keeps `ButtonText`, as it is at work, not unavailable.
- **Focus:** the global 3 px Ladder Blue ring, 2 px offset; `CanvasText` under forced colors.

### Badges

- **Use:** a count or a tag beside what it describes ("Pending changes 3"), its words saying what it counts or names, the count included. Never a number or a color alone.
- **Style:** a pill in its tone's pair (see Colors), 0.125rem by 0.5rem padding: Secondary (the badge's own gray pair, the default), Primary (its own blue pair), or Success, Warning or Error (their status pairs). Under forced colors, a 1px edge in the text's color, inside the pill.
- **Content:** an optional icon as tall as the text, stroked in the text's color, then the words. Where the two don't fit side by side, the words wrap whole below the icon.
- **Plain text:** no role, nothing to focus; it takes the text's size and stands in its line.

### Status Badges

- **Style:** a badge in its state's tone: Success's pair or Error's for a run that succeeded or failed, the badge's own Loading (blue) pair while it runs and its Empty (gray) pair otherwise.
- **Content:** the state's own icon, then the label.

### Inputs / Fields

- **The component:** `TextField`, a native `<input>`, or `<textarea>` for many lines, drawn from the tokens (decision 113); every page's field uses it.
- **Style:** a 1 px Field Edge, the small corner (`rounded.sm`), flat on the page's background, no shadow; full line width up to 32rem (40rem in Mono for a token); label above in Label weight, its description under it.
- **Size:** a button's height (Small 1.75em, Medium 2em, the default, Large 2.5em, XLarge 3em) and inline padding, read from the one size rule both share, so a field and a button of one size stand level in a row of actions.
- **Required:** "(required)" after the label, in words.
- **Read only and unavailable:** told in words by a note under the description; read only has a dashed edge and stays focusable, unavailable takes Ledger Gray with Slate Note text.
- **Icon:** optional, decorative, inside the edge at the inline start.
- **Forced colors:** the reader's `Field` and `FieldText`, an unavailable field `GrayText`; invalid keeps its 2 px edge and read only its dashes.
- **Checkbox:** before its label, never under 24 px square, growing with the text; its description sits under the label.
- **Error:** the message above its field in error red and semibold, the field's edge 2 px in Red Ink; on submit, an error summary boxed in a 2 px error-red border, with its own heading, takes focus.

### Links

- **The component:** `Link`, a native `<a>` to a page of the app or to another site; pages move onto it in #886.
- **Style:** Ladder Blue (`--accent`), always underlined, the underline a token's thickness, drawn below the descenders, thicker under the pointer. No hover-only underline, no visited color, no link drawn as a button.
- **Tone:** Primary, the default, or Inherit, the color of the text around it, for a link inside a status sentence; still underlined. No status tones and no size: a link is the text around it.
- **Current page:** bold, with `aria-current="page"`, from where the reader is.
- **Another site:** opens where the reader is, never a new tab of its own; "(goes to another site)" after its words for a screen reader, and the ExternalLink icon after its words.
- **Forced colors:** `LinkText`, the underline kept.
- **Focus:** the global 3 px ring.

### Navigation

- **Style:** a wrapping row of plain links in the Ladder Blue link color on the Ledger Gray header, 1rem apart; underlined once they move onto `Link` (#886).
- **Current page:** bold, with `aria-current="page"`.
- **Brand:** the product's mark before its name, as tall as the name's letters, hidden from screen readers; under forced colors it takes `CanvasText` and `Canvas`.
- **Sign out:** a Ghost button with the LogOut icon after the running version, outside the navigation's list, since it isn't a page, its words on the links' baseline and no taller than the links; busy while the daemon signs out. A failed sign-out is said once at the top of `main`, never in the header, so the header keeps its height.
- A skip link is the first focusable element and appears at the top of the page on focus.

### Tables

- **Style:** the shared table: full width, cells aligned to the start and top, one Slate Note rule under each row, captions in Slate Note, header text bold. No zebra stripes, no hover highlight, no header band.
- **Columns:** each a fixed width in rem; the table is as wide as its frame or its columns added up, whichever is wider, so a column never moves as a cell changes. Numbers stand at the end in figures of one width.
- **Sorting:** a sortable header puts a Ghost Small sort button after its text, its arrow showing the direction, wrapping below the text when there's no room.
- **Narrow:** the table's frame takes keyboard focus at every width, and where its columns are wider than the page it scrolls sideways; each row's header stays pinned at the frame's start on the page's background, at most two fifths of the frame.

### Progress Bar

A Ledger Gray pill track, half its size on the shared scale thick (Small, the default, 0.5em; up to 1.5em at Extra large), filling the room it is given up to 16rem, with a fill in its tone (Ladder Blue, Primary, by default) that eases to each report (0.4 s) and jumps under reduced motion. The tone colors the fill only; the track stays Ledger Gray and the words keep the text's color. The fill is the tone's own color, never a status pair: a pair is for words on a tinted fill, and the fill carries none, so each tone's fill keeps 3:1 against the track and the page instead. The current step sits on its own line above it.

### Banners

- **The component:** `Banner`, a page-wide notice above a page's content: an icon, then its words. Pages move onto it in #930.
- **Look:** a square strip of Ledger Gray across the content, the words in Graphite Ink, the tone in the icon and in a 0.25rem edge at the start of the line; never a colored fill, so a warning is not a colored banner.
- **Tone:** Primary (information, the default) and Warning, each with its own icon, Info and TriangleAlert, named by its tone in words for a screen reader. No Success (a toast says it once), no Error (no red banners), no Secondary and no Inherit.
- **Dismiss:** optional, a Ghost Small X button at the line's end, its name shown by a tooltip; the page names where focus goes, and the browser remembers the dismissal of that edition, so a newer one shows again.
- **Not a live region and not a landmark:** what is urgent goes through the announcer too.
- **Forced colors:** a 1px `CanvasText` edge all round, the start edge still thick; the icon in the text's color.
- **No size and no placement:** its words are the page's text, and it spans the content.

### Spinner

- **Use:** a wait for something the page can't show yet. Where a spinner would start by itself beside content the reader can use and might turn past 5 s, the page shows the progress bar or words instead, since a spinner has no way to stop it (2.2.2); one the reader started, one over within 5 s, or one that is all the page shows while nothing can be used yet is fine. The technical design (§4.5) has the rule in full.
- **Shape:** a faint full-circle track and a quarter arc with round ends, the arc turning steadily once in 0.9 s; under reduced motion it stands still, and its label carries the meaning.
- **Size:** the shared sizes, in `em`, never pixels: Small (1em, as tall as the text, the default, inside a button or a line of text), Medium (1.5em), Large (2em) and XLarge (3em).
- **Tone:** any of the shared tones, Primary (Ladder Blue) by default, Inherit (the text's color) inside a button or a toast. The tone colors the drawing only; the label keeps the text's color, so a Success, Warning or Error spinner still says in words what it waits for. The track is the arc's color at a quarter of its strength. Under forced colors the arc is the text's own color, as every tone is, and the track `GrayText`.
- **Label:** always one, saying what the page waits for. Beside the spinner by default, where the drawing is hidden from a screen reader; where there's no room, the drawing is an image named by it.
- **Placement:** the shared placements, on the spinner's line: Inline (the default), in its line of text, the label on the line's baseline and the drawing centered on the text; Start, Center or End, on a line of its own, which follow the page's direction, a long label wrapping beside the drawing, never under it, the drawing at its first line.
- **The page's duties:** the spinner is no live region, so a wait the reader caused, and its end, are said through the announcer (4.1.3); the region it stands for is `aria-busy` until it is filled; a busy button takes `aria-disabled`, not `disabled`, so it keeps focus, and ignores presses until the work ends.

### Skeleton

- **Use:** the still shape of content on its way, standing where the content will, beside the words that say the wait ("Loading…"). Never a stand-in for those words, and hidden from a screen reader; the region it stands for is busy until the content comes.
- **Shape:** lines of text, each 1em tall on a line of the text's height, the full width of their room but the last, which is shorter; or a block in `rem`, such as a field, never wider than its room. No circles, avatars or image shapes.
- **Look:** flat Pencil Gray with the small corner, on the page and on Ledger Gray alike. Under forced colors, no fill and a 1px `GrayText` edge.
- **Still:** no shimmer, no pulse, in any motion setting: it starts by itself and nothing could pause it (2.2.2).
- **No tone and no size:** it is not a status, and it takes the content's size from the reader's text.

### Alerts

- **The component:** `Alert`, an inline message about a page's state: an icon, then its words. The Log, Plans and Snapshots pages, and a snapshot's Restore and Download dialogs, show their refusals with it, as Error alerts; the other pages and the tree's own message move onto it in #929.
- **Look:** Ledger Gray with the small corner, the words and any link in them in Graphite Ink, the tone in the icon and in a 0.25rem edge at the start of the line; never a colored fill, so a failure is not a red banner.
- **Tone:** Primary (information, the default), Success, Warning and Error, each with its own icon: Info, CircleCheck, TriangleAlert and CircleX, the icon named by its tone in words for a screen reader. No Secondary and no Inherit: an alert always says its tone.
- **Dismiss:** optional, a Ghost Small X button at the line's end, its name shown by a tooltip; the page takes the alert away and names where focus goes.
- **Not a live region:** urgent text goes through the announcer too; one failure is said once.
- **Forced colors:** a 1px `CanvasText` edge all round, the start edge still thick; the icon in the text's color.
- **No size:** its words are the page's text.

### Toasts

- **The component:** `Toast`, the answer to what the reader just did, in the page's flow where its page puts it, never fixed over the page: an icon, the words, then a Dismiss button. One at a time; a new one takes the place of the one shown.
- **Look:** a status tone's toast lies on its status pair, its words, icon and Dismiss button in the pair's words; information on Ledger Gray in Graphite Ink. Edged 1px all round in its tone, with the small corner. Flat: its fill and edge set it off, never a lift.
- **Tone:** Primary (information, the default), Success, Warning and Error, with the alert's icons, each named by its tone in words for a screen reader. No Secondary and no Inherit.
- **Time:** information and success go by themselves after 6 s, held while the pointer rests on it or focus is in it; a warning or a failure stays until dismissed. A failure also stays on its page.
- **Said, not seized:** the announcer says it, assertively only for a failure; it never takes focus. Dismiss hands focus to the control its page names.
- **Forced colors:** no fill, a 1px `CanvasText` edge; the icon in the text's color.

### Icon

- **Use:** one Lucide glyph beside words that say what it shows, or inside a control its own label names. Decoration by default, hidden from a screen reader; given a label, an image named by it, for an icon that says something no words beside it say.
- **Shape:** Lucide's outline, round ends and joins, 2 units of stroke on its 24-unit grid, never filled. Never mirrored right to left: a glyph that points along the reading direction is the page's to pick.
- **Size:** the shared sizes, Small (1em, as tall as the text, the default) up to XLarge (3em), centered on the text in a line of it.
- **Tone:** any of the shared tones, Inherit (the text's color) by default, so it reads as part of its words and their contrast covers it. A tone never says a meaning alone: the glyph's shape or the words do too. Under forced colors it is the text's own color, as every tone is.
- **Icon-only button:** named on the button, the icon inside it decoration. A toggle keeps its one name and says pressed in its glyph's shape (the gallery's bell, crossed out while off), since it has no words to turn bold.

### Tooltips

- **Use:** a short note beside one control, a sentence or two that adds to its name: what pressing it does, or an icon-only control's name. Never the only place a control's name lives, never something the reader needs to finish the task (a tooltip is found only by focusing or pointing at its control), and never a link or a button inside (that is a popover's).
- **Look:** the page's colors swapped: the fill is Graphite Ink (`--fg`), the words Paper (`--bg`), in light and dark. The small corner (`--radius-small`), flat, no shadow, no arrow. Under forced colors it is the system's page colors, edged in a 1px line of its text color, so it stands apart from what it lies over.
- **Size:** its words a step smaller than its control's (0.875em), so it grows with the reader's text; it wraps at about 20 of its em, or the window's width less a margin, no word cut. Its height has no limit, so its text stays short enough to fit a small window at 200 % text.
- **Side:** the shared sides, Above (the default), Below, Start and End, which follow the page's direction. Without room on its side it flips to the opposite one, then to the other axis, and stays inside the window, never over its control.
- **Showing:** keyboard focus shows it at once; a resting pointer after 300 ms, and it stays while the pointer is on the control or on it. Escape hides it, focus and pointer staying put. It fades in and out over 100 ms, at once under reduced motion. A mouse press's focus on a button doesn't show it (the resting pointer already did) and a finger shows nothing there: no tap opens one. A text field's note shows on a press, as any focus on a field is the keyboard's kind.
- **No tone and no size:** it is a note, not a status, and its size follows its control's text.

### Tabs

- **Use:** switching between views within one page. Never navigation between pages: that is the navigation's links.
- **Orientation:** horizontal (the default), a row of tabs above the panel; vertical, a column at the panel's start, for a list too long or its words too wide for a row; responsive, a column while the tabs' own space has room for the list beside the panel and a row below that, for tabs that sit in a wide page and a narrow column alike. A change of layout moves nothing else: the selected tab, the focus and what a panel holds stay.
- **Look:** the tabs on one Slate Note rule between them and the panel, under a row and at the end of a column, each tab its words in the text's color, no fill and no box. The selected tab is bold with a 3 px Ladder Blue bar on the rule beside it; a hovered one shows a Slate Note line as thick as the bar in the bar's place. Flat: no boxed, lifted or pill tabs, which say a depth this system doesn't use, and no icons.
- **Size:** the text's size, no shared size; each tab at least 24 × 24 CSS px. A bold copy of the words, unseen, keeps each tab as wide selected as not, so the row stays still as the selection moves. A column takes at most half the width, its words wrapping.
- **Narrow screens:** the row wraps onto more lines, every tab whole. Each tab stands on its own stretch of the rule, edge to edge with the next, so a tab selected on an earlier line keeps its bar over a rule; the rule runs on to the row's end under the last line. Responsive tabs are a row there, and with larger text.
- **No tone:** the bar is the accent, and tabs aren't a status.
- **Forced colors:** the rule and the selected bar in the text's own color, as the focus ring is, the bar told by its thickness and the weight kept, in either layout.
- **Focus:** the global 3 px ring, on a tab and on a panel that is a Tab stop of its own.

### Dialogs

The native `<dialog>`, as wide as its content up to 48rem. Review dialogs put Cancel first; a line that reports progress takes no room while it is empty.

### Snapshot Tree

A flat list of rows indented by level (capped so deep folders fit 320 px), each row at least 1.5rem tall; the selected row is bold.

## Do's and Don'ts

### Do:

- **Do** use the shared components in `crates/web/src/components/` and add new ones to `/_gallery`, each taking its color, size and placement from the shared vocabulary.
- **Do** style with Tailwind utilities first, and write custom CSS in `style.css` only for what they can't reasonably express, with a comment saying why (decision 117).
- **Do** take colors only from the custom properties in `style.css`, through their Tailwind names (`text-muted`, `bg-surface`), or `var()` in the custom CSS that remains.
- **Do** give every state words and an icon or weight as well as its color.
- **Do** keep a changing table's columns still: fixed shares, or widths that don't depend on a cell's changing content.
- **Do** check a new layout at 320 px with 200 % text, right to left, in dark mode and under forced colors.

### Don't:

- **Don't** add dashboard chrome: cards, gradients, glows, shadows or big hero numbers.
- **Don't** repeat a failure in red across the page. It is stated once, as an error badge or one alert or error message; no red banners, red rows or red headings.
- **Don't** restyle a native control with custom fills, radii or borders outside its shared component: a button's look is `Button`'s.
- **Don't** introduce a second accent color, a fourth status hue, or a color without a contrast-checked pair.
- **Don't** set text or control sizes in pixels, or use physical properties (`left`, `width`, `margin-top`).
- **Don't** animate anything that the reduced-motion rule can't turn off.
