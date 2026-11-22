# Decision 142: The running version stands in a footer at the foot of every page

**The running version leaves the header for a footer, a shared component and the page's `contentinfo` landmark after the main content: "Version 0.4.0+3f937cf" in Slate Note under a Slate Note hairline across the window, its content in the main content's frame, at the window's foot on a short page. The update notice stays in the header, on a line of its own under the pages' links and Sign out** (decided by the maintainer 2026-11-22, §4.5, DESIGN.md "Footer").

## Why

The version is reference: a reader looks for it when reporting a problem or checking an update took, and never acts on it. In the header it took the end of the links' line, so on a narrow screen it wrapped there with Sign out and the header grew a line for words no one needs on every visit. A footer is where people look for a build, and it frees the header's line for what they act on. The update notice asks the reader to act, so it stays where they look first.

## Decided

1. **The footer** is `components::Footer`: a `<footer>` placed as `<main>`'s next sibling, so it is the page's one `contentinfo` landmark (a footer inside a section, as in the gallery's demo, is none). It holds one paragraph, the build worded `ui-version`. Given no version it draws nothing, not an empty landmark.
2. **Where it shows** is where the version showed: every page that shows the navigation, with sign-in off or signed in. The sign-in page has no footer, as it named no build (decision 107).
3. **Quiet.** The words are Slate Note on the page's own background, with no fill and no shadow (the Flat Ledger Rule), under a 1px Slate Note rule. The rule spans the window, as the header's band does; the content stands in the main content's frame (Settings' Page width), its words 1rem in, where the page's text starts, in either direction.
4. **At the foot of the window.** The header, the main content and the footer stand in a column at least as tall as the window (`min-block-size: 100dvh`), and the footer's top margin takes the room a short page leaves. Narrow's frame fills its line up to 60rem, since a box with auto margins in a column would shrink to what it holds.
5. **Read in order, never announced.** The version is plain text: no stop for Tab, and no live region (A3).

## What it costs

The version is one landmark further from the page's start: a screen-reader user reaches it by the landmarks list or the end of the page, not the header. On a long page it is below the fold, so checking it means scrolling to the end, which is where a page's build usually is.
