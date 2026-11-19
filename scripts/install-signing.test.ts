// Tests for scripts/install-signing.ts, which `just install` runs before it
// builds (`check`) and after `cargo install` (`sign`). Stand-ins for
// `security` and `codesign` first on the PATH serve a list of identities and
// record each call, so the tests sign nothing and read no keychain, on Linux
// as on macOS: the operating system is an argument. `just lint` runs them
// with `node --test`.

import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { binFolder, IDENTIFIER, IDENTITY_VARIABLE, main, reinstallWarning } from "./install-signing.ts";

const HASH = "0123456789ABCDEF0123456789ABCDEF01234567";
const OTHER = "89ABCDEF0123456789ABCDEF0123456789ABCDEF";
const NAME = "Ladderback Code Signing";
// As `security find-identity -v -p codesigning` lists valid identities; the
// second's name holds the first's.
const LISTING = `  1) ${HASH} "${NAME}"\n  2) ${OTHER} "${NAME} 2"\n     2 valid identities found\n`;

interface Stand {
  /** What `security` lists. */
  listing?: string;
  /** Which call fails: the listing, the signing or the verifying. */
  failing?: "security" | "sign" | "verify";
}

// Runs the script's `argv` on `os`, with `identity` in the variable (none
// when undefined) and `cargo install`'s root in a scratch folder; the exit
// status, what it said, and the calls of the stand-ins.
function run(argv: string[], os: NodeJS.Platform, identity: string | undefined, stand: Stand = {}) {
  const dir = mkdtempSync(join(tmpdir(), "install-signing-"));
  try {
    const log = join(dir, "log");
    const failing = stand.failing ?? "";
    writeFileSync(
      join(dir, "security"),
      `#!/usr/bin/env bash
echo "security $*" >> "${log}"
[ "${failing}" = security ] && { echo "security: SecKeychainSearchCopyNext: the specified keychain could not be found" >&2; exit 1; }
cat "${join(dir, "listing")}"
`,
    );
    writeFileSync(
      join(dir, "codesign"),
      `#!/usr/bin/env bash
echo "codesign $*" >> "${log}"
case "$1" in
  --force) [ "${failing}" = sign ] && { echo "\${@: -1}: errSecInternalComponent" >&2; exit 1; } ;;
  --verify) [ "${failing}" = verify ] && { echo "\${@: -1}: invalid signature (code or signature have been modified)" >&2; exit 3; } ;;
esac
exit 0
`,
    );
    chmodSync(join(dir, "security"), 0o755);
    chmodSync(join(dir, "codesign"), 0o755);
    writeFileSync(join(dir, "listing"), stand.listing ?? LISTING);
    const env: NodeJS.ProcessEnv = { PATH: `${dir}:${process.env["PATH"]}`, CARGO_HOME: join(dir, "cargo") };
    if (identity !== undefined) env[IDENTITY_VARIABLE] = identity;
    const said: string[] = [];
    const record = (...parts: unknown[]) => void said.push(parts.join(" "));
    const original = { log: console.log, error: console.error };
    console.log = record;
    console.error = record;
    let status: number;
    try {
      status = main(
        argv.map((arg) => arg.replaceAll("<dir>", dir)),
        env,
        os,
      );
    } finally {
      console.log = original.log;
      console.error = original.error;
    }
    const calls = existsSync(log)
      ? readFileSync(log, "utf8")
          .split("\n")
          .filter(Boolean)
          .map((c) => c.replaceAll(dir, "<dir>"))
      : [];
    return { status, said: said.join("\n").replaceAll(dir, "<dir>"), calls };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const LIST = "security find-identity -v -p codesigning";

test("elsewhere than macOS, a named identity signs nothing", () => {
  for (const command of ["check", "sign"]) {
    const { status, said, calls } = run([command], "linux", NAME);
    assert.equal(status, 0);
    assert.equal(said, "");
    assert.deepEqual(calls, []);
  }
});

test("on macOS without an identity, or with an empty one, nothing is signed", () => {
  for (const identity of [undefined, ""]) {
    for (const command of ["check", "sign"]) {
      const { status, said, calls } = run([command], "darwin", identity);
      assert.equal(status, 0);
      assert.equal(said, "");
      assert.deepEqual(calls, []);
    }
  }
});

test("check passes an identity listed by its name and signs nothing", () => {
  const { status, said, calls } = run(["check"], "darwin", NAME);
  assert.equal(status, 0);
  assert.equal(said, "");
  assert.deepEqual(calls, [LIST]);
});

test("sign signs the installed program with the identity's hash under the fixed identifier, then verifies it", () => {
  const { status, said, calls } = run(["sign"], "darwin", NAME);
  assert.equal(status, 0);
  assert.deepEqual(calls, [
    LIST,
    `codesign --force --sign ${HASH} --identifier io.github.nledford.ladderback <dir>/cargo/bin/ladderback`,
    "codesign --verify <dir>/cargo/bin/ladderback",
  ]);
  assert.equal(said, `install: signed <dir>/cargo/bin/ladderback with "${NAME}" as io.github.nledford.ladderback.`);
  assert.equal(IDENTIFIER, "io.github.nledford.ladderback");
});

test("sign finds the program where cargo install's --root put it", () => {
  for (const root of [["--root", "<dir>/elsewhere"], ["--root=<dir>/elsewhere"]]) {
    const { status, calls } = run(["sign", "--locked", ...root], "darwin", NAME);
    assert.equal(status, 0);
    assert.deepEqual(calls.slice(1), [
      `codesign --force --sign ${HASH} --identifier ${IDENTIFIER} <dir>/elsewhere/bin/ladderback`,
      "codesign --verify <dir>/elsewhere/bin/ladderback",
    ]);
  }
});

test("an identity named by its hash, in either case, signs with that identity", () => {
  for (const identity of [OTHER, OTHER.toLowerCase()]) {
    const { status, calls } = run(["sign"], "darwin", identity);
    assert.equal(status, 0);
    assert.equal(calls[1], `codesign --force --sign ${OTHER} --identifier ${IDENTIFIER} <dir>/cargo/bin/ladderback`);
  }
});

test("a name security doesn't list as valid stops the check, saying how to make one", () => {
  const { status, said, calls } = run(["check"], "darwin", "Someone Else");
  assert.equal(status, 1);
  assert.match(
    said,
    new RegExp(
      `^install: ${IDENTITY_VARIABLE} names "Someone Else", which \`security find-identity -v -p codesigning\` doesn't list as a valid code-signing identity`,
    ),
  );
  assert.match(said, /Certificate Assistant → Create a Certificate/);
  assert.match(said, /Trust → Code Signing to Always Trust/);
  assert.match(said, /Nothing was built or installed\.$/);
  assert.deepEqual(calls, [LIST]);
});

test("part of a listed name is refused, naming the names that hold it", () => {
  // `codesign` would take it as a name; the check takes only a whole one.
  const { status, said, calls } = run(["check"], "darwin", "Ladderback");
  assert.equal(status, 1);
  assert.match(
    said,
    new RegExp(
      `names "Ladderback", which is no valid code-signing identity's whole name; .* lists "${NAME}", "${NAME} 2": name one in full, or by its SHA-1 hash`,
    ),
  );
  assert.match(said, /Nothing was built or installed\.$/);
  assert.deepEqual(calls, [LIST]);
});

test("a name security doesn't list after the install leaves the program signed ad hoc and says how to sign it again", () => {
  const { status, said, calls } = run(["sign"], "darwin", "Someone Else");
  assert.equal(status, 1);
  assert.match(said, /doesn't list as a valid code-signing identity/);
  assert.match(
    said,
    /The program is installed with the linker's ad-hoc signature\. To sign it without building again, run `node scripts\/install-signing\.ts sign`/,
  );
  assert.deepEqual(calls, [LIST]);
});

test("a name two valid identities share is refused, naming the hash as the fix", () => {
  const listing = `  1) ${HASH} "${NAME}"\n  2) ${OTHER} "${NAME}"\n     2 valid identities found\n`;
  const { status, said, calls } = run(["check"], "darwin", NAME, { listing });
  assert.equal(status, 1);
  assert.match(said, /which names 2 valid code-signing identities; name one by the SHA-1 hash/);
  assert.deepEqual(calls, [LIST]);
});

test("a certificate listed once for each of two keychains is refused, by name or by hash, naming the copies as the fix", () => {
  const listing = `  1) ${HASH} "${NAME}"\n  2) ${HASH} "${NAME}"\n     2 valid identities found\n`;
  for (const identity of [NAME, HASH]) {
    const { status, said } = run(["check"], "darwin", identity, { listing });
    assert.equal(status, 1);
    assert.match(said, /lists 2 times, once for each keychain that holds a copy: delete all but one copy in Keychain Access/);
  }
});

test("a listing security can't give stops the check", () => {
  const { status, said } = run(["check"], "darwin", NAME, { failing: "security" });
  assert.equal(status, 1);
  assert.match(
    said,
    /couldn't list the code-signing identities with `security find-identity -v -p codesigning`: security: SecKeychainSearchCopyNext/,
  );
  assert.match(said, /Nothing was built or installed\.$/);
});

test("a signing codesign refuses fails the install, unverified, with the ad-hoc signature left", () => {
  const { status, said, calls } = run(["sign"], "darwin", NAME, { failing: "sign" });
  assert.equal(status, 1);
  assert.match(
    said,
    /^install: couldn't sign <dir>\/cargo\/bin\/ladderback: <dir>\/cargo\/bin\/ladderback: errSecInternalComponent\./,
  );
  assert.match(said, /The program is installed with the linker's ad-hoc signature\. To sign it without building again/);
  assert.equal(calls.length, 2);
});

test("a signature codesign --verify refuses fails the install, saying the program carries it", () => {
  const { status, said, calls } = run(["sign"], "darwin", NAME, { failing: "verify" });
  assert.equal(status, 1);
  assert.match(
    said,
    /signed <dir>\/cargo\/bin\/ladderback, but `codesign --verify` refuses it: .*invalid signature.*\. The program is installed with that signature, which macOS may refuse to run\. To sign it without building again/,
  );
  assert.doesNotMatch(said, /ad-hoc/);
  assert.equal(calls.length, 3);
});

test("no command, or another, is bad usage", () => {
  for (const argv of [[], ["install"]]) assert.equal(run(argv, "darwin", NAME).status, 2);
});

test("install-restart warns of the keychain, Photos and Full Disk Access for an unsigned build on macOS", () => {
  const warning = reinstallWarning({}, "darwin");
  assert.match(warning ?? "", /keychain item/);
  assert.match(warning ?? "", /Photos and Full Disk Access may need granting again/);
  assert.match(warning ?? "", new RegExp(IDENTITY_VARIABLE));
  assert.equal(reinstallWarning({ [IDENTITY_VARIABLE]: "" }, "darwin"), warning);
  assert.equal(reinstallWarning({}, "linux"), undefined);
  assert.equal(reinstallWarning({ [IDENTITY_VARIABLE]: NAME }, "linux"), undefined);
});

test("install-restart says a signed build keeps its grants, as tested on a Mac", () => {
  const note = reinstallWarning({ [IDENTITY_VARIABLE]: NAME }, "darwin") ?? "";
  assert.match(note, new RegExp(`signed with "${NAME}", so macOS keeps the Full Disk Access and Photos grants`));
  assert.match(note, /the keychain still asks once for this build/);
  assert.doesNotMatch(note, /still being confirmed/);
  assert.doesNotMatch(note, /may need granting again|may still ask/);
});

test("cargo install's bin folder: CARGO_INSTALL_ROOT, then CARGO_HOME, then ~/.cargo", () => {
  assert.equal(binFolder([], { CARGO_INSTALL_ROOT: "/a", CARGO_HOME: "/b" }), "/a/bin");
  assert.equal(binFolder([], { CARGO_HOME: "/b" }), "/b/bin");
  assert.equal(binFolder([], {}), join(homedir(), ".cargo", "bin"));
  assert.equal(binFolder(["--root", "/c"], { CARGO_INSTALL_ROOT: "/a" }), "/c/bin");
});
