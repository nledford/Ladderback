// Pointer moves, for what a keyboard can't reach: what a resting pointer
// shows. A tooltip shows when the pointer rests on its control and stays
// while the pointer moves onto it (WCAG 1.4.13, Content on Hover or Focus),
// and a link's underline grows thicker under it (§4.5); that is pointer
// behaviour, so their scenarios move the pointer.
// Its moves never press, but for one: the drag of a column in a table's
// Columns panel (decision 140), whose scenario checks dragging itself, a
// pointer's own behaviour, while its Move buttons do all of it by keyboard
// (2.5.7). Every control is still reached and used by keyboard alone
// (support/keyboard.ts), so no journey can pass by clicking (A1). `npm run
// lint` keeps pointer actions out of steps/, which call these by name.

import type { Locator, Page } from "playwright";

/** The drag handle of an item of a table's Columns panel: decoration a
 *  screen reader skips (the item's Move buttons are its way), so it has no
 *  role or name to find it by, only its mark (`components/table.rs`). */
export function dragHandleOf(item: Locator): Locator {
  return item.locator("[data-drag-handle]");
}

export function pointer(page: Page) {
  /** The first half of a drag: presses the pointer on the middle of
   *  `handle` and moves it, as a hand does, to a quarter of the way down
   *  `onto`, still pressed. Both are brought into view first, as a hand
   *  scrolls to what it drags: a pointer pressed past the viewport's edge
   *  presses nothing. */
  const holdOverTopHalf = async (handle: Locator, onto: Locator): Promise<void> => {
    await onto.scrollIntoViewIfNeeded();
    await handle.scrollIntoViewIfNeeded();
    const from = await handle.boundingBox();
    if (from === null) throw new Error("the pointer can't press what isn't shown");
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2, { steps: 4 });
    const target = await onto.boundingBox();
    if (target === null) throw new Error("the pointer can't drop onto what isn't shown");
    await page.mouse.down();
    await page.mouse.move(target.x + target.width / 2, target.y + target.height / 4, { steps: 8 });
  };
  /** The end of a drag: lets the pressed pointer go where it is. */
  const letGo = async (): Promise<void> => {
    await page.mouse.up();
  };
  const moveTo = async (x: number, y: number): Promise<void> => {
    // A few steps, as a hand moves, so the page sees the pointer leave what
    // it was on before it arrives.
    await page.mouse.move(x, y, { steps: 4 });
  };
  const restOn = async (locator: Locator): Promise<void> => {
    const box = await locator.boundingBox();
    if (box === null) throw new Error("the pointer can't rest on what isn't shown");
    await moveTo(box.x + box.width / 2, box.y + box.height / 2);
  };
  return {
    /** Moves the pointer to the middle of the one element `locator` finds. */
    restOn,

    /** Moves the pointer to the point `x`, `y` of the viewport. */
    moveTo,

    holdOverTopHalf,

    letGo,

    /** A whole drag, for a Columns panel's scenarios of dragging alone:
     *  `holdOverTopHalf`, then `letGo`. */
    async dragOntoTopHalf(handle: Locator, onto: Locator): Promise<void> {
      await holdOverTopHalf(handle, onto);
      await letGo();
    },
  };
}
