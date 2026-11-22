// Precompression of the web UI's release build (technical design §10.1).
// Trunk's post-build hook (crates/web/Trunk.toml) runs it over the staging
// folder of a release build, before Trunk moves that folder to
// crates/web/dist. Each file gets a Brotli copy (`<name>.br`, quality 11) and
// a gzip copy (`<name>.gz`, level 9) beside it, each only when it is smaller
// than the file, and the daemon sends the copy a request's Accept-Encoding
// allows (crates/adapters/http/src/precompressed.rs). The work is done once
// per build, so a request costs only the lookup. Node's own zlib does it: no
// package to install.
//
// Usage: node scripts/precompress.ts <folder>

import { readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

/** The suffixes of the copies, which are never compressed again. */
const COPIES = [".br", ".gz"];

function brotli(bytes: Buffer): Buffer {
  return brotliCompressSync(bytes, {
    params: {
      [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY,
    },
  });
}

function gzip(bytes: Buffer): Buffer {
  return gzipSync(bytes, { level: constants.Z_BEST_COMPRESSION });
}

/** Every file under `folder`, copies left out. */
function filesUnder(folder: string): string[] {
  return readdirSync(folder, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && !COPIES.some((suffix) => entry.name.endsWith(suffix)))
    .map((entry) => path.join(entry.parentPath, entry.name));
}

/** Writes the Brotli and the gzip copy of each file under `folder` that
 *  compression makes smaller. Returns how many copies it wrote. */
export function precompress(folder: string): number {
  let written = 0;
  for (const file of filesUnder(folder)) {
    const bytes = readFileSync(file);
    for (const [suffix, compress] of [
      [".br", brotli],
      [".gz", gzip],
    ] as const) {
      const copy = compress(bytes);
      if (copy.length < bytes.length) {
        writeFileSync(`${file}${suffix}`, copy);
        written += 1;
      }
    }
  }
  return written;
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const folder = process.argv[2];
  if (!folder) {
    console.error("usage: node scripts/precompress.ts <folder>");
    process.exit(2);
  }
  const started = performance.now();
  const written = precompress(folder);
  console.log(`precompress: wrote ${written} copies in ${Math.round(performance.now() - started)} ms`);
}
