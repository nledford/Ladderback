// The parts of the dependency rule (technical design §3.2, AGENTS.md "Where
// code goes") that `cargo metadata --no-deps` can show without a build, for
// `scripts/check-architecture.sh`:
//
//   - the workspace graph: each crate depends only on the workspace crates its
//     row in `GRAPH` names. Every kind of dependency is checked: a normal or
//     build dependency must be in the row's `normal` list; a dev-dependency
//     there, in its `dev` list, or the crate itself (a crate's tests turning
//     on its own features). So adapters stay apart in their tests too;
//   - the owners of foreign models: only the crates `OWNERS` names depend on
//     a crate of each foreign model's family (`sqlx`, `sqlx-sqlite`…), as a
//     normal or build dependency, on any target and optional or not.
//     Dev-dependencies are exempt, as in the banned-crate checks: tests may
//     use a foreign crate as an oracle, and nothing ships;
//   - no `async` in `domain`: no `async fn`, block or closure in its `src`,
//     and nothing there (`#[path]`, `include!`, a symbolic link) that would
//     bring in code the scan doesn't read.
//
// Workspace dependencies are matched by path, foreign ones by package name,
// so a renamed one (`package =`) is caught either way; every target's and
// every optional dependency is listed. A feature can turn an optional
// dependency on, never add one.
//
// Usage: cargo metadata --no-deps --format-version 1 | node scripts/check-crate-graph.ts
// Prints an `ok:` line per rule, or an `error:` line per problem on standard
// error and exits 1.

import { type Dirent, readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseMetadata } from "./wasm-test-packages.ts";

/** One dependency as `cargo metadata` lists it. `path` is set for a path dependency only. */
export interface Dependency {
  name: string;
  kind: "dev" | "build" | null;
  target: string | null;
  path?: string | null;
}

/** The part of `cargo metadata`'s JSON this reads. */
export interface Metadata {
  workspace_root: string;
  workspace_members: string[];
  packages: { id: string; name: string; manifest_path: string; dependencies: Dependency[] }[];
}

/** The workspace crates one crate may depend on: `normal` for every kind, `dev` for dev-dependencies alone. */
export interface Row {
  normal: readonly string[];
  dev?: readonly string[];
}

/** The composition root's row: it wires everything, so it may depend on every workspace crate. */
export const ANY = "any";

/** An adapter's row: the core, the wire contract and spawn-gate, never another adapter; `extra` where §3.2 allows more. */
const adapter = (...extra: string[]): Row => ({ normal: ["app", "domain", "api", "spawn-gate", ...extra], dev: ["testkit"] });

/**
 * The allowed workspace graph (technical design §3.2): the two rings, the
 * crates outside them, the adapters and the composition root. A new crate
 * gets its row here, and its edges in §3.2, in the change that adds it.
 */
export const GRAPH: Readonly<Record<string, Row | typeof ANY>> = {
  domain: { normal: [] },
  // `api` since #79, for the codes and wire types the use cases return (§16, decision 106).
  app: { normal: ["domain", "api"], dev: ["testkit"] },
  api: { normal: [] },
  "spawn-gate": { normal: [] },
  testkit: { normal: ["spawn-gate"] },
  l10n: { normal: ["api"], dev: ["spawn-gate"] },
  web: { normal: ["api", "l10n"], dev: ["testkit"] },
  "config-adapter": adapter(),
  engine: adapter(),
  "http-adapter": adapter(),
  hooks: adapter(),
  interop: adapter(),
  mcp: adapter("l10n"),
  "notify-adapter": adapter("l10n"),
  platform: adapter(),
  "releases-adapter": adapter(),
  secrets: adapter(),
  "signing-adapter": adapter(),
  store: adapter(),
  "tls-adapter": adapter(),
  ladderback: ANY,
};

/**
 * A foreign model: the crates of its family, its own and those it is built
 * from (`sqlx`, `sqlx-core`, `sqlx-sqlite`…), so that depending on a part of
 * it is caught too, and the crates that may depend on them.
 */
export interface Model {
  name: string;
  crates: RegExp;
  owners: readonly string[];
}

