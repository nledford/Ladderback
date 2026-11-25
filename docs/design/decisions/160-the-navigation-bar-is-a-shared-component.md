# Decision 160: The navigation bar is a shared component, and below 54rem its panel stands over the page

**The header's navigation is `NavigationBar`, a shared component (§4.5): the brand, the pages' links and a group of rows behind a disclosure menu, one line from 54rem up and behind a hamburger below. Below 54rem its panel stands over the page under the brand's line, as the Settings menu's panel does, rather than in the page's flow, and closes as that panel does: on Escape, a row chosen, focus leaving the bar or a press outside it. Its rows stand in as many columns of 10em as its width holds, each at least 2.75rem tall. The header stays where the page puts it: it doesn't stick to the window's top** (recorded 2026-11-24, §4.4 and §4.5, #796 and #1582; it supersedes the panel "in the page's flow" and the 3rem rows of [decision 143](143-a-hamburger-menu-holds-the-pages-links-below-41rem.md)).

## Why

PR #1496 made the header one line with a Settings menu (decision 152) and left two parts of #796: the navigation as a shared component, with its gallery entry, contract and scenarios (development.md §13.7.5), and a check that 320 px with 200 % text doesn't fill the screen. An Impeccable critique of the navbar (2026-11-24, on `main` at `40d6327d`) measured what the in-flow panel of decision 143 cost below 54rem:

- Open, it took 414 px of a 390 × 844 window (49 %) and 938 px at 320 px with 200 % text, 134 % of the window. The page's heading moved a full screen down, and the reader scrolled inside the navigation to reach Log.
- Closing it meant scrolling back to an X that had left the window: no press outside closed it (#1582), and a touch screen has no Escape.
- The header's two disclosures disagreed: the Settings panel closed on a press outside, the hamburger's didn't.

## Decided

1. **One component.** `NavigationBar` (`components/navigation_bar.rs`) draws the brand its caller passes, a `<nav>` of `NavigationPage`s and a `DisclosureMenu` for the group, and below 54rem the hamburger and its panel. What it decides is `components/navigation_bar/rules.rs`, host-tested, which takes over `navigation.rs`. The caller names the navigation, the group and the hamburger, and gives the panel's `id`; the hamburger's and the group's are made from it, so two bars on one page never share one. The gallery's "Navigation bar" demo is a second bar.
2. **Over the page.** Below 54rem the open panel stands over what follows the brand's line (`position: absolute`), the width of the bar, under its line, in the page's color with the `--field-edge` edge and the small radius. Opening and closing it moves nothing on the page: the reader's place, and the page's heading, stay where they were.
3. **Closed as the Settings menu is.** Escape closes it and returns focus to the hamburger, while the hamburger is drawn. Focus moving to an element outside the bar closes it, since it would cover that element (2.4.11). A press of the pointer outside the bar closes it, the touch reader's way back (#1582). Choosing a row closes it; the current page's row changes no route, so focus goes back to the hamburger. When it closes on focus moving, on a press and on the current page's row are the disclosure menu's own rules (`disclosure_menu::rules`), so the two disclosures can't drift apart again. No focus trap, no overlay behind it.
4. **Rows in columns.** Its rows stand in as many columns as its width holds at 10em each, the group's rows starting a row of their own and its last rows (Sign out) under the hairline across the panel. At 390 px with the browser's own text size that is two columns: five rows of 44 px instead of eight of 48. At 320 px with 200 % text it is one column. Each row is at least 2.75rem tall, a finger's target, as the Settings menu's rows are.
5. **Not sticky.** At 320 px with 200 % text the closed header is about 210 px of a 568 px window; held at the window's top it would take that much of every screen and cover what focus moves to under it (2.4.11). The header scrolls away with the page, as it did.
6. **The check.** At 320 × 568 px with 200 % text the closed header leaves the page's heading inside the window, and the open panel is one column that leaves the heading where it was (`features/ui/header.feature`).
7. **A failed sign-out takes focus to its alert** (the maintainer, 2026-11-24). The alert stands at the top of `<main>`, which the open panel covers below 54rem, so a failed sign-out hands focus to it (`SignOutFailure`, a box with `tabindex="-1"` around the shared Alert, which stays no Tab stop): focus leaving the bar closes the panel, a sighted reader sees the alert, and a screen reader lands on it. It does so at every width, so the Settings menu closes the same way.

## What it costs

The open panel covers the top of the page, so a reader can't read the page and the rows at once below 54rem; any press outside or Tab past the rows gives the page back. At 320 px with 200 % text the open panel is still taller than the window: eight rows a finger can hit don't fit beside the brand's line there, and the page scrolls under it to reach the last. The 10em columns wrap a long translation's words inside a row rather than widening it. One more component, with its own tests and gallery demo.

## Alternatives

- **Keep the panel in the page's flow, with shorter rows:** the page's heading still moves on every opening, and a press outside, which now closes it, would make the page jump under the reader's finger.
- **A drawer from the side, or a modal panel:** a focus trap and a backdrop for a list of links, which the Settings menu's model already serves.
- **A sticky header:** see 5.
- **The panel's own scrolling, its height the window's:** the reader scrolls inside the navigation, which the critique named as the problem.
