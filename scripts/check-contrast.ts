#!/usr/bin/env node
// Token contrast gate (technical design §4.5, A4). Reads the color tokens
// from crates/web/style.css and fails any named pair below WCAG 2.2 AA:
// 4.5:1 for text, 3:1 for UI parts like the focus ring. style.css stays the
// source of truth; Tailwind maps the same tokens, so it needs no second table.
// It also fails two status fills that must be told apart and sit too close
// in lightness (FILLS_APART), and a status fill that must read at least as
// strongly as another beside it and doesn't (FILLS_STRONGER).
//
// Usage: node scripts/check-contrast.ts crates/web/style.css
// Exit status: 0 when every pair passes, 1 otherwise.

import { readFile } from "node:fs/promises";

// The pairs the UI relies on, each with its WCAG threshold. Badge variants
// pair each badge's text token with its background token, and each status
// tone's pair (variants.rs, `StatusTone`) its words' token with its fill's.
// The tones of the shared vocabulary (variants.rs) draw text and strokes on
// the page and on a surface, so each tone's token is paired with both:
// Primary is the link pairs, Secondary the muted text, Inherit the text
// around it. A tooltip is the page's colors swapped: the page's background
// on its text color.
// A link (§4.5) is always underlined, so it has no 3:1 pair against the text
// around it: its color never tells it apart alone.
const PAIRS = [
  { name: "text on surface", fg: "--fg", bg: "--surface", min: 4.5 },
  { name: "muted text on surface", fg: "--muted", bg: "--surface", min: 4.5 },
  { name: "focus ring on surface", fg: "--focus", bg: "--surface", min: 3 },
  { name: "loading badge", fg: "--badge-loading-fg", bg: "--badge-loading-bg", min: 4.5 },
  { name: "empty badge", fg: "--badge-empty-fg", bg: "--badge-empty-bg", min: 4.5 },
  { name: "success words on their fill", fg: "--success-fg", bg: "--success-bg", min: 4.5 },
  { name: "warning words on their fill", fg: "--warning-fg", bg: "--warning-bg", min: 4.5 },
  { name: "error words on their fill", fg: "--error-fg", bg: "--error-bg", min: 4.5 },
  { name: "link on surface", fg: "--accent", bg: "--surface", min: 4.5 },
  { name: "text on page", fg: "--fg", bg: "--bg", min: 4.5 },
  { name: "link on page", fg: "--accent", bg: "--bg", min: 4.5 },
  { name: "form error on page", fg: "--error-fg", bg: "--bg", min: 4.5 },
  { name: "table rule on page", fg: "--muted", bg: "--bg", min: 3 },
  { name: "secondary tone on page", fg: "--muted", bg: "--bg", min: 4.5 },
  { name: "success tone on page", fg: "--success", bg: "--bg", min: 4.5 },
  { name: "success tone on surface", fg: "--success", bg: "--surface", min: 4.5 },
  { name: "warning tone on page", fg: "--warning", bg: "--bg", min: 4.5 },
  { name: "warning tone on surface", fg: "--warning", bg: "--surface", min: 4.5 },
  { name: "error tone on page", fg: "--error", bg: "--bg", min: 4.5 },
  { name: "error tone on surface", fg: "--error", bg: "--surface", min: 4.5 },
  ...buttonPairs(),
  ...progressPairs(),
  { name: "tooltip text", fg: "--bg", bg: "--fg", min: 4.5 },
  // A text field (components §4.5): its edge against the page and a
  // surface (1.4.11); its invalid edge is the error tone's pairs above, its
  // text the text on page, a disabled one's muted text on surface.
  { name: "text field edge on page", fg: "--field-edge", bg: "--bg", min: 3 },
  { name: "text field edge on surface", fg: "--field-edge", bg: "--surface", min: 3 },
  // A skeleton (components §4.5): its bars against the page and Ledger Gray.
  // A floor for visibility, not a WCAG minimum: a skeleton carries no
  // information, so 1.4.11 doesn't apply, but it must still be seen.
  { name: "skeleton on page", fg: "--skeleton", bg: "--bg", min: 1.4 },
  { name: "skeleton on surface", fg: "--skeleton", bg: "--surface", min: 1.4 },
  // The change mark (components §4.5, decision 129): a live row's edge
  // against the page and the cue's fill it fades from (1.4.11), and that
  // fill's floor against the page: not a WCAG minimum, since the edge and
  // the bold carry the mark and the fill only its arrival, but it must
  // still be seen. While lit, the row's text, links and focus ring lie on
  // the fill. A status tone's words lie on its badge's own fill, never on
  // the cue's, so no tone is paired with it.
  { name: "change mark's edge on page", fg: "--cue", bg: "--bg", min: 3 },
  { name: "change mark's edge on its fill", fg: "--cue", bg: "--cue-fill", min: 3 },
  { name: "change mark's fill on page", fg: "--cue-fill", bg: "--bg", min: 1.3 },
  { name: "text on the change mark's fill", fg: "--fg", bg: "--cue-fill", min: 4.5 },
  { name: "muted text on the change mark's fill", fg: "--muted", bg: "--cue-fill", min: 4.5 },
  { name: "link on the change mark's fill", fg: "--accent", bg: "--cue-fill", min: 4.5 },
  { name: "focus ring on the change mark's fill", fg: "--focus", bg: "--cue-fill", min: 3 },
  // The tabs (components §4.5): the selected tab's bar, the shape that
  // tells it with its weight, against the page and a surface (1.4.11). A
  // tab's words are the text on page; the list's rule the table rule.
  { name: "selected tab's bar on page", fg: "--accent", bg: "--bg", min: 3 },
  { name: "selected tab's bar on surface", fg: "--accent", bg: "--surface", min: 3 },
  // A toast (components §4.5) in a status tone lies on its pair's fill:
  // the focus ring around its Dismiss button against that fill (1.4.11),
  // its edge, in its tone, against that fill as well as the page (the tone
  // pairs above), so the edge holds 3:1 on both its sides, and the button's
  // label, the toast's words' color, on the surface it takes on hover.
  ...["success", "warning", "error"].flatMap((tone) => [
    { name: `focus ring on the ${tone} fill`, fg: "--focus", bg: `--${tone}-bg`, min: 3 },
    { name: `${tone} toast edge on its fill`, fg: `--${tone}`, bg: `--${tone}-bg`, min: 3 },
    { name: `${tone} words on surface`, fg: `--${tone}-fg`, bg: "--surface", min: 4.5 },
  ]),
  // A checkbox and a radio (components §4.5): a chosen box's edge and fill
  // against the page and a surface, and its mark, in the page's color, on
  // that fill (1.4.11). An unchosen box's edge is the text field's edge.
  { name: "chosen box on page", fg: "--accent", bg: "--bg", min: 3 },
  { name: "chosen box on surface", fg: "--accent", bg: "--surface", min: 3 },
  { name: "chosen box's mark on its fill", fg: "--bg", bg: "--accent", min: 3 },
];

