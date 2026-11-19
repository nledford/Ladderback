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

// A box as the page lays it out, in pixels from the viewport's corner.
export interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

// Words as they lie beside their icon (#1070, #1071): the icon's box and the
// box of each line the words take, top to bottom, and the page's direction.
export interface WordsBesideIcon {
  text: string;
  rtl: boolean;
  icon: Box | null;
  lines: Box[];
}

// Measures `WordsBesideIcon`. Runs in the page, on the element that holds
// both; `icon` is the selector of the icon inside it, and `words` that of
// the words, or "" for the element's own text outside the icon. A line is
// the boxes of the text on it, read from the text nodes, never from an
// element, whose box spans every line it holds; a fragment of no width (a
// bidi isolation mark, a space at the end of a line) is on no line. It is
// self-contained, as `evaluate` sends only its source to the page.
export function wordsBesideIcon(element: Element, { icon, words }: { icon: string; words: string }): WordsBesideIcon {
  const glyph = element.querySelector(icon);
  const root = words === "" ? element : element.querySelector(words);
  const lines: Box[] = [];
  const walker = root === null ? null : document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker?.nextNode() ?? null; node !== null; node = walker?.nextNode() ?? null) {
    if (glyph?.contains(node) || (node.textContent ?? "").trim() === "") continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    for (const rect of Array.from(range.getClientRects())) {
      if (rect.width === 0) continue;
      const middle = (rect.top + rect.bottom) / 2;
      const line = lines.find((seen) => middle > seen.top && middle < seen.bottom);
      if (line === undefined) {
        lines.push({ left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom });
      } else {
        line.left = Math.min(line.left, rect.left);
        line.right = Math.max(line.right, rect.right);
        line.top = Math.min(line.top, rect.top);
        line.bottom = Math.max(line.bottom, rect.bottom);
      }
    }
  }
  lines.sort((a, b) => a.top - b.top);
  const box = glyph?.getBoundingClientRect();
  return {
    text: root?.textContent?.trim() ?? "",
    rtl: getComputedStyle(element).direction === "rtl",
    icon: box === undefined ? null : { left: box.left, right: box.right, top: box.top, bottom: box.bottom },
    lines,
  };
}

// Half a pixel of rounding in a laid-out length.
const ROUNDING_PX = 0.5;

// What keeps words from lying beside their icon (#1070, #1071): an icon on a
// line of its own, its middle outside the words' first line, or a line of
// the words that starts under the icon rather than after it in the page's
// direction. Empty when the icon stands at the start of the first line and
// every line follows it.
export function besideIconProblems({ text, rtl, icon, lines }: WordsBesideIcon): string[] {
  if (icon === null) return [`"${text}" has no icon`];
  const first = lines[0];
  if (first === undefined) return [`"${text}" shows no words`];
  const middle = (icon.top + icon.bottom) / 2;
  const problems =
    middle >= first.top && middle <= first.bottom
      ? []
      : [`the icon of "${text}" is centered at ${middle}px, off its first line at ${first.top}–${first.bottom}px`];
  for (const [at, line] of lines.entries()) {
    const after = rtl ? line.right <= icon.left + ROUNDING_PX : line.left >= icon.right - ROUNDING_PX;
    if (!after) problems.push(`line ${at + 1} of "${text}" spans ${line.left}–${line.right}px, under its icon at ${icon.left}–${icon.right}px`);
  }
  return problems;
}
