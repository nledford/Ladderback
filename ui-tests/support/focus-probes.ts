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

/** The points at which to look for the focused element: the center of each
 *  of its fragments, one for each line an inline element wraps onto, never
 *  the center of the box around them all, which can lie between two lines,
 *  where only the parent is (#949). An empty fragment, as at a line break, is
 *  left out; an element with no fragment of its own is looked at through its
 *  whole box. Given `inPart`, for an engine that scrolls a focused element
 *  only until part of it shows, the point is the center of the part of each
 *  fragment in the viewport, and a fragment wholly out of it is left out.
 *  The element is in sight where any point finds it; no point at all means
 *  it is out of sight. */
export function focusProbes(fragments: Box[], whole: Box, viewport: { width: number; height: number }, inPart: boolean): Point[] {
  const hasArea = (b: Box): boolean => b.left < b.right && b.top < b.bottom;
  const own = fragments.filter(hasArea);
  return (own.length > 0 ? own : [whole])
    .map((b) =>
      inPart
        ? {
            left: Math.max(b.left, 0),
            right: Math.min(b.right, viewport.width),
            top: Math.max(b.top, 0),
            bottom: Math.min(b.bottom, viewport.height),
          }
        : b,
    )
    .filter(hasArea)
    .map((b) => ({ x: (b.left + b.right) / 2, y: (b.top + b.bottom) / 2 }));
}
