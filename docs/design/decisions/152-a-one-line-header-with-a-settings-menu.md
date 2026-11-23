# Decision 152: A one-line header with a Settings menu, and links standing alone without an underline

**From 54rem up the header is one line: the product's name at its start, then the five pages' links and a Settings button at its end, which opens a disclosure menu holding Settings, Log and Sign out. Below 54rem the hamburger menu of decision 143 holds every row. A link standing alone (a page in the header, a menu's row, a snapshot's ID in its table) shows no underline at rest; a link among words keeps its underline** (recorded 2026-11-23, §4.4 and §4.5, #1452 and #1453; it supersedes the second line of [decision 141](141-the-header-stands-in-the-page-frame.md) and the 41rem breakpoint of [decision 143](143-a-hamburger-menu-holds-the-pages-links-below-41rem.md)).

## Why

The header took two lines at every width above 41rem: the brand's, then seven links and Sign out under it. Two of those links are not places a reader goes in a day's work, Settings and the Log, and Sign out isn't a page at all. Grouped behind one button they leave five pages, which fit beside the brand on one line at a laptop's width, and the header gives its second line back to the page.

Every link in the header was underlined, as every link was (decision 141). An underline is what tells a link from the words around it; a row of links in a navigation bar has no words around it, and five underlines in a row read as noise. Links standing alone are links by where they stand, as every navigation bar's are (WCAG 1.4.1 asks that a link among words be told apart by more than its color, not one standing alone).

## Decided

1. **The breakpoint.** 54rem (864 px at the browser's base size), in a viewport media query in rem, as decision 143's was. The brand, the five links and the Settings button take about 785 px in en-US with room for Pending changes' count, "(12)", which #1476 adds (measured: at 864 px the line holds up to 110 % text), and 815 px with 2rem left between the brand and the links; 54rem leaves 5 % more than that for a longer translation. Below it the hamburger menu holds the five pages, then Settings, Log and Sign out, as decision 143 laid out, and the Settings button isn't drawn.
2. **The Settings menu** (`DisclosureMenu`, a shared component):
   - A native `<button>` with its words, "Settings", and a chevron that turns over while the menu shows; `aria-expanded` carries the state and `aria-controls` names the panel. Not the shared `Button`: its words and chevron are a navigation's look, in the text's color and the links' size, not a control's.
   - The panel hangs under the button's inline end, over the page (`position: absolute`), in the page's own color, edged in `--field-edge` and rounded with the small radius, as decision 143's panel is. Its rows are at least 44 px tall: Settings and Log, then Sign out under a hairline.
   - On Settings or the Log the button is marked `aria-current="true"`, bold and with the current page's bar, so the header still says where the reader is when the page's link is in the menu.
   - Enter or Space toggles it. Escape closes it and returns focus to the button. Choosing a row closes it as the page changes, and focus goes to the new page's heading as on every route change; the current page's row changes no route, so focus goes back to the button. A press outside it and focus leaving it close it without moving focus. A disclosure, not an ARIA menu: Tab walks its rows, and no arrow keys are needed or taken.
3. **Links standing alone** (`LinkPlacement::Alone`): no underline at rest; the underline under the pointer; on focus the focus ring alone, as #1452 asks; the deeper Ladder Blue (`--accent-hover`) while pressed, whatever the link's tone; the current page marked by a 3px bar at its inline start in the text's color, and bold. Under forced colors the system's link color, and the bar the system's text color. A link among words (`LinkPlacement::InText`) is unchanged. Every `Link` names its placement: no default, so no call site takes one by accident.
4. **The pointer in the tests.** `ui-tests/support/pointer.ts` gains one action, a press held on one element (`holdOn`), for the pressed link's color and the press outside the menu, approved by the maintainer on 2026-11-23. Every control is still reached and pressed by keyboard alone.

## What it costs

Settings and the Log are one more press away from 54rem up, behind the Settings button. Between 41rem and 54rem the hamburger menu now stands where the two-line header did, so a reader there presses twice to reach any page. The underline no longer marks the header's links at rest; their place in the header does. One more shared component, with its own tests and gallery demo.

## Alternatives

- **The header on two lines as before,** only without the underlines: the second line of links stays, which this decision set out to give back.
- **An ARIA menu (`role="menu"`) for Settings:** arrow keys and roving focus that a list of links doesn't need, and screen readers announce it as an application menu (APG's disclosure navigation pattern is the one for links).
- **A breakpoint at 41rem with a smaller font in the header:** the 320 Rule's text sizes would differ by place.
- **Every link underlined, standing alone or not:** consistent, and the noise this decision removes.
