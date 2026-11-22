# Decision 141: The header's content stands in the page's frame, and the pages' links take the text's color

**A Ledger Gray band spans the window, and the header's content stands in the page's frame, as `<main>` does: as wide as Settings' Page width lays each page, and 1rem from the frame's inline edges, so the product's name starts where each page's heading does. The pages' links are drawn in the color of the text around them, underlined, the current page bold; in the header, Ladder Blue is the focus ring's alone** (recorded 2026-11-22, §4.4 and §4.5; chosen in a design critique of the header, for the maintainer's approval with the change that makes it).

## Why

The header's content was pinned to the window's edges while `<main>` was a centered column of 60rem, so the header was the one full-width row on a centered page. At 1280 px the product's name started 160 px left of the page's heading, at 1600 px 320 px, and 335 px of empty band lay between the last link and the end of the line.

The seven pages' links were Ladder Blue, the one accent for action. Above the fold at 1280 px, Operations drew it seven times for navigation and twice for its own Start backup buttons; Plans drew it seven times against one. The quietest part of the page was its loudest, and the page's own action was never the first blue on the screen.

## Decided

1. **The band and the frame.** The band (`py-3 bg-surface`, Tailwind utilities on a `<div>` in `main.rs`) carries the Ledger Gray surface and 0.75rem of block padding. The header's content (`.site-header`) takes the frame's Tailwind utilities (`PageWidth::frame_class`, which `main_class` shares: centered at 60rem in Narrow, the window's width in Full width) and `<main>`'s 1rem of inline padding. Their content shares both edges and follows the Page width setting.
2. **The links' color.** `Navigation` draws each page's link with `LinkTone::Inherit`, as the brand does: the color of the text around it, always underlined, the current page bold with `aria-current="page"` (The Weight Not Color Rule). Under forced colors each still takes the system's link color. The contrast gate's `text on surface` pair covers them.
3. **The line's width.** The pages' links are 0.5rem apart, not 1rem, and Sign out has no gap of its own before the last link: its padding is the room. In a pseudolocale the seven links and Sign out took 967 px in DejaVu Sans, which a Linux machine falls back on, so Sign out wrapped alone under the frame's 928 px; they take 903 px now. The line stands on its parts' last baselines, as Sign out's already did, so WebKit, which takes a flex container's first baseline from its first item, draws Sign out's words on the links' baseline.
4. **What stays.** The brand on its own line, the order of the pages, where Sign out stands and how it wraps on a narrow screen, the band's height (88 px at 1280 px wide) and the header's order of Tab. The running version is not the header's: the footer shows it (decision 142).

## What it costs

In Full width on a very wide window the header's content is spread as wide as the page's tables are. A page's link in the header is now told by its underline and, for the current page, its weight alone, as the brand's always was.
