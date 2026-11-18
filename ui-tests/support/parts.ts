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

// Where a sentence breaks (#972): between words, never inside a word that
// fits on a line of its own; only a word longer than a whole line, such as a
// long path, may break inside. Runs in the page, on the element that holds
// the words; `line` is the widest line in pixels its words can be given,
// which the caller measures, as the element's own box may be narrower than
// that. It lists the words that break inside and the count of words it
// measured. A word is what lies between spaces, zero-width ones too, and
// a break after its hyphen, or at a soft hyphen, is a break a reader expects,
// not one inside it. Each character's box says which line it is on, a bidi
// isolation mark isn't text a reader sees, and a character of no width is on
// no line. It is self-contained, as `evaluate` sends only its source to the
// page.
export function wordsBrokenInside(element: Element, { line }: { line: number }): { words: number; inside: string[] } {
  const words: { char: string; box: DOMRect }[][] = [];
  let word: { char: string; box: DOMRect }[] = [];
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const content = node.textContent ?? "";
    for (let at = 0; at < content.length; at += 1) {
      const char = content.charAt(at);
      if (/[\u2066-\u2069]/.test(char)) continue;
      if (/[\s\u200B]/.test(char)) {
        if (word.length > 0) words.push(word);
        word = [];
        continue;
      }
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + 1);
      const box = [...range.getClientRects()].find((rect) => rect.width > 0);
      if (box !== undefined) word.push({ char, box });
    }
  }
  if (word.length > 0) words.push(word);
  const inside: string[] = [];
  for (const letters of words) {
    const wide = letters.reduce((sum, { box }) => sum + box.width, 0);
    // Within a pixel of the line, a word can need it all and more: the
    // characters' boxes leave out the run's sub-pixel layout.
    if (wide > line - 1) continue;
    letters.forEach(({ box }, at) => {
      const before = letters[at - 1];
      if (before === undefined || /[-\u2010\u00AD]/.test(before.char)) return;
      if (Math.round(box.top) > Math.round(before.box.top)) {
        inside.push(`"${letters.map(({ char }) => char).join("")}" (${Math.round(wide)}px on a ${Math.round(line)}px line) at ${at}`);
      }
    });
  }
  return { words: words.length, inside };
}
