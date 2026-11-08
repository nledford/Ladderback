// Runs only the @ui scenarios from the shared features/ folder. The Rust
// runners skip @ui, and this runner skips everything else (§13.2).
export default {
  paths: ["../features/**/*.feature"],
  import: ["support/**/*.mjs", "steps/**/*.mjs"],
  tags: "@ui",
  strict: true,
  format: ["progress"],
};
