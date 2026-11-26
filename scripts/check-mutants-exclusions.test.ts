// Tests for the check that every exclusion in .cargo/mutants.toml still
// matches a mutant (scripts/check-mutants-exclusions.ts), on fixed lists of
// mutant names: no cargo runs. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { deadExclusions, globRegExp, parseExclusions } from "./check-mutants-exclusions.ts";

const NAMES = [
  "crates/web/src/main.rs:443:46: replace == with != in App",
  "crates/web/src/main.rs:12:5: replace main with ()",
  "crates/web/src/components/button.rs:10:5: replace Button -> impl IntoView with Default::default()",
  "crates/web/src/components/button/rules.rs:4:5: replace label -> String with String::new()",
  "crates/app/src/contract/store/mod.rs:7:9: delete ! in check",
  "crates/domain/src/error.rs:20:9: replace <impl fmt::Display for InvalidZoneName>::fmt -> fmt::Result with Ok(Default::default())",
];

/** The source files of `NAMES`, and one of DOM glue that has no mutant. */
const FILES = [
  "crates/web/src/main.rs",
  "crates/web/src/components/button.rs",
  "crates/web/src/components/button/rules.rs",
  "crates/app/src/contract/store/mod.rs",
  "crates/domain/src/error.rs",
  "crates/web/src/settings/language.rs",
];

test("the lists are read, comments and blank lines skipped, both kinds of string", () => {
  const toml = [
    "# a comment",
    "exclude_globs = [",
    '    "crates/web/src/components/*.rs",',
    "",
    "    # why",
    '    "crates/app/src/contract/**"',
    "]",
    "exclude_re = [",
    '    "<impl fmt::Display for InvalidZoneName>",',
    "    'crates/web/src/main\\.rs:443:46: replace == with != in App$',",
    "]",
    "timeout_multiplier = 3.0",
  ].join("\n");
  assert.deepEqual(parseExclusions(toml), {
    exclude_globs: ["crates/web/src/components/*.rs", "crates/app/src/contract/**"],
    exclude_re: ["<impl fmt::Display for InvalidZoneName>", "crates/web/src/main\\.rs:443:46: replace == with != in App$"],
  });
});

test("a list written another way fails rather than pass unread", () => {
  assert.throws(() => parseExclusions('exclude_re = [\n    "a", "b",\n]'), /mutants\.toml:2/);
  assert.throws(() => parseExclusions('exclude_re = [\n    "a",\n'), /never closes/);
  assert.throws(() => parseExclusions('exclude_re = ["a"]'), /mutants\.toml:1: write the list/);
  assert.throws(() => parseExclusions('exclude_globs = [  # why\n    "a",\n]'), /mutants\.toml:1: write the list/);
});

test("exclusions that each match a mutant or a source file pass", () => {
  const exclusions = {
    exclude_re: [
      "crates/web/src/main\\.rs:443:46: replace == with != in App$",
      "crates/web/src/main\\.rs.*: replace main with \\(\\)$",
      "<impl fmt::Display for (InvalidZone|InvalidZoneName)>",
    ],
    exclude_globs: ["crates/web/src/components/*.rs", "crates/app/src/contract/**"],
  };
  assert.deepEqual(deadExclusions(exclusions, NAMES, FILES), []);
});

test("a glob of a source file with no mutant passes: glue can be excluded before it has one", () => {
  const exclusions = { exclude_re: [], exclude_globs: ["crates/web/src/settings/language.rs"] };
  assert.deepEqual(deadExclusions(exclusions, NAMES, FILES), []);
});

test("an exclusion whose line moved is named, with where the same mutant is now", () => {
  const exclusions = { exclude_re: ["crates/web/src/main\\.rs:459:46: replace == with != in App$"], exclude_globs: [] };
  assert.deepEqual(deadExclusions(exclusions, NAMES, FILES), [
    "exclude_re 'crates/web/src/main\\.rs:459:46: replace == with != in App$' matches no mutant; the same mutant is now at crates/web/src/main.rs:443:46",
  ]);
});

test("an exclusion whose code is gone is named, each dead one once", () => {
  const exclusions = {
    exclude_re: ["<impl fmt::Display for Gone>", "crates/web/src/main\\.rs:443:46: replace == with != in Gone$"],
    exclude_globs: ["crates/web/src/gone.rs"],
  };
  assert.deepEqual(deadExclusions(exclusions, NAMES, FILES), [
    "exclude_re '<impl fmt::Display for Gone>' matches no mutant",
    "exclude_re 'crates/web/src/main\\.rs:443:46: replace == with != in Gone$' matches no mutant",
    "exclude_globs 'crates/web/src/gone.rs' matches no source file",
  ]);
});

test("a regex the check can't read is named, not thrown", () => {
  const found = deadExclusions({ exclude_re: ["(unclosed"], exclude_globs: [] }, NAMES, FILES);
  assert.equal(found.length, 1);
  assert.match(found[0] ?? "", /^exclude_re '\(unclosed': not a regex/);
});

test("a glob's `*` stays in its folder and `**` crosses folders, none between two slashes", () => {
  assert.equal(globRegExp("crates/web/src/components/*.rs").test("crates/web/src/components/button.rs"), true);
  assert.equal(globRegExp("crates/web/src/components/*.rs").test("crates/web/src/components/button/rules.rs"), false);
  assert.equal(globRegExp("crates/app/src/contract/**").test("crates/app/src/contract/store/mod.rs"), true);
  assert.equal(globRegExp("crates/l10n/src/**/browser.rs").test("crates/l10n/src/browser.rs"), true);
  assert.equal(globRegExp("crates/l10n/src/**/browser.rs").test("crates/l10n/src/intl/browser.rs"), true);
  assert.equal(globRegExp("crates/web/src/main.rs").test("crates/web/src/mainxrs"), false);
  assert.equal(globRegExp("crates/web/src/sett?ngs.rs").test("crates/web/src/settings.rs"), true);
});

test("a glob with a `/` names one path, so a folder's name alone matches nothing under it", () => {
  assert.equal(globRegExp("crates/app/src/contract").test("crates/app/src/contract/store/mod.rs"), false);
  assert.equal(globRegExp("crates/app/src/contract/").test("crates/app/src/contract/store/mod.rs"), false);
});

test("a glob without a `/` matches a file or folder of that name anywhere", () => {
  assert.equal(globRegExp("main.rs").test("crates/web/src/main.rs"), true);
  assert.equal(globRegExp("main.rs").test("crates/web/src/domain.rs"), false);
  assert.equal(globRegExp("contract").test("crates/app/src/contract/store/mod.rs"), true);
});

test("a glob with braces or brackets is named as unreadable, not passed or thrown", () => {
  const found = deadExclusions({ exclude_re: [], exclude_globs: ["crates/web/src/{live,page}.rs"] }, NAMES, FILES);
  assert.deepEqual(found, [
    "exclude_globs 'crates/web/src/{live,page}.rs': not a glob this check can read (Error: braces and brackets aren't read)",
  ]);
});