/** The foreign models that stay behind one anti-corruption layer each (AGENTS.md "Where code goes"). */
export const OWNERS: readonly Model[] = [
  { name: "rustic", crates: /^rustic([-_]|$)/, owners: ["engine"] },
  { name: "sqlx", crates: /^sqlx([-_]|$)/, owners: ["store"] },
  { name: "toml", crates: /^toml([-_]|$)/, owners: ["config-adapter", "interop"] },
  { name: "keyring", crates: /^keyring([-_]|$)/, owners: ["secrets"] },
  { name: "dbus-secret-service", crates: /^dbus-secret-service([-_]|$)/, owners: ["secrets"] },
  { name: "rmcp", crates: /^rmcp([-_]|$)/, owners: ["mcp"] },
  { name: "axum", crates: /^axum([-_]|$)/, owners: ["http-adapter", "ladderback"] },
  // Ed25519 for the hub and its agents goes through the `Signing` port (§16, decision 120).
  { name: "aws-lc", crates: /^aws-lc([-_]|$)/, owners: ["signing-adapter"] },
  // The hub's built-in TLS, its certificate and the agent's pinned verifier (§16, decision 120).
  { name: "rustls", crates: /^(tokio-)?rustls([-_]|$)/, owners: ["tls-adapter"] },
  { name: "rcgen", crates: /^rcgen$/, owners: ["tls-adapter"] },
];

const own = <T>(record: Readonly<Record<string, T>>, key: string): T | undefined =>
  Object.hasOwn(record, key) ? record[key] : undefined;

/** How `dependency` is declared, for a message: its kind and its target, when not a plain one. */
function declared(dependency: Dependency): string {
  const kind = dependency.kind === null ? "" : ` (${dependency.kind}-dependency)`;
  const target = dependency.target === null ? "" : ` for ${dependency.target}`;
  return `${kind}${target}`;
}

function members(metadata: Metadata) {
  const ids = new Set(metadata.workspace_members);
  return metadata.packages.filter((pkg) => ids.has(pkg.id));
}

/** What is wrong with the tables themselves: a member with no row, or a row or owner that names no member. */
export function tableProblems(metadata: Metadata, graph = GRAPH, owners = OWNERS): string[] {
  const names = new Set(members(metadata).map((pkg) => pkg.name));
  const problems: string[] = [];
  for (const name of names) {
    if (own(graph, name) === undefined) {
      problems.push(`${name} has no row in the crate graph (scripts/check-crate-graph.ts): add it, with its edges in §3.2`);
    }
  }
  const named = [
    ...Object.entries(graph).flatMap(([crate, row]) => [crate, ...(row === ANY ? [] : [...row.normal, ...(row.dev ?? [])])]),
    ...owners.flatMap((model) => model.owners),
  ];
  for (const name of new Set(named)) {
    if (!names.has(name)) {
      problems.push(`the crate graph (scripts/check-crate-graph.ts) names ${name}, which isn't a workspace crate`);
    }
  }
  return problems;
}

/** Each edge between workspace crates that the crate's row in `graph` doesn't allow, named. */
export function graphProblems(metadata: Metadata, graph = GRAPH): string[] {
  const crates = members(metadata);
  const byDir = new Map(crates.map((pkg) => [path.resolve(path.dirname(pkg.manifest_path)), pkg.name]));
  const names = new Set(crates.map((pkg) => pkg.name));
  const problems: string[] = [];
  for (const pkg of crates) {
    const row = own(graph, pkg.name);
    if (row === undefined) continue; // `tableProblems` names it.
    for (const dependency of pkg.dependencies) {
      if (dependency.path === undefined || dependency.path === null) {
        if (names.has(dependency.name)) {
          problems.push(
            `${pkg.name} depends on ${dependency.name}${declared(dependency)} from outside the workspace, under a workspace crate's name`,
          );
        }
        continue;
      }
      const to = byDir.get(path.resolve(dependency.path));
      if (to === undefined) {
        problems.push(`${pkg.name} has a path dependency on ${dependency.name}${declared(dependency)}, which isn't a workspace crate`);
        continue;
      }
      if (row === ANY) continue;
      const allowed = dependency.kind === "dev" ? [...row.normal, ...(row.dev ?? []), pkg.name] : row.normal;
      if (!allowed.includes(to)) {
        problems.push(`${pkg.name} -> ${to}${declared(dependency)} isn't an edge the crate graph allows (§3.2)`);
      }
    }
  }
  return problems;
}

/** Each normal or build dependency on a crate of a foreign model in `owners` from a crate that doesn't own it, named. */
export function ownerProblems(metadata: Metadata, owners = OWNERS): string[] {
  const problems: string[] = [];
  for (const pkg of members(metadata)) {
    for (const dependency of pkg.dependencies) {
      if (dependency.kind === "dev") continue;
      const model = owners.find((candidate) => candidate.crates.test(dependency.name));
      if (model !== undefined && !model.owners.includes(pkg.name)) {
        problems.push(
          `${pkg.name} depends on ${dependency.name}${declared(dependency)}, which only ${model.owners.join(" and ")} may`,
        );
      }
    }
  }
  return problems;
}

