// Tests for the web UI's precompression (scripts/precompress.ts), which
// Trunk's post-build hook runs over a release build of crates/web. `just
// lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { brotliDecompressSync, gunzipSync } from "node:zlib";

import { precompress } from "./precompress.ts";

function inScratch(run: (folder: string) => void): void {
  const folder = mkdtempSync(join(tmpdir(), "precompress-"));
  try {
    run(folder);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
}

const TEXT = "<p>The page's text, which repeats as a module's code does.</p>\n".repeat(200);

test("a file compression makes smaller gets a Brotli and a gzip copy that decode to it", () => {
  inScratch((folder) => {
    mkdirSync(join(folder, "assets"));
    writeFileSync(join(folder, "index.html"), TEXT);
    writeFileSync(join(folder, "assets", "web_bg.wasm"), TEXT);
    precompress(folder);
    for (const file of [join(folder, "index.html"), join(folder, "assets", "web_bg.wasm")]) {
      const brotli = readFileSync(`${file}.br`);
      const gzip = readFileSync(`${file}.gz`);
      assert.ok(brotli.length < TEXT.length, `${file}.br is no smaller`);
      assert.ok(gzip.length < TEXT.length, `${file}.gz is no smaller`);
      assert.equal(brotliDecompressSync(brotli).toString(), TEXT);
      assert.equal(gunzipSync(gzip).toString(), TEXT);
    }
  });
});

test("a file compression doesn't make smaller gets no copy", () => {
  inScratch((folder) => {
    writeFileSync(join(folder, "icon.png"), randomBytes(512));
    writeFileSync(join(folder, "empty.txt"), "");
    precompress(folder);
    assert.deepEqual(readdirSync(folder).sort(), ["empty.txt", "icon.png"]);
  });
});

test("a second run compresses no copy and leaves the copies as they were", () => {
  inScratch((folder) => {
    writeFileSync(join(folder, "index.html"), TEXT);
    precompress(folder);
    const brotli = readFileSync(join(folder, "index.html.br"));
    precompress(folder);
    assert.deepEqual(readdirSync(folder).sort(), ["index.html", "index.html.br", "index.html.gz"]);
    assert.deepEqual(readFileSync(join(folder, "index.html.br")), brotli);
  });
});
