import type { Locator } from "playwright";

// The words an element shows, on one line: the text of its rendered text
// nodes, each run of white space one space, without the bidi isolation marks.
// Not `innerText`, whose spaces depend on where the lines wrap: where a line
// wraps before an inline-block placeable (style.css, `td .placeable`),
// WebKit drops the space before it ("IPv6 bya wired network"), as a row of
// the table does on Linux, whose fonts are wider than the Mac's.
export const said = (element: Locator): Promise<string> =>
  element.evaluate((root) => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let text = "";
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (node.parentElement?.checkVisibility({ visibilityProperty: true })) text += node.textContent;
    }
    return text.replace(/[\u2066-\u2069]/g, "").replace(/\s+/g, " ").trim();
  });
