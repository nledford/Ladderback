# Critique ignore list

Findings a critique drops, each with the decision behind it.

- **`side-tab` on an alert, a banner, a toast or the selected tab** (component source, `/_gallery` or any page). The start-edge bar is the component's designed edge: DESIGN.md's Weight Not Color Rule and its Ladder Blue entry. A `side-tab` on any other element is still reported.
- **Any finding in `crates/web/tw-out.css` or `crates/web/dist/`.** Generated Tailwind and Trunk output, not committed; review the component that produced the class instead.
- **"No CLI findings" from `impeccable detect` on `crates/web/src/**/*.rs`.** The detector can't parse Leptos `view!` macros, so a source scan proves nothing. Scan the running UI by URL instead, and say so in Run Notes rather than reporting the source as clean.
- **Dashboard-chrome suggestions: cards, shadows, gradients, glows, big hero numbers.** DESIGN.md forbids them (the Flat Ledger Rule and its Don'ts).
- **Physical-property or pixel-size suggestions** (`left`, `width`, `margin-top`, `px` text). DESIGN.md's Logical and Reader's Size Rules.
