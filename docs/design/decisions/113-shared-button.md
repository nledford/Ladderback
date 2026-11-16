# Decision 113: A button drawn from the tokens, not the browser's own

**A button is the shared `Button` component: a native `<button>` drawn from the tokens rather than in the browser's own look, in one of three appearances (Filled, Outlined, Ghost) and a tone of the shared vocabulary, Outlined in Secondary by default; its size is its height in `em`; one small corner, flat; unavailable and busy are `aria-disabled`, never `disabled`. DESIGN.md's rule that buttons keep the browser's look gives way to it, and pages move onto it in their own change** (recorded 2026-11-16, §4.5; decided by the maintainer on #779, part of #776).

## Why

- **One look in every engine.** The browser's own button looks different in each engine and scheme, and a page can't say which of its buttons is the main action without a color of its own. Drawn from the tokens, each appearance and tone is a text-and-background pair the contrast gate checks (`scripts/check-contrast.ts`), in light and dark, and a hover a step darker (lighter in dark) is checked too.
- **What axe-core reads is what is painted.** Linux WebKit reports a native button's background as its user-agent `ButtonFace` (`#c0c0c0`) whatever it paints, so axe-core read white text on it as 1.81:1 in the dark scheme (#775). A button with a background of its own reports that background; #780, which moves the pages onto `Button`, closes #775.
- **Still the native element.** A `<button>` underneath keeps Enter and Space, form submission (`type="submit"` only when asked; `type="button"` otherwise, so no button sends a form by accident), focus and the reader's assistive technology as they were (§4.5, "Native HTML first"). It inherits the page's font (The Reader's Size Rule).
- **Three appearances, from loud to quiet.** Filled for an action row's one main action, Outlined (the default) for the others, Ghost for a row that has many. Every tone makes sense for Outlined and Ghost; a Filled button has no Inherit, because its label would need a color checked against whatever color the text around it takes, so it is drawn Outlined.
- **Sizes are heights.** The shared scale's lengths size a glyph (decision 108); a button's size is its height in `em`, Small 1.75em, Medium 2em (DESIGN.md's 2rem at the page's text size), Large 2.5em and XLarge 3em, its padding growing with it, at least 24 CSS px at 100 % (2.5.8). The icon and spinner inside it stay as tall as the text.
- **State never by color alone.** Unavailable is 60 % opacity and a not-allowed cursor; pressed is bolder with an inset edge; under forced colors a button takes the reader's `ButtonText` and `ButtonFace`, an unavailable one `GrayText`.
- **A busy button keeps its words.** Its spinner takes its icon's place, so a button with an icon keeps its width; one with only words grows by the spinner and its gap, since its words say what is running ("calm and exact"). A button that must not move gets an icon.

## Left out

- **From Flowbite, the reference:** gradients, colored shadows, pills, social and brand buttons, its "Dark" color, and links drawn as buttons (the Link component's, #781).
- **A button group** (#783), which a later change builds around `Button`.
