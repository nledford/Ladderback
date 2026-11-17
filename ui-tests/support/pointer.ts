// Pointer moves, for what a keyboard can't reach: what a resting pointer
// shows. A tooltip shows when the pointer rests on its control and stays
// while the pointer moves onto it (WCAG 1.4.13, Content on Hover or Focus),
// and a link's underline grows thicker under it (§4.5); that is pointer
// behaviour, so their scenarios move the pointer.
// This helper only moves it and never presses: every control is still
// reached and used by keyboard alone (support/keyboard.ts), so no journey
// can pass by clicking (A1). `npm run lint` keeps pointer actions out of
// steps/, which call these moves by name.

import type { Locator, Page } from "playwright";

export function pointer(page: Page) {
  const moveTo = async (x: number, y: number): Promise<void> => {
    // A few steps, as a hand moves, so the page sees the pointer leave what
    // it was on before it arrives.
    await page.mouse.move(x, y, { steps: 4 });
  };
  return {
    /** Moves the pointer to the middle of the one element `locator` finds. */
    async restOn(locator: Locator): Promise<void> {
      const box = await locator.boundingBox();
      if (box === null) throw new Error("the pointer can't rest on what isn't shown");
      await moveTo(box.x + box.width / 2, box.y + box.height / 2);
    },

    /** Moves the pointer to the point `x`, `y` of the viewport. */
    moveTo,
  };
}
