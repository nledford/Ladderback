// Runs only the @ui scenarios from the shared features/ folder. The Rust
// runners skip @ui, and this runner skips everything else (§13.2). cucumber-js
// finds only a .js, .cjs, .mjs, .json or YAML config by itself, not a .ts
// one, so every call names this file: `--config cucumber.ts` (the Justfile,
// package.json and scripts/red-first.ts).

import type { IConfiguration } from "@cucumber/cucumber/api";

const config: Partial<IConfiguration> = {
  paths: ["../features/**/*.feature"],
  import: ["support/**/*.ts", "steps/**/*.ts"],
  tags: "@ui",
  strict: true,
  format: ["progress"],
};

export default config;
