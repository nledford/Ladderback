#!/usr/bin/env node
// Signs the program `just install` installs on macOS with a code-signing
// identity the person names in LADDERBACK_SIGN_IDENTITY, usually a
// self-signed certificate in their login keychain (decision 71, #281). macOS
// ties Full Disk Access, the per-category privacy grants such as Photos and
// a keychain item's "Always Allow" to a program's signature. The linker's
// ad-hoc signature changes with every build, so each update is a new program
// to macOS; a certificate's signature under one fixed identifier stays the
// same, so the grants carry over (operations §12; tested on macOS 27.3,
// #281).
//
//   node scripts/install-signing.ts check
//     Before `just install` builds anything: when an identity is named, it
//     must be one `security find-identity -v -p codesigning` lists as valid,
//     by its exact name or by its SHA-1 hash, and only once; otherwise the
//     install stops here, before anything is built or installed.
//   node scripts/install-signing.ts sign [args for cargo install]...
//     After `cargo install`: `codesign --force --sign <hash> --identifier
//     io.github.nledford.ladderback` on the installed program, found as
//     `cargo install` with those arguments put it, then `codesign --verify`.
//     It signs with the identity's hash, which names exactly one identity,
//     where a name is also matched as part of a longer one. By then the new
//     program is installed, so a failure leaves it installed (and says how
//     it is signed); run `sign` again to sign it without building again.
//     It finds the program by `--root`, CARGO_INSTALL_ROOT and CARGO_HOME,
//     as install-restart does, not by cargo's `install.root` setting.
//
// Without LADDERBACK_SIGN_IDENTITY, or with it empty, and on every other
// operating system, both do nothing, and `just install` installs as it did
// before. Only the identity's name passes through: the key stays in the
// keychain, where `codesign` finds it, and the first signing asks whether
// `codesign` may use it ("Always Allow" answers that for later installs).
//
// Exit status: 0 when there is nothing to do or it is done; 1 when the
// identity can't sign or the signing fails; 2 on bad usage.

import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { homedir, platform } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

/** The variable that names the code-signing identity. */
export const IDENTITY_VARIABLE = "LADDERBACK_SIGN_IDENTITY";
/**
 * The identifier every signed build carries (decision 71): the launchd
 * agent's label too. Fixed, as `codesign` otherwise takes it from the file
 * name, and a renamed copy would then lose its grants.
 */
export const IDENTIFIER = "io.github.nledford.ladderback";
// How long `security` may take to list the identities.
const LIST_MS = 30_000;
// How to sign an installed program again, once the signing failed.
const RETRY = `To sign it without building again, run \`node scripts/install-signing.ts sign\` in the checkout with ${IDENTITY_VARIABLE} and the arguments given to \`just install\`.`;

type Env = NodeJS.ProcessEnv;

/** Why something can't be done. */
interface Refusal {
  why: string;
}

/** The identity named for signing: on macOS, the variable's value unless empty; elsewhere none. */
export function namedIdentity(env: Env, os: NodeJS.Platform): string | undefined {
  if (os !== "darwin") return undefined;
  return env[IDENTITY_VARIABLE] || undefined;
}

/** Cargo's bin folder for `cargo install` with `args`. */
export function binFolder(args: readonly string[], env: Env): string {
  for (let i = 0; i < args.length; i++) {
    const arg = args[i] ?? "";
    if (arg === "--root" && args[i + 1] !== undefined) return path.join(path.resolve(args[i + 1] ?? ""), "bin");
    if (arg.startsWith("--root=")) return path.join(path.resolve(arg.slice("--root=".length)), "bin");
  }
  const root = env.CARGO_INSTALL_ROOT || env.CARGO_HOME || path.join(homedir(), ".cargo");
  return path.join(root, "bin");
}

/** The first line of `text`, trimmed. */
function firstLine(text: string): string {
  return text.trim().split("\n")[0]?.trim() ?? "";
}

/** A valid identity as `security find-identity -v -p codesigning` lists it. */
interface Listed {
  hash: string;
  name: string;
}

/** The valid identities in `listing`, the output of `security find-identity -v -p codesigning`. */
export function listedIdentities(listing: string): Listed[] {
  const identities: Listed[] = [];
  for (const line of listing.split("\n")) {
    const listed = /^\s*\d+\)\s+([0-9A-Fa-f]{40})\s+"(.*)"\s*$/.exec(line);
    if (listed?.[1] !== undefined && listed[2] !== undefined) identities.push({ hash: listed[1], name: listed[2] });
  }
  return identities;
}

