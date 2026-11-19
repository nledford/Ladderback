// Tests for the crate graph, foreign-model owner and no-async checks
// (scripts/check-crate-graph.ts), on a stand-in `cargo metadata` with the
// workspace's crates and their real tables. `just lint` runs them with
// `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ANY,
  domainProblems,
  graphProblems,
  GRAPH,
  ownerProblems,
  tableProblems,
  type Dependency,
  type Metadata,
} from "./check-crate-graph.ts";

const root = "/w";
const dir = (name: string) => (name === "ladderback" ? root : `${root}/crates/${name}`);

type Edge = Partial<Dependency> & { name: string };

/** A dependency on the workspace crate `name`, by path, normal unless `edge` says otherwise. */
const on = (name: string, edge: Partial<Dependency> = {}): Edge => ({ path: dir(name), ...edge, name });
/** A dependency on a crate from a registry. */
const foreign = (name: string, edge: Partial<Dependency> = {}): Edge => ({ ...edge, name });

/** A workspace with every crate in `GRAPH`, depending on nothing but what `edges` gives each. */
function workspace(edges: Record<string, Edge[]> = {}, names = Object.keys(GRAPH)): Metadata {
  const packages = names.map((name) => ({
    id: `path+file://${dir(name)}#0.1.0`,
    name,
    manifest_path: `${dir(name)}/Cargo.toml`,
    dependencies: (edges[name] ?? []).map((edge) => ({ kind: null, target: null, ...edge })),
  }));
  return { workspace_root: root, workspace_members: packages.map((pkg) => pkg.id), packages };
}

test("today's edges are allowed", () => {
  const metadata = workspace({
    app: [on("domain"), on("api"), on("app", { kind: "dev" }), on("testkit", { kind: "dev" })],
    testkit: [on("spawn-gate")],
    l10n: [on("api"), on("spawn-gate", { kind: "dev", target: 'cfg(not(target_arch = "wasm32"))' })],
    web: [on("api"), on("l10n"), on("testkit", { kind: "dev" })],
    engine: [on("app"), on("domain"), on("api"), on("spawn-gate"), on("testkit", { kind: "dev" })],
    mcp: [on("api"), on("domain")],
    "notify-adapter": [on("app"), on("l10n")],
    ladderback: [on("store"), on("engine"), on("testkit", { kind: "dev" })],
  });
  assert.deepEqual([...tableProblems(metadata), ...graphProblems(metadata)], []);
});

test("an adapter depending on another adapter is named, whatever the kind or target", () => {
  const metadata = workspace({
    engine: [on("store")],
    hooks: [on("secrets", { kind: "build" })],
    interop: [on("config-adapter", { kind: "dev" })],
    platform: [on("store", { target: 'cfg(target_os = "linux")' })],
  });
  assert.deepEqual(graphProblems(metadata), [
    "engine -> store isn't an edge the crate graph allows (§3.2)",
    "hooks -> secrets (build-dependency) isn't an edge the crate graph allows (§3.2)",
    "interop -> config-adapter (dev-dependency) isn't an edge the crate graph allows (§3.2)",
    'platform -> store for cfg(target_os = "linux") isn\'t an edge the crate graph allows (§3.2)',
  ]);
});

test("the core reaching outward is named", () => {
  const metadata = workspace({ domain: [on("api")], app: [on("store"), on("l10n")] });
  assert.deepEqual(graphProblems(metadata), [
    "domain -> api isn't an edge the crate graph allows (§3.2)",
    "app -> store isn't an edge the crate graph allows (§3.2)",
    "app -> l10n isn't an edge the crate graph allows (§3.2)",
  ]);
});

test("a dev-only edge is allowed as a dev-dependency only", () => {
  const metadata = workspace({ app: [on("testkit")], l10n: [on("spawn-gate", { kind: "build" })] });
  assert.deepEqual(graphProblems(metadata), [
    "app -> testkit isn't an edge the crate graph allows (§3.2)",
    "l10n -> spawn-gate (build-dependency) isn't an edge the crate graph allows (§3.2)",
  ]);
});

test("a renamed workspace dependency is found by its path, not the name it's used under", () => {
  const metadata = workspace({ engine: [{ ...on("store"), name: "storage" }] });
  assert.deepEqual(graphProblems(metadata), ["engine -> store isn't an edge the crate graph allows (§3.2)"]);
});

test("a workspace crate's name from outside the workspace, or a path outside it, is refused", () => {
  const metadata = workspace({ engine: [foreign("store")], hooks: [{ name: "vendored", path: "/elsewhere/vendored" }] });
  assert.deepEqual(graphProblems(metadata), [
    "engine depends on store from outside the workspace, under a workspace crate's name",
    "hooks has a path dependency on vendored, which isn't a workspace crate",
  ]);
});

test("the composition root may depend on every workspace crate", () => {
  const all = Object.keys(GRAPH)
    .filter((name) => name !== "ladderback")
    .map((name) => on(name));
  assert.equal(GRAPH.ladderback, ANY);
  assert.deepEqual(graphProblems(workspace({ ladderback: all })), []);
});

