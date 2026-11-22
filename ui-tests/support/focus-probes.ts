// Where the keyboard helper (keyboard.ts) looks for what covers a focused
// element (2.4.11, §4.5 "Tests"). Pure, so that `node --test` can test it
// (scripts/focus-probes.test.ts); the page measures the boxes and tries the
// points.

/** A box in the viewport's coordinates, as `getBoundingClientRect` and
 *  `getClientRects` give it. */
export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface Point {
  x: number;
  y: number;
}

/** The points at which to look for the focused element: the center of the
 *  part in the viewport of each of its fragments, one for each line an
 *  inline element wraps onto, never the center of the box around them all,
 *  which can lie between two lines, where only the parent is (#949). An
 *  empty fragment, as at a line break, is left out; an element with no
 *  fragment of its own is looked at through its whole box. A fragment wholly
 *  out of the viewport is left out, so an element wholly out of it has no
 *  point: it is out of sight. WCAG 2.4.11 fails a focused element only when
 *  it is entirely hidden, so the part in view is what counts, however the
 *  engine scrolled: Firefox scrolls a focused element only until part of it
 *  shows, and Chromium and WebKit, when part of it already shows, only until
 *  its nearest edge does, which leaves the center of an element taller than
 *  the viewport out of it (#1292). The element is in sight where any point
 *  finds it. */
export function focusProbes(fragments: Box[], whole: Box, viewport: { width: number; height: number }): Point[] {
  const hasArea = (b: Box): boolean => b.left < b.right && b.top < b.bottom;
  const own = fragments.filter(hasArea);
  return (own.length > 0 ? own : [whole])
    .map((b) => ({
      left: Math.max(b.left, 0),
      right: Math.min(b.right, viewport.width),
      top: Math.max(b.top, 0),
      bottom: Math.min(b.bottom, viewport.height),
    }))
    .filter(hasArea)
    .map((b) => ({ x: (b.left + b.right) / 2, y: (b.top + b.bottom) / 2 }));
}