/** The SHA-1 hash of the one valid code-signing identity `identity` names, exactly or by its hash, or why there is none. */
export function identityHash(identity: string, env: Env): string | Refusal {
  const listed = spawnSync("security", ["find-identity", "-v", "-p", "codesigning"], { encoding: "utf8", env, timeout: LIST_MS });
  if (listed.error || listed.status !== 0) {
    const said = listed.error?.message ?? (firstLine(listed.stderr) || `exit status ${listed.status}`);
    return { why: `couldn't list the code-signing identities with \`security find-identity -v -p codesigning\`: ${said}` };
  }
  const identities = listedIdentities(listed.stdout);
  const named = identities.filter((i) => i.name === identity || i.hash.toUpperCase() === identity.toUpperCase());
  const [first] = named;
  if (first === undefined) {
    const longer = identities.filter((i) => i.name.includes(identity)).map((i) => `"${i.name}"`);
    if (longer.length > 0) {
      return {
        why: `${IDENTITY_VARIABLE} names "${identity}", which is no valid code-signing identity's whole name; \`security find-identity -v -p codesigning\` lists ${longer.join(", ")}: name one in full, or by its SHA-1 hash`,
      };
    }
    return {
      why:
        `${IDENTITY_VARIABLE} names "${identity}", which \`security find-identity -v -p codesigning\` doesn't list as a valid code-signing identity. ` +
        "Create one with Keychain Access → Certificate Assistant → Create a Certificate (Identity Type: Self Signed Root, Certificate Type: Code Signing), " +
        "then set the certificate's Trust → Code Signing to Always Trust; or unset the variable to install without signing (docs/guide/install.md)",
    };
  }
  if (named.length > 1) {
    const hashes = new Set(named.map((i) => i.hash.toUpperCase()));
    if (hashes.size === 1) {
      return {
        why: `${IDENTITY_VARIABLE} names "${identity}", a certificate that \`security find-identity -v -p codesigning\` lists ${named.length} times, once for each keychain that holds a copy: delete all but one copy in Keychain Access`,
      };
    }
    return {
      why: `${IDENTITY_VARIABLE} names "${identity}", which names ${hashes.size} valid code-signing identities; name one by the SHA-1 hash \`security find-identity -v -p codesigning\` lists before it`,
    };
  }
  return first.hash;
}

/** Signs `program` with the identity whose hash is `hash`, under the fixed identifier, and verifies it; why it couldn't. */
export function signProgram(program: string, hash: string, env: Env): Refusal | undefined {
  const signed = spawnSync("codesign", ["--force", "--sign", hash, "--identifier", IDENTIFIER, program], { encoding: "utf8", env });
  if (signed.error || signed.status !== 0) {
    const said = signed.error?.message ?? (firstLine(signed.stderr) || `exit status ${signed.status}`);
    return {
      why: `couldn't sign ${program}: ${said}. A locked keychain, or a prompt to use the key that nobody can answer (over SSH), stops \`codesign\`. The program is installed with the linker's ad-hoc signature`,
    };
  }
  const verified = spawnSync("codesign", ["--verify", program], { encoding: "utf8", env });
  if (verified.error || verified.status !== 0) {
    const said = verified.error?.message ?? (firstLine(verified.stderr) || `exit status ${verified.status}`);
    return {
      why: `signed ${program}, but \`codesign --verify\` refuses it: ${said}. The program is installed with that signature, which macOS may refuse to run`,
    };
  }
  return undefined;
}

/**
 * What `just install-restart` says on `os` after a restart about the
 * permissions macOS may ask for again. On macOS without an identity, that the
 * keychain, Photos and Full Disk Access ask again; with one, that they carry
 * over (tested on a Mac, #281). Elsewhere nothing.
 */
export function reinstallWarning(env: Env, os: NodeJS.Platform): string | undefined {
  if (os !== "darwin") return undefined;
  const identity = namedIdentity(env, os);
  if (identity === undefined) {
    return (
      "macOS will ask again for each keychain item Ladderback reads, and Photos and Full Disk Access may need granting again: " +
      `a local build is signed ad hoc, so macOS sees a new program (#281). ${IDENTITY_VARIABLE} names a code-signing identity to sign it with instead (docs/guide/install.md).`
    );
  }
  return `the program is signed with "${identity}", so macOS keeps the Full Disk Access and Photos grants it gave the last build signed with it (#281); the keychain still asks once for this build before it lets it read a keyring item (#892).`;
}

/** Runs `check` or `sign [args for cargo install]` for `os`; its exit status. */
export function main(argv: readonly string[], env: Env, os: NodeJS.Platform): number {
  const [command, ...args] = argv;
  if (command !== "check" && command !== "sign") {
    console.error("usage: node scripts/install-signing.ts check | sign [args for cargo install]...");
    return 2;
  }
  const identity = namedIdentity(env, os);
  if (identity === undefined) return 0;
  const hash = identityHash(identity, env);
  if (typeof hash !== "string") {
    console.error(
      `install: ${hash.why}. ${command === "check" ? "Nothing was built or installed." : `The program is installed with the linker's ad-hoc signature. ${RETRY}`}`,
    );
    return 1;
  }
  if (command === "check") return 0;
  const program = path.join(binFolder(args, env), "ladderback");
  const refused = signProgram(program, hash, env);
  if (refused !== undefined) {
    console.error(`install: ${refused.why}. ${RETRY}`);
    return 1;
  }
  console.log(`install: signed ${program} with "${identity}" as ${IDENTIFIER}.`);
  return 0;
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2), process.env, platform());
}