test("a crate with no row, or a row naming no crate, is refused", () => {
  const names = [...Object.keys(GRAPH).filter((name) => name !== "hooks"), "sync"];
  assert.deepEqual(tableProblems(workspace({}, names)), [
    "sync has no row in the crate graph (scripts/check-crate-graph.ts): add it, with its edges in §3.2",
    "the crate graph (scripts/check-crate-graph.ts) names hooks, which isn't a workspace crate",
  ]);
});

test("only a foreign model's owners depend on it, on any target or to build", () => {
  const metadata = workspace({
    engine: [foreign("rustic_core"), foreign("rustic_backend"), foreign("sqlx")],
    store: [foreign("sqlx"), foreign("rustic_core", { kind: "build" })],
    secrets: [foreign("keyring", { target: 'cfg(target_os = "macos")' })],
    platform: [foreign("dbus-secret-service", { target: 'cfg(target_os = "linux")' })],
    hooks: [foreign("toml_edit"), foreign("rmcp")],
    "http-adapter": [foreign("axum")],
    ladderback: [foreign("axum")],
    mcp: [foreign("rmcp"), foreign("axum")],
    web: [foreign("toml")],
  });
  assert.deepEqual(ownerProblems(metadata), [
    "web depends on toml, which only config-adapter and interop may",
    "engine depends on sqlx, which only store may",
    "hooks depends on toml_edit, which only config-adapter and interop may",
    "hooks depends on rmcp, which only mcp may",
    "mcp depends on axum, which only http-adapter and ladderback may",
    'platform depends on dbus-secret-service for cfg(target_os = "linux"), which only secrets may',
    "store depends on rustic_core (build-dependency), which only engine may",
  ]);
});

test("a dev-dependency on a foreign model is allowed: tests may use it as an oracle", () => {
  const metadata = workspace({
    "config-adapter": [foreign("rustic_core", { kind: "dev" })],
    mcp: [foreign("axum", { kind: "dev" })],
    ladderback: [foreign("sqlx", { kind: "dev" })],
  });
  assert.deepEqual(ownerProblems(metadata), []);
});

test("a part of a foreign model counts as the model, and a crate that only starts alike doesn't", () => {
  const metadata = workspace({
    hooks: [foreign("sqlx-sqlite"), foreign("sqlx-core", { kind: "build" }), foreign("sqlxmq")],
    platform: [foreign("axum-core"), foreign("toml_datetime"), foreign("tomlkit")],
    secrets: [foreign("rustic_config"), foreign("keyring-core")],
  });
  assert.deepEqual(ownerProblems(metadata), [
    "hooks depends on sqlx-sqlite, which only store may",
    "hooks depends on sqlx-core (build-dependency), which only store may",
    "platform depends on axum-core, which only http-adapter and ladderback may",
    "platform depends on toml_datetime, which only config-adapter and interop may",
    "secrets depends on rustic_config, which only engine may",
  ]);
});

/** The lines of `source` that `domainProblems` names. */
const lines = (source: string) => domainProblems("lib.rs", source).map((problem) => Number(/^lib\.rs:(\d+):/.exec(problem)?.[1]));

test("async fn, blocks and closures are found, with their lines", () => {
  const source = [
    "pub async fn run() {}",
    "fn f() { let _ = async { 1 }; }",
    "fn g() { let _ = async move || 2; }",
    "impl X { pub(crate) async unsafe fn h(&self) {} }",
    "fn k<'a>(x: &'a str) -> impl Future + 'a { async move { x } }",
  ].join("\n");
  assert.deepEqual(lines(source), [1, 2, 3, 4, 5]);
  assert.equal(domainProblems("lib.rs", source)[0], "lib.rs:1: async in domain, which has no async (§3.2)");
});

test("async in comments, literals and longer names isn't code", () => {
  const source = [
    "//! No I/O, no async.",
    "/* outer /* async */ still a comment: async */",
    'const A: &str = "async fn";',
    'const B: &str = r#"say "async" here"#;',
    'const C: &[u8] = br"async";',
    "const D: char = '\"'; // a quote, then: async",
    "#[async_trait] trait T {}",
    "fn r#async() {}",
    "let async_mode = 1;",
    'let s = "escaped \\" async";',
  ].join("\n");
  assert.deepEqual(lines(source), []);
});

test("code after a char literal, a lifetime or a raw string is still read", () => {
  const source = ["let q = '\"'; async {}", "fn f<'a>(_: &'a ()) { async {} }", 'let s = r"x"; async {}'].join("\n");
  assert.deepEqual(lines(source), [1, 2, 3]);
});

test("#[path] and include! in domain are refused: they bring in code the scan doesn't read", () => {
  const source = [
    '#[path = "../extra/x.rs"]',
    "mod x;",
    'include!("../gen.rs");',
    '// include!("ignored")',
    'const S: &str = include_str!("a.txt");',
  ].join("\n");
  assert.deepEqual(domainProblems("lib.rs", source), [
    "lib.rs:1: #[path] or include! in domain, which the no-async check can't follow",
    "lib.rs:3: #[path] or include! in domain, which the no-async check can't follow",
  ]);
});
