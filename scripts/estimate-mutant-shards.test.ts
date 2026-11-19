// Tests for scripts/estimate-mutant-shards.sh, which ci.yml's `changes` job
// runs to size the mutation shards. Each test builds a small repository (a
// Justfile with the gate's list, two crates and a mutants.toml), commits a
// change on a branch and reads the shard count the script answers. `just
// lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const script = join(import.meta.dirname, "estimate-mutant-shards.sh");

const justfile = (packages: string) => `root := justfile_directory()

mutants_packages := "${packages}"

[positional-arguments]
mutants base='origin/main' *args:
    cargo mutants {{ mutants_packages }}

mutants-all *args:
    cargo mutants {{ mutants_packages }}

fmt:
    cargo fmt
`;

const mutantsToml = (globs: string[]) => `# comment
exclude_globs = [
${globs.map((g) => `    "${g}",`).join("\n")}
]
exclude_re = ["Display"]
`;

const manifest = (name: string) => `[package]\nname = "${name}"\n`;

type Files = Record<string, string>;

const lines = (count: number) => "let x = 1;\n".repeat(count);

function repo(setup: Files = {}) {
  const dir = mkdtempSync(join(tmpdir(), "estimate-shards-"));
  const git = (...args: string[]) => {
    const result = spawnSync("git", ["-C", dir, "-c", "user.name=t", "-c", "user.email=t@example.com", ...args], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  const write = (files: Files) => {
    for (const [path, content] of Object.entries(files)) {
      mkdirSync(dirname(join(dir, path)), { recursive: true });
      writeFileSync(join(dir, path), content);
    }
  };
  mkdirSync(join(dir, "scripts"));
  copyFileSync(script, join(dir, "scripts", "estimate-mutant-shards.sh"));
  git("init", "-q", "-b", "main");
  write({
    Justfile: justfile("--package domain --package web-ui"),
    ".cargo/mutants.toml": mutantsToml(["crates/web/src/components/*.rs", "crates/domain/src/generated/**"]),
    "crates/domain/Cargo.toml": manifest("domain"),
    "crates/web/Cargo.toml": manifest("web-ui"),
    "crates/other/Cargo.toml": manifest("other"),
    ...setup,
  });
  git("add", "-A");
  git("commit", "-q", "-m", "base");
  git("checkout", "-q", "-b", "change");
  return {
    // Commits `files` on main, after the branch was cut from it.
    onMain(files: Files) {
      git("checkout", "-q", "main");
      write(files);
      git("add", "-A");
      git("commit", "-q", "-m", "main moves on");
      git("checkout", "-q", "change");
    },
    // Moves a committed file on the branch.
    rename(from: string, to: string) {
      mkdirSync(dirname(join(dir, to)), { recursive: true });
      git("mv", from, to);
    },
    // Commits `files` and answers what the script says about the change.
    estimate(files: Files) {
      write(files);
      git("add", "-A");
      git("commit", "-q", "-m", "change");
      const result = spawnSync("bash", [join(dir, "scripts", "estimate-mutant-shards.sh"), "main", "HEAD"], {
        encoding: "utf8",
      });
      const count = /^mutant-shard-count=(\d+)$/m.exec(result.stdout)?.[1];
      const shards = /^mutant-shards=(.*)$/m.exec(result.stdout)?.[1];
      return { status: result.status, count: count === undefined ? undefined : Number(count), shards, why: result.stderr.trim() };
    },
    done: () => rmSync(dir, { recursive: true, force: true }),
  };
}

function estimate(files: Files, setup: Files = {}) {
  const r = repo(setup);
  try {
    return r.estimate(files);
  } finally {
    r.done();
  }
}

test("a change with no gated Rust gets one shard", () => {
  const result = estimate({
    "docs/design/x.md": "text\n",
    "crates/other/src/lib.rs": lines(5000), // a crate outside the gate
    "crates/domain/tests/it.rs": lines(5000), // an integration test, not mutated
    "crates/domain/src/tests.rs": lines(5000), // a test module
    "crates/domain/src/parse/tests/cases.rs": lines(5000),
    "crates/domain/src/generated/table.rs": lines(5000), // an excluded glob
    "crates/web/src/components/button.rs": lines(5000), // `*` stays in a folder
    "crates/domain/README.md": lines(5000),
  });
  assert.equal(result.status, 0);
  assert.equal(result.count, 1);
  assert.equal(result.shards, "[0]");
});

test("one shard holds up to 590 changed lines, counting added and deleted lines", () => {
  // 100 lines deleted and 490 added are 590 changed lines; one more is 591.
  const base = { "crates/domain/src/lib.rs": lines(100) };
  const rewrite = (added: number) => estimate({ "crates/domain/src/lib.rs": "let z = 2;\n".repeat(added) }, base);
  assert.equal(rewrite(490).count, 1);
  assert.equal(rewrite(491).count, 2);
});

test("the shards grow with the changed lines to a most of four", () => {
  const count = (n: number) => estimate({ "crates/domain/src/lib.rs": lines(n) }).count;
  assert.equal(count(1180), 2);
  assert.equal(count(1181), 3);
  assert.equal(count(1770), 3);
  assert.equal(count(1771), 4);
  assert.equal(count(50000), 4);
});

test("the list of shards matches the count", () => {
  const result = estimate({ "crates/domain/src/lib.rs": lines(1200) });
  assert.equal(result.count, 3);
  assert.equal(result.shards, "[0,1,2]");
});

test("an exclude glob's * stays in its folder, its ** does not", () => {
  const nested = estimate({ "crates/web/src/components/tree/rules.rs": lines(600) });
  assert.equal(nested.count, 2, "a component's rules are mutated");
  const generated = estimate({ "crates/domain/src/generated/deep/table.rs": lines(5000) });
  assert.equal(generated.count, 1, "** crosses folders");
});

test("a change to .cargo/mutants.toml gets four shards", () => {
  const result = estimate({ ".cargo/mutants.toml": mutantsToml(["crates/domain/src/generated/**"]) });
  assert.equal(result.count, 4);
  assert.match(result.why, /mutants\.toml/);
});

test("a change to the gate's list gets four shards", () => {
  const result = estimate({ Justfile: justfile("--package domain --package web-ui --package other") });
  assert.equal(result.count, 4);
  assert.match(result.why, /Justfile/);
});

test("a change to the mutants recipe gets four shards", () => {
  const result = estimate({
    Justfile: justfile("--package domain --package web-ui").replace(
      "cargo mutants {{ mutants_packages }}\n\nmutants-all",
      "cargo mutants --jobs 2 {{ mutants_packages }}\n\nmutants-all",
    ),
  });
  assert.equal(result.count, 4);
});

test("a change to another recipe is not a change of the gate", () => {
  const result = estimate({
    Justfile: justfile("--package domain --package web-ui").replace("cargo fmt", "cargo fmt --all"),
  });
  assert.equal(result.count, 1);
});

test("a Justfile with no gate list gets four shards, not one", () => {
  const result = estimate({ "crates/domain/src/lib.rs": lines(3) }, { Justfile: "fmt:\n    cargo fmt\n" });
  // The base has no list either, so only the unreadable gate says four.
  assert.equal(result.count, 4);
  assert.match(result.why, /don.t match/);
});

test("a wrong number of arguments is refused", () => {
  const r = repo();
  try {
    const result = spawnSync("bash", [script, "main"], { encoding: "utf8" });
    assert.equal(result.status, 2);
  } finally {
    r.done();
  }
});

test("a change to mutants.toml that only adds a comment or a list entry is sized by its lines", () => {
  const base = mutantsToml(["crates/web/src/components/*.rs", "crates/domain/src/generated/**"]);
  const more = base.replace(
    '    "crates/web/src/components/*.rs",',
    '    # why\n    "crates/web/src/components/*.rs",\n    "crates/domain/src/old.rs",',
  );
  assert.notEqual(more, base);
  const result = estimate({ ".cargo/mutants.toml": more });
  assert.equal(result.count, 1, result.why);
  // The new entry is read: the file it names is left out of the lines.
  const excluded = estimate({ ".cargo/mutants.toml": more, "crates/domain/src/old.rs": lines(5000) });
  assert.equal(excluded.count, 1, excluded.why);
});

test("a change to mutants.toml that edits or removes a line gets four shards", () => {
  const base = mutantsToml(["crates/web/src/components/*.rs", "crates/domain/src/generated/**"]);
  assert.equal(estimate({ ".cargo/mutants.toml": base.replace('exclude_re = ["Display"]', 'exclude_re = ["Debug"]') }).count, 4);
  assert.equal(estimate({ ".cargo/mutants.toml": base.replace("# comment\n", "") }).count, 4);
});

test("a recipe with a blank line in its body is still watched", () => {
  const blank = (body: string) =>
    justfile("--package domain --package web-ui").replace(
      "mutants base='origin/main' *args:\n    cargo mutants {{ mutants_packages }}\n",
      `mutants base='origin/main' *args:\n    echo start\n\n    ${body}\n`,
    );
  const r = repo({ Justfile: blank("cargo mutants {{ mutants_packages }}") });
  try {
    assert.equal(r.estimate({ Justfile: blank("cargo mutants --jobs 2 {{ mutants_packages }}") }).count, 4);
  } finally {
    r.done();
  }
});

test("an exclude glob's other regex characters are literal", () => {
  const toml = mutantsToml(["crates/domain/src/a?.rs"]);
  const result = estimate({ "crates/domain/src/a.rs": lines(3000) }, { ".cargo/mutants.toml": toml });
  // `a.rs` is no `a?.rs`, so its 3000 lines are counted: four shards.
  assert.equal(result.count, 4, result.why);
});

test("a gated package that is no crate under crates/ gets four shards", () => {
  const result = estimate({ "crates/domain/src/lib.rs": lines(3) }, { Justfile: justfile("--package domain --package ladderback") });
  assert.equal(result.count, 4);
});

test("the branch's own lines count, not what main gained since it was cut", () => {
  const r = repo();
  try {
    r.onMain({ "crates/domain/src/lib.rs": lines(5000), Justfile: justfile("--package domain --package web-ui --package other") });
    const result = r.estimate({ "crates/domain/src/small.rs": lines(10) });
    assert.equal(result.count, 1, result.why);
  } finally {
    r.done();
  }
});

test("a renamed file counts as deleted and added", () => {
  const r = repo({ "crates/domain/src/old.rs": lines(400) });
  try {
    r.rename("crates/domain/src/old.rs", "crates/domain/src/new.rs");
    // 400 deleted and 400 added: 800 lines, two shards.
    assert.equal(r.estimate({}).count, 2);
  } finally {
    r.done();
  }
});

test("a file with a non-ASCII name counts", () => {
  const result = estimate({ "crates/domain/src/caf\u00e9.rs": lines(600) });
  assert.equal(result.count, 2, result.why);
});

test("a binary file has no lines", () => {
  const result = estimate({ "crates/domain/src/blob.rs": "\u0000\u0001".repeat(5000) });
  assert.equal(result.status, 0);
  assert.equal(result.count, 1);
  assert.equal(result.why, "0 changed lines in the gated crates' source, so 1 shard(s)", "no error on stderr");
});

test("a change to the mutants-all recipe gets four shards", () => {
  const result = estimate({
    Justfile: justfile("--package domain --package web-ui").replace(
      "mutants-all *args:\n    cargo mutants {{ mutants_packages }}",
      "mutants-all *args:\n    cargo mutants --jobs 2 {{ mutants_packages }}",
    ),
  });
  assert.equal(result.count, 4);
});

test("mutants.toml is compared with where the branch was cut, not with main's tip", () => {
  const r = repo();
  try {
    r.onMain({
      ".cargo/mutants.toml": mutantsToml([
        "crates/web/src/components/*.rs",
        "crates/domain/src/generated/**",
        "crates/domain/src/new.rs",
      ]),
    });
    assert.equal(r.estimate({ "crates/domain/src/lib.rs": lines(3) }).count, 1);
  } finally {
    r.done();
  }
});