const CHAR = /'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'/uy;
// A raw string's opening (`r"`, `br#"`, `cr##"`), where no identifier runs into it.
const RAW = /[bc]?r(#*)"/y;

/**
 * `source` with its comments, string and character literals blanked out:
 * every character but a line break becomes a space, so line numbers stay.
 */
export function codeOnly(source: string): string {
  let out = "";
  let i = 0;
  const blank = (end: number) => {
    out += source.slice(i, end).replace(/[^\n]/g, " ");
    i = end;
  };
  while (i < source.length) {
    const rest = source.slice(i, i + 2);
    if (rest === "//") {
      const end = source.indexOf("\n", i);
      blank(end < 0 ? source.length : end);
    } else if (rest === "/*") {
      let depth = 0;
      let j = i;
      while (j < source.length) {
        const pair = source.slice(j, j + 2);
        if (pair === "/*") {
          depth += 1;
          j += 2;
        } else if (pair === "*/") {
          depth -= 1;
          j += 2;
          if (depth === 0) break;
        } else {
          j += 1;
        }
      }
      blank(j);
    } else if (source[i] === '"') {
      let j = i + 1;
      while (j < source.length && source[j] !== '"') j += source[j] === "\\" ? 2 : 1;
      blank(Math.min(j + 1, source.length));
    } else if (source[i] === "'") {
      CHAR.lastIndex = i;
      const char = CHAR.exec(source);
      if (char) {
        blank(i + char[0].length);
      } else {
        out += "'"; // A lifetime or a label.
        i += 1;
      }
    } else {
      RAW.lastIndex = i;
      const raw = /[A-Za-z0-9_]/.test(source[i - 1] ?? "") ? null : RAW.exec(source);
      if (raw) {
        const hashes = raw[1] ?? "";
        const open = i + raw[0].length;
        const close = source.indexOf(`"${hashes}`, open);
        blank(close < 0 ? source.length : close + 1 + hashes.length);
      } else {
        out += source[i];
        i += 1;
      }
    }
  }
  return out;
}

/**
 * What in `source`, the text of `file` in `domain`, breaks "no async": each
 * line holding the `async` keyword, and each `#[path]` or `include!`, which
 * would bring in code from outside `src`, where this check doesn't look.
 * Comments and literals are skipped.
 */
export function domainProblems(file: string, source: string): string[] {
  return codeOnly(source)
    .split("\n")
    .flatMap((line, i) => [
      ...(/(?<!r#)\basync\b/.test(line) ? [`${file}:${i + 1}: async in domain, which has no async (§3.2)`] : []),
      ...(/#\s*\[\s*path\b|\binclude\s*!/.test(line)
        ? [`${file}:${i + 1}: #[path] or include! in domain, which the no-async check can't follow`]
        : []),
    ]);
}

/** The `.rs` files under `dir`, recursively and sorted, and the symbolic links there, which the scan doesn't follow. */
function rustFiles(dir: string): { files: string[]; links: string[] } {
  const entries = readdirSync(dir, { withFileTypes: true, recursive: true });
  const full = (entry: Dirent) => path.join(entry.parentPath, entry.name);
  return {
    files: entries
      .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
      .map(full)
      .sort(),
    links: entries
      .filter((entry) => entry.isSymbolicLink())
      .map(full)
      .sort(),
  };
}

// Run as a script, not imported (the real path: `import.meta.url` is one).
if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  const metadata = parseMetadata<Metadata>(readFileSync(0, "utf8"));
  const root = metadata.workspace_root;
  let failed = false;
  const report = (problems: string[], ok: string) => {
    for (const problem of problems) process.stderr.write(`error: ${problem}\n`);
    if (problems.length === 0) {
      process.stdout.write(`ok: ${ok}\n`);
    } else {
      failed = true;
    }
  };
  report(
    [...tableProblems(metadata), ...graphProblems(metadata)],
    "every edge between workspace crates is one the crate graph allows",
  );
  report(ownerProblems(metadata), `only their owners depend on the crates of ${OWNERS.map((model) => model.name).join(", ")}`);
  const domain = members(metadata).find((pkg) => pkg.name === "domain");
  if (domain === undefined) {
    report(["found no domain crate to check for async"], "");
  } else {
    const src = path.join(path.dirname(domain.manifest_path), "src");
    const { files, links } = rustFiles(src);
    report(
      [
        ...links.map((link) => `${path.relative(root, link)} is a symbolic link, which the no-async check doesn't follow`),
        ...files.flatMap((file) => domainProblems(path.relative(root, file), readFileSync(file, "utf8"))),
      ],
      `no async in ${path.relative(root, src)}`,
    );
  }
  process.exit(failed ? 1 : 0);
}