// Status fills that must be told apart, and how far apart in lightness, on
// OKLab's scale from black (0) to white (1) (decision 108). Warning and
// Error are neighboring hues, amber and red, which a reader who can't tell
// red from green (protanopia, deuteranopia) sees as one hue, and a dark
// fill holds too little color for its hue to tell; lightness survives both.
// A tenth of the scale keeps them apart: dark ones 0.07 apart, a 1.24:1
// luminance ratio, read as one. The words and the icon still say which is
// which (1.4.1); this keeps two fills from looking alike beside each other.
const FILLS_APART = [{ name: "warning and error fills", a: "--warning-bg", b: "--error-bg", min: 0.1 }];

// A status fill that must read at least as strongly as another beside it
// (decision 108), so the fills read in the order of their severity: Warning's
// beside Success's, as on the Operations page, and Error's beside Warning's. How
// strongly a fill reads is how far it stands from what lies around it, the
// page or a surface, and the stronger fill must stand further on both scales
// the gate uses: by WCAG's contrast ratio, the scale of every pair above, at
// least as far; and in OKLab lightness, a perceptual scale, at least 0.02
// further, the difference CSS Color 4 takes as just noticeable (a deltaEOK of
// 0.02), so the reader sees it rather than a tie that rounding could flip.
// Lightness, as for FILLS_APART, survives a reader who can't tell green from
// amber, and both scales agreeing keeps either from hiding a fill the other
// reads as paler.
const FILLS_STRONGER = [
  { name: "warning fill over success fill", stronger: "--warning-bg", weaker: "--success-bg", min: 0.02 },
  { name: "error fill over warning fill", stronger: "--error-bg", weaker: "--warning-bg", min: 0.02 },
];
const AROUND_FILLS = [
  { name: "page", token: "--bg" },
  { name: "surface", token: "--surface" },
];

