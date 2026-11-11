#!/usr/bin/env node
// Token contrast gate (technical design §4.5, A4). Reads the color tokens
// from crates/web/style.css and fails any named pair below WCAG 2.2 AA:
// 4.5:1 for text, 3:1 for UI parts like the focus ring. style.css stays the
// source of truth; Tailwind maps the same tokens, so it needs no second table.
//
// Usage: node scripts/check-contrast.ts crates/web/style.css
// Exit status: 0 when every pair passes, 1 otherwise.

import { readFile } from "node:fs/promises";

// The pairs the UI relies on, each with its WCAG threshold. Badge variants
// pair each badge's text token with its background token.
const PAIRS = [
  { name: "text on surface", fg: "--fg", bg: "--surface", min: 4.5 },
  { name: "muted text on surface", fg: "--muted", bg: "--surface", min: 4.5 },
  { name: "focus ring on surface", fg: "--focus", bg: "--surface", min: 3 },
  { name: "loading badge", fg: "--badge-loading-fg", bg: "--badge-loading-bg", min: 4.5 },
  { name: "empty badge", fg: "--badge-empty-fg", bg: "--badge-empty-bg", min: 4.5 },
  { name: "error badge", fg: "--badge-error-fg", bg: "--badge-error-bg", min: 4.5 },
  { name: "success badge", fg: "--badge-success-fg", bg: "--badge-success-bg", min: 4.5 },
  { name: "link on surface", fg: "--accent", bg: "--surface", min: 4.5 },
  { name: "text on page", fg: "--fg", bg: "--bg", min: 4.5 },
  { name: "link on page", fg: "--accent", bg: "--bg", min: 4.5 },
  { name: "form error on page", fg: "--badge-error-fg", bg: "--bg", min: 4.5 },
  { name: "table rule on page", fg: "--muted", bg: "--bg", min: 3 },
];

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

function luminance(rgb: Rgb): number {
  const [lr, lg, lb] = rgb.map((c) => {
    const srgb = c / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  }) as Rgb;
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
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
    console.log(
      `${ok ? "pass" : "FAIL"} ${where}: ${ratio.toFixed(2)}:1 (needs ${pair.min}:1) — ${fg} on ${bg}`,
    );
  }
}

if (failed > 0) {
  console.error(`${failed} pair(s) below WCAG 2.2 AA, or tokens missing.`);
  process.exit(1);
}
