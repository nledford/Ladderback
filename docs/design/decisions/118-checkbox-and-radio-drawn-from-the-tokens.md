# Decision 118: A checkbox and a radio drawn from the tokens, not the browser's own

**A checkbox's and a radio's box is drawn from the tokens, as the text field is (decision 113): the native `<input>` with its own drawing dropped (`appearance: none`), a 1 px Field Edge round the page's color, never under 24 px a side and 1.5 times its text, and once chosen the edge and the fill in Ladder Blue under a mark in the page's color, a Lucide check or a dot. Its error edges it 2 px in the Error tone, and it takes the text field's description and error parts. Under forced colors it takes the reader's field colors, the mark alone telling chosen from not** (recorded 2026-11-17, §4.5; #840, part of #776).

## Why

- **The same reasons as the text field's.** The browser's own box differs by engine and scheme, and neither its edge nor its fill is a token the contrast gate can check (1.4.11). Drawn from the tokens, the edge is the text field's pair (`--field-edge`, 3:1 on the page and a surface) and the chosen fill and its mark are pairs of their own in `scripts/check-contrast.ts`.
- **Not `accent-color`.** Tinting the native box with `accent-color` would leave its edge and its mark to each engine, which picks the mark's color itself and draws the unchosen edge in a gray of its own, so the gate still couldn't check them, and it can't take the Error tone's edge.
- **Still the native element.** The `<input>` underneath keeps Space, the arrow keys within a group, the one Tab stop of a group, form state, the label as its click target and the reader's assistive technology as they were (§4.5, "Native HTML first"). The mark is drawn over the box, hidden from a screen reader and from the pointer, and shown by the box's own `:checked`, so nothing on the page keeps a state of its own.

## Cost

- **Forced colors needs its own rules.** With its drawing dropped, the box's fill would be forced to the page's, and a dot drawn as a fill would vanish; the box takes `Field` and `FieldText`, and the mark `FieldText` with `forced-color-adjust: none`, which `features/ui/components/choices.feature` checks.
- **No indeterminate state yet.** A drawn box shows only the marks it is given; a page that needs an indeterminate or unavailable box adds its mark then.