// A button (components §4.5) in each tone but Inherit, whose colors are the
// text's own: a Filled button's label (`--on-fill`) on its fill and on the
// fill a step darker it takes on hover (lighter in the dark scheme); its
// edge, which is the fill on a Filled button and the tone on an Outlined
// one, against the page and a surface (1.4.11), and the hover's; and the
// hover's label of an Outlined or a Ghost button on the surface it takes.
// Their resting labels are the tone pairs above.
function buttonPairs() {
  const tones = [
    { name: "primary", tone: "--accent", hover: "--accent-hover" },
    { name: "secondary", tone: "--muted", hover: "--muted-hover" },
    { name: "success", tone: "--success", hover: "--success-hover" },
    { name: "warning", tone: "--warning", hover: "--warning-hover" },
    { name: "error", tone: "--error", hover: "--error-hover" },
  ];
  return tones.flatMap(({ name, tone, hover }) => [
    { name: `filled ${name} button`, fg: "--on-fill", bg: tone, min: 4.5 },
    { name: `filled ${name} button, hovered`, fg: "--on-fill", bg: hover, min: 4.5 },
    { name: `${name} button edge on page`, fg: tone, bg: "--bg", min: 3 },
    { name: `${name} button edge on surface`, fg: tone, bg: "--surface", min: 3 },
    { name: `${name} button edge on page, hovered`, fg: hover, bg: "--bg", min: 3 },
    { name: `${name} button edge on surface, hovered`, fg: hover, bg: "--surface", min: 3 },
    { name: `${name} button label on surface, hovered`, fg: hover, bg: "--surface", min: 4.5 },
  ]);
}

// A progress bar (components §4.5) in each tone it takes: its fill, a part
// with no words on it, against its Ledger Gray track and against the page
// around the track (1.4.11). Inherit fills with the text's own color. The
// fill takes the tone's own token, not its status pair, which is for words
// on a tinted fill (decision 108).
function progressPairs() {
  const tones = [
    { name: "primary", tone: "--accent" },
    { name: "secondary", tone: "--muted" },
    { name: "success", tone: "--success" },
    { name: "warning", tone: "--warning" },
    { name: "error", tone: "--error" },
    { name: "inherit", tone: "--fg" },
  ];
  return tones.flatMap(({ name, tone }) => [
    { name: `${name} progress fill on its track`, fg: tone, bg: "--surface", min: 3 },
    { name: `${name} progress fill on page`, fg: tone, bg: "--bg", min: 3 },
  ]);
}

const SCHEMES = [
  { name: "light", pick: tokensFromLight },
  { name: "dark", pick: tokensFromDark },
];

type Tokens = Record<string, string>;
type Rgb = [number, number, number];

/** The declarations of the first `:root` block (light). */
function tokensFromLight(css: string): Tokens {
  const match = css.match(/^:root\s*{([^}]*)}/m);
  return parseDeclarations(match?.[1] ?? "");
}

/** The declarations of the `:root` inside the dark-scheme media block. */
function tokensFromDark(css: string): Tokens {
  const blocks = [...css.matchAll(/prefers-color-scheme:\s*dark\)\s*{\s*:root\s*{([^}]*)}/g)];
  return parseDeclarations(blocks.at(-1)?.[1] ?? "");
}

function parseDeclarations(block: string): Tokens {
  const tokens: Tokens = {};
  for (const [, name, value] of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    if (name !== undefined && value !== undefined) tokens[name] = value.trim();
  }
  return tokens;
}

