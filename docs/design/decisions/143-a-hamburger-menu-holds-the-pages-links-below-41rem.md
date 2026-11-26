# Decision 143: Below 41rem a hamburger menu holds the pages' links and Sign out

**Below 41rem the header's seven links and Sign out sit in a menu behind a hamburger button: an icon-only Ghost button, 44 px square at least, that ends the brand's line where it fits. Shown, the menu is a bordered panel under that line in the page's flow, a row for each page and one for Sign out, each 3rem tall at least. Escape, a page chosen or a sign-out hides it again. The header is then the brand's line alone** (recorded 2026-11-22, §4.4 and §4.5; the maintainer chose the pattern on 2026-11-22 in the navigation bar critique, with Flowbite's navbar as the reference: <https://flowbite.com/docs/components/navbar/#hamburger-menu>).

**Partly superseded by decision 152** (2026-11-23, #1453): the hamburger menu is drawn below 54rem, not 41rem, where the header's one line no longer fits, and its rows are the five pages, then Settings, the Log and Sign out. The rest of this record is kept as it was decided.

**Partly superseded by [decision 160](160-the-navigation-bar-is-a-shared-component.md)** (2026-11-24, #796, #1582): the menu is a part of the shared `NavigationBar`, and shown, its panel stands over the page rather than in its flow, its rows in as many 10em columns as fit, each 2.75rem tall at least. Focus leaving it and a press outside it hide it too.

## Why

Below 41rem (656 px) the seven links no longer fit on one line: 614 px of links beside 2rem of the frame's padding. They wrapped onto two lines at 390 px and six at 320 px with 200 % text, with Sign out under them, so the header took 18.5 % of a 390×844 screen, 29 % of one 320 px wide, and the whole first screen at 320 px with 200 % text, before the first word of a page. The 320 Rule was met, and the reader's first screen was still navigation.

The links were 32 px tall with 4 px between wrapped rows: above WCAG 2.5.8's 24 px, below the 44 px a finger wants, a slip away from the neighboring page.

This decision first put the links behind a text button, "Pages" with a chevron. The maintainer asked for the common mobile pattern instead, a hamburger menu, which readers of a small screen already know without reading: an icon at the end of the brand's line that opens a panel of links. It follows Flowbite's navbar: an icon-only button with `aria-controls` and `aria-expanded` and a name only a screen reader hears, and a full-width panel under the brand's line holding the links stacked as rows with room around their words, in a bordered, rounded box, each row tinting under the pointer.

## Decided

1. **The breakpoint.** 41rem, in a viewport media query: the narrowest whole rem at which the seven links and the frame's padding still fit on one line. In rem, so it follows the browser's base font size, as the page's text does when the reader raises it there. The 320 Rule is in pixels and holds on either side of the breakpoint.
2. **The button.** `MenuButton` (`main.rs`):
   - The shared `Button`, icon-only (`Content::IconOnly`), Ghost, XLarge, so it is 3em square: 48 px at the page's own text size, over the 44 px a finger wants, and larger as the reader's text is.
   - Its icon is Lucide's `Menu` (the hamburger) while the panel is hidden and `X` while it shows. Its name is "Menu" (`ui-menu-button`), the same in both states: `aria-expanded` carries the state, and `aria-controls` names the panel.
   - As every icon-only button's is, its name is also its tooltip, which stands at the button's inline start, so it covers neither the panel nor the page.
   - It is drawn below 41rem alone, level with the brand on the brand's line where it fits, from 390 px at 100 % text up, and at the line's end under it where it doesn't, as at 320 px with 200 % text.
   - A disclosure, not a dialog: no overlay, no focus trap, the panel in the page's flow. Tab reaches the button, then each link, then Sign out: the order never changes.
3. **The panel.**
   - A box in the page's own color on the header's Ledger Gray band, the width of the header's content, rounded with the small radius and edged all round in the text field's edge token (`--field-edge`, over 3:1 on the page and the band, in light and dark).
   - Not Ledger Gray itself, since the band is: the box would be an outline with nothing to tell it from its surroundings, and a hover tint would have nothing to be tinted from.
   - Its rows are blocks the panel's width: the seven links, then Sign out under a hairline, each at least 3rem tall (0.75rem of block padding around a line of text) with 1rem of inline padding. A row under the pointer tints to Ledger Gray, and its underline thickens as every link's does.
   - The current page keeps its weight (decision 141) and gains a 3px bar at its inline start in the text's color (`--fg`, never Ladder Blue). The bar is a border, so it stays under forced colors, where it takes the system's text color. No other row has one, and the current row's inline padding is the bar's width less, so every row's words stand aligned.
   - The focus ring is drawn inside the row. Sign out is the shared button at the panel's width, as tall as the links, its icon where their words start.
4. **Shown and hidden.** `navigation.rs` decides, tested on the host: the button toggles, Escape anywhere in the header hides the panel and returns focus to the button, but only while the button is drawn, and a page chosen from the panel shows it closed, so the header is as tall again as it was. Signing out shows it closed too: the next sign-in finds it hidden, whatever it was. A sign-out that fails leaves it open, where focus is. Wider than 41rem none of this applies: the button isn't drawn, the panel draws no box and the links and Sign out stand on the header's line, always shown, whatever the button was last pressed to do. The running version is the footer's at every width (decision 142).
5. **No motion.** The panel appears and goes at once. Nothing moves, resizes or fades, so the reduced-motion rule (decision 130) has nothing to turn off.
6. **Where the rules live.** In `style.css`, in one block below the header's own rules: those sit outside Tailwind's layers (#931), where a utility could not override them.
7. **The icon.** `Menu` is in Lucide's `layout` category, which the workspace's `lepticons-data` turns on for it: 98 icons in no category already on. The category adds 29,560 B (0.49 %) to the release `web` wasm built with `--no-default-features`, 5,986,103 B before and 6,015,663 B after, and 4,220 B (0.37 %) to its brotli, 1,151,933 B before and 1,156,153 B after (technology.md §8.1).

## What it costs

A reader below 41rem presses twice to reach any page from another, the button and the link, and the current page is no longer marked in the header there until the panel is open: the page's own heading says where the reader is. Sign out is one more press away, behind the button. The brand's line is 16 px taller where the button stands on it, 48 px beside the brand's 32. The button's icon is shorter than its 48 px square, a hamburger's own proportion. Focus on a header control that a window resize across 41rem stops drawing falls to the page, as for any control that stops being drawn.

## Alternatives

- **The text button "Pages" with a chevron,** this decision's first form: no one has to know an icon, and it names the navigation. Against it, the maintainer's reading of what small-screen readers expect, and the space: a Large button with words is wider than a hamburger where the brand's line is short.
- **An overlay or a drawer from the side:** a focus trap and a dismissal, for no task the page's flow doesn't do.
- **Fewer links on a small screen:** the information architecture would differ by context.
