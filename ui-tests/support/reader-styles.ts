// What a reader can override. WCAG 1.4.12 sets these minimums and the page
// must still show all its text. Browsers have no switch for them, so a
// stylesheet stands in for the reader's own.
export const TEXT_SPACING_CSS = `
  * {
    line-height: 1.5 !important;
    letter-spacing: 0.12em !important;
    word-spacing: 0.16em !important;
  }
  p {
    margin-block-end: 2em !important;
  }
`;

// A reader's 200 % default text size; the page must use relative units.
export const DOUBLE_FONT_CSS = "html { font-size: 200% !important; }";