function parseHex(color: string): Rgb {
  const hex = /^#([\da-f]{6})$/i.exec(color)?.[1];
  if (hex === undefined) {
    throw new Error(`not a 6-digit hex color: ${color}`);
  }
  const n = Number.parseInt(hex, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** The color's channels in linear light, as WCAG's luminance takes them. */
function linear(rgb: Rgb): Rgb {
  return rgb.map((c) => {
    const srgb = c / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  }) as Rgb;
}

function luminance(rgb: Rgb): number {
  const [lr, lg, lb] = linear(rgb);
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}

/** OKLab's perceived lightness (Björn Ottosson, 2020): 0 is black, 1 white. */
function lightness(rgb: Rgb): number {
  const [r, g, b] = linear(rgb);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
}

function contrastRatio(fg: string, bg: string): number {
  const [a, b] = [luminance(parseHex(fg)), luminance(parseHex(bg))].sort((x, y) => y - x) as [number, number];
  return (a + 0.05) / (b + 0.05);
}

const css = await readFile(process.argv[2] ?? "crates/web/style.css", "utf8");

let failed = 0;
for (const scheme of SCHEMES) {
  const tokens = scheme.pick(css);
  for (const pair of PAIRS) {
    const where = `${scheme.name}: ${pair.name}`;
    const fg = tokens[pair.fg];
    const bg = tokens[pair.bg];
    if (fg === undefined || bg === undefined) {
      failed += 1;
      const missing = [pair.fg, pair.bg].filter((t) => !(t in tokens));
      console.error(`FAIL ${where}: ${missing.join(", ")} not defined in style.css`);
      continue;
    }
    const ratio = contrastRatio(fg, bg);
    const ok = ratio >= pair.min;
    if (!ok) failed += 1;
    console.log(`${ok ? "pass" : "FAIL"} ${where}: ${ratio.toFixed(2)}:1 (needs ${pair.min}:1) — ${fg} on ${bg}`);
  }
  for (const fills of FILLS_APART) {
    const where = `${scheme.name}: ${fills.name}`;
    const a = tokens[fills.a];
    const b = tokens[fills.b];
    if (a === undefined || b === undefined) {
      failed += 1;
      const missing = [fills.a, fills.b].filter((t) => !(t in tokens));
      console.error(`FAIL ${where}: ${missing.join(", ")} not defined in style.css`);
      continue;
    }
    const apart = Math.abs(lightness(parseHex(a)) - lightness(parseHex(b)));
    const ok = apart >= fills.min;
    if (!ok) failed += 1;
    console.log(`${ok ? "pass" : "FAIL"} ${where}: ${apart.toFixed(3)} apart in lightness (needs ${fills.min}) — ${a} and ${b}`);
  }
  for (const fills of FILLS_STRONGER) {
    for (const around of AROUND_FILLS) {
      const where = `${scheme.name}: ${fills.name} on ${around.name}`;
      const strong = tokens[fills.stronger];
      const weak = tokens[fills.weaker];
      const bg = tokens[around.token];
      if (strong === undefined || weak === undefined || bg === undefined) {
        failed += 1;
        const missing = [fills.stronger, fills.weaker, around.token].filter((t) => !(t in tokens));
        console.error(`FAIL ${where}: ${missing.join(", ")} not defined in style.css`);
        continue;
      }
      const ground = lightness(parseHex(bg));
      const further = Math.abs(lightness(parseHex(strong)) - ground) - Math.abs(lightness(parseHex(weak)) - ground);
      const strongRatio = contrastRatio(strong, bg);
      const weakRatio = contrastRatio(weak, bg);
      const ok = further >= fills.min && strongRatio >= weakRatio;
      if (!ok) failed += 1;
      console.log(
        `${ok ? "pass" : "FAIL"} ${where}: ${further.toFixed(3)} further in lightness (needs ${fills.min}), ` +
          `${strongRatio.toFixed(3)}:1 against ${weakRatio.toFixed(3)}:1 (needs at least as much) — ${strong} and ${weak} on ${bg}`,
      );
    }
  }
}

if (failed > 0) {
  console.error(`${failed} pair(s) below WCAG 2.2 AA or their minimum, or tokens missing.`);
  process.exit(1);
}
