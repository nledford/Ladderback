// Where a path or an address breaks, as a reader splits one (#482): after a
// slash, a dot, an at sign or a colon that a letter follows ("backup@"
// "storage." "example." "net:/srv/" …), never inside the part between two of
// them, unless that part is longer than a whole line; with `whole`, no part
// may break, so the line must have room for the longest. Runs in the page, on
// the element that shows `text`: it says how many lines the text takes and
// which parts break inside, or null when the element doesn't show it. The
// line is the one the reader sees, the list item or cell around the element,
// not the element's own box, which may be narrower than that line. Each
// character's box says which line it is on. A bidi isolation mark isn't
// text a reader sees, and a character of no width, such as a space at the
// end of a line, is on no line.
export function partsBrokenInside(
  element: Element,
  { text, whole }: { text: string; whole: boolean },
): { lines: number; inside: string[] } | null {
  const chars: { char: string; box: DOMRect | undefined }[] = [];
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const content = node.textContent ?? "";
    for (let at = 0; at < content.length; at += 1) {
      const char = content.charAt(at);
      if (/[\u2066-\u2069]/.test(char)) continue;
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + 1);
      chars.push({ char, box: [...range.getClientRects()].find((rect) => rect.width > 0) });
    }
  }
  const start = chars.map(({ char }) => char).join("").indexOf(text);
  if (start < 0) return null;
  const around = element.closest("li, td, th") ?? element;
  const style = getComputedStyle(around);
  const line = around.clientWidth - parseFloat(style.paddingInlineStart) - parseFloat(style.paddingInlineEnd);
  let part: { char: string; box: DOMRect }[] = [];
  const parts = [part];
  for (const { char, box } of chars.slice(start, start + text.length)) {
    if (box === undefined) continue;
    const last = part[part.length - 1];
    if (last !== undefined && /[/.@:]/.test(last.char) && /\p{L}/u.test(char)) {
      part = [];
      parts.push(part);
    }
    part.push({ char, box });
  }
  const tops = new Set(parts.flat().map(({ box }) => Math.round(box.top)));
  const inside: string[] = [];
  for (const letters of parts) {
    const wide = letters.reduce((sum, { box }) => sum + box.width, 0);
    // Within a pixel of the line, a part can need it all and more: the
    // characters' boxes leave out the run's sub-pixel layout.
    if (!whole && wide > line - 1) continue;
    letters.forEach(({ box }, at) => {
      const before = letters[at - 1];
      if (before !== undefined && Math.round(box.top) > Math.round(before.box.top)) {
        inside.push(`"${letters.map(({ char }) => char).join("")}" (${Math.round(wide)}px on a ${Math.round(line)}px line) at ${at}`);
      }
    });
  }
  return { lines: tops.size, inside };
}
