# Decision 46: The tooling's language

**The tooling (`scripts/`, the l10n expectations generator, the UI tests) is TypeScript that Node runs as it is, by stripping the types: no build step, nothing emitted, and `tsc --noEmit` in `just lint` is the only place the types are checked** (recorded 2026-11-10; #144).

The scripts were JavaScript, so a wrong argument or a misspelled field showed only when a run reached it.

Decided:

1. **The configuration.** `tsconfig.json` at the root covers the three trees with `strict`, `noEmit`, `erasableSyntaxOnly` (only syntax Node can strip: no `enum`, no parameter properties, no `namespace`), `allowImportingTsExtensions` and `module: nodenext`, because Node needs the `.ts` in each import.
   - It adds two options beyond what type stripping needs: `verbatimModuleSyntax`, so a type-only import must say `import type` and Node never loads a module for a name it strips, and `noUncheckedIndexedAccess`, so an index into an array or a record is `T | undefined` and a script that reads past its arguments or its parsed JSON has to say what happens then.
2. **The package.** A root `package.json` with `type: module`, so Node and `tsc` treat `scripts/` and the generator as ES modules, and `engines.node >= 26.0.0` (raised on 2026-11-15 from 22.18.0, the first LTS that strips types without a flag: CI runs only the Node `ui-tests/.nvmrc` names, 26, so the older minimum was one nothing ran, and building from source already needs 26, decision 79).
   - It has no dependencies: `typescript`, `@types/node` and `@biomejs/biome` are `ui-tests`' exact dev-dependencies.
3. **The minimum.** The check holds the code to that minimum: `target` and `lib` are `es2025`, all of which Node 26 has, so a newer built-in fails `tsc`, and `@types/node` follows the minimum's major, 26, so a `node:` API Node 26 lacks fails too.
   - `lib` adds the DOM for the UI tests' `page.evaluate` callbacks, and the scripts see those types as well, the price of one check over three trees.
4. **The cucumber configuration.** cucumber-js finds no `.ts` configuration by itself, so `ui-tests/cucumber.ts` is named with `--config` everywhere the UI tests run: `just test-ui`, the npm `test` script and `scripts/red-first.ts` (when the tree it checks has one).
5. **The type check.** `just lint` runs `tsc --project tsconfig.json`, after `_ui-deps` installs `ui-tests`' packages when they are missing or older than the lockfile, so the first `just lint` in a fresh worktree needs the network.
6. **The binary.** The UI tests' world finds the binary itself, `LADDERBACK_BIN` or else `debug/ladderback` under the target directory `cargo metadata` names, which replaces `test-ui`'s inline lookup and makes a bare `npm test` follow `CARGO_TARGET_DIR` (it needs `cargo` on the `PATH` when `LADDERBACK_BIN` is unset).
7. **The format and the lint.** `biome.json` at the root has Biome (`@biomejs/biome`, an exact `ui-tests` dev-dependency, as the compiler is) format and lint the same three trees, leaving `.worktrees` and `target` alone (Biome stops at a second `biome.json`, and each worktree and each `red-first` scratch tree has one). `just lint` runs `biome ci --error-on-warnings`, which writes nothing and fails on a warning as on an error; `just fmt` writes the format, and so needs `_ui-deps`'s packages too.
   - The formatter's settings match the code that was there: two spaces, double quotes, semicolons, and a line width of 135, the one that changed the fewest lines of those tried. The lint set is Biome's recommended rules; a rule is turned off only in `biome.json` (JSON has no comments, so the reason goes here), and a single line is excused only by a `biome-ignore` comment that says why.
   - One rule is off: `performance/useDomNodeTextContent`. The UI steps read `innerText`, and Playwright's `innerText()`, on purpose: the text a person sees, with what is hidden left out, which `textContent` doesn't give.

Not chosen:

- **A build step** (`tsc` emitting JavaScript, or a bundler), which adds output to keep in step with its source for scripts that run once each.
- **JSDoc types over JavaScript**, which `tsc` checks but Node never sees.
- **Prettier with ESLint**, two tools and two configurations for what Biome does in one. The maintainer chose Biome (2026-11-19).
