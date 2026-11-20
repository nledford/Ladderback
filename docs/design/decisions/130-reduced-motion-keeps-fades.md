# Decision 130: What reduced motion stops, and what it keeps

**Under `prefers-reduced-motion: reduce`, anything that moves, resizes, rotates, scales or loops stops: every keyframe animation (the spinner's turn), and every transition of anything but a color or the opacity (the progress fill's growth, any change of size or position). A short fade of a color or the opacity stays, as feedback that moves nothing: the tooltip's 100 ms fade, and the change cue of a live row, which settles over 600 ms. One rule in `style.css` does it, and no component can turn its stopping half back on** (recorded 2026-11-20, §4.5; the maintainer's decision).

## Why

The rule this replaces stopped every animation and every transition. That kept motion away from a reader it can make ill (WCAG 2.3.3), but it also took away feedback that moves nothing: a tooltip snapped in, and a live row could have no cue at all. What 2.3.3 guards against is motion: movement, zoom, parallax, spinning. A color that settles once in place is a change of state, not motion, and the reader who asks for less motion still needs to see what changed. So reduced motion means fewer motions and none spatial, rather than none of any kind.

## Decided

1. **What stops.** Every animation (`animation: none`), every smooth scroll, and every transition of a property that isn't a color or the opacity: the rule sets each element's `transition-property`, its `::before`, `::after`, `::backdrop` and `::marker` too, to opacity, the colors (`color`, `background-color`, `border-color`, `outline-color`, `text-decoration-color`, `fill`, `stroke`) and `display`, `overlay` and `visibility`, which hold a fading element until its fade ends (`transition-behavior: allow-discrete`). The spinner stands still beside its label, and the progress fill jumps to each report, as before.
2. **What stays.** A fade of those properties, short enough to be feedback: 600 ms at most, the change cue's length. Under emulated reduced motion, `features/ui/change_cue.feature` checks a live Operations page and `features/ui/components/tooltip.feature` the component gallery: no animation runs, and no transition is set on anything else or for longer.
3. **No opting out of the stopping half.** The rule is `!important`, in a cascade layer of its own, the first the page names (`style.css` loads before Tailwind's layers). An important declaration in the first layer beats one in every later layer and in none, so no component rule and no utility, `!`-marked or not, can bring a movement back. Out of its reach are an inline `!important` style and motion a script runs (the Web Animations API, SMIL); the app uses none, and the checks above would see a Web Animation.
4. **One duration for a kept transition.** The rule lists properties of its own, which the browser pairs with a component's durations by position, so a component whose transition this rule keeps gives all its properties one duration.
5. **The change cue** (§4.5, "Live updates"): a row of a live table whose status a live update changed is lit once in the cue's fill (`--cue-fill`; Ledger Gray when this was recorded) and settles back to its own fill over 600 ms; never as the page first shows its rows, for an update that leaves the status as it was, or while live updates are paused, and once for each changed row as they resume. It is a color, so it stays under reduced motion. Its rules apply only outside forced colors, where the system's colors are the cells' and there is no cue. The mark that stays until the row is seen is decision 129.

## What it costs

A reader who wanted no change of any kind on the screen now sees a tooltip fade over 100 ms and a changed row settle over 600 ms. Neither moves, flashes or loops, and both are shorter than the 5 s after which 2.2.2 asks for a way to stop moving content.

The rule also turns a kept transition's duration onto every color, so a transition that moves in full motion can fade a color it doesn't fade there: the progress fill would fade a change of its tone over its 400 ms, a tone that doesn't change while a bar shows. A new transition is held to the 600 ms the journeys check.
