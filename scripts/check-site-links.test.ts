// Tests for the user docs site's link check (scripts/check-site-links.ts).
// `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { anchors, brokenLinks, links, type Repository } from "./check-site-links.ts";

const SITE = "docs/guide";

/** A repository of the given files and their text. */
function repository(files: Record<string, string>): Repository {
  return {
    read: (file) => files[file],
    exists: (file) => file in files || Object.keys(files).some((name) => name.startsWith(`${file}/`)),
    pages: (folder) =>
      Object.keys(files)
        .filter((name) => name.startsWith(`${folder}/`) && name.endsWith(".md"))
        .sort(),
  };
}

/** The problems in the site's pages, from a repository whose SUMMARY.md lists every page. */
function check(pages: Record<string, string>, others: Record<string, string> = {}): string[] {
  const summary = Object.keys(pages)
    .map((file) => `- [${file}](${file})`)
    .join("\n");
  const files: Record<string, string> = { [`${SITE}/SUMMARY.md`]: summary, ...others };
  for (const [name, text] of Object.entries(pages)) files[`${SITE}/${name}`] = text;
  return brokenLinks(SITE, repository(files));
}

test("a heading's anchor is GitHub's: lower case, punctuation dropped, spaces as hyphens", () => {
  assert.deepEqual(
    [...anchors("# Install\n\n## macOS: Full Disk Access\n\n### Moving a repository's password out of `config.toml`\n")],
    ["install", "macos-full-disk-access", "moving-a-repositorys-password-out-of-configtoml"],
  );
});

test("a repeated heading's anchor gets -1, -2 after the first", () => {
  assert.deepEqual([...anchors("## Updating\n\n## Updating\n\n## Updating\n")], ["updating", "updating-1", "updating-2"]);
});

test("a heading's link keeps only its text in the anchor", () => {
  assert.deepEqual([...anchors("## The [install script](install.md)\n")], ["the-install-script"]);
});

test("a heading inside fenced code has no anchor, and an <a id> has one", () => {
  assert.deepEqual([...anchors('```sh\n# not a heading\n```\n\n<a id="by-hand"></a>\n')], ["by-hand"]);
});

test("an anchor is mdBook's for emphasis, entities, an {#id} and a repeat of a made one", () => {
  assert.deepEqual(
    [...anchors('## _Emph_ heading\n\n## Foo &amp; bar\n\n## Foo {#my-id}\n\n<a name="top"></a>\n')],
    ["emph-heading", "foo--bar", "my-id", "top"],
  );
  assert.deepEqual([...anchors("## Dup\n\n## Dup\n\n## Dup 1\n")], ["dup", "dup-1", "dup-1-1"]);
});

test("links come from inline links, images and reference definitions, with their lines", () => {
  assert.deepEqual(links('See [Install](install.md#by-hand) and ![a shot](shot.png "title").\n\n[ref]: <secrets.md>\n'), [
    { line: 1, target: "install.md#by-hand" },
    { line: 1, target: "shot.png" },
    { line: 3, target: "secrets.md" },
  ]);
});

test("a link inside code is text, not a link", () => {
  assert.deepEqual(links("`[a](missing.md)`\n\n```md\n[b](missing.md)\n```\n"), []);
});

test("a code span closes only on a run of its own length, so a lone run of three leaves the link a link", () => {
  assert.deepEqual(links("A ``` b [x](gone.md) `c`\n"), [{ line: 1, target: "gone.md" }]);
});

test("an image inside a link, escaped brackets, autolinks and HTML give their targets too", () => {
  assert.deepEqual(
    links(
      '[![badge](badge.svg)](outer.md) [a \\] b](escaped.md) <https://example.org/x>\n<a href="page.md">p</a> <img src="shot.png"> <source srcset="dark.png 2x, light.png">\n',
    ),
    [
      { line: 1, target: "badge.svg" },
      { line: 1, target: "outer.md" },
      { line: 1, target: "escaped.md" },
      { line: 1, target: "https://example.org/x" },
      { line: 2, target: "page.md" },
      { line: 2, target: "shot.png" },
      { line: 2, target: "dark.png" },
      { line: 2, target: "light.png" },
    ],
  );
});

test("a footnote definition isn't a link", () => {
  assert.deepEqual(links("[^1]: a note\n"), []);
});

test("links to existing pages, anchors and repository files pass", () => {
  const found = check(
    {
      "README.md": "# Guide\n\n[Install](install.md), [by hand](install.md#by-hand), [top](#guide).\n",
      "install.md":
        "# Install\n\n## By hand\n\n[Cargo.toml](https://github.com/nledford/Ladderback/blob/main/Cargo.toml), [§12](https://github.com/nledford/Ladderback/blob/main/docs/design/operations.md#12-packaging-and-deployment), [rustic](https://rustic.cli.rs/docs/).\n",
    },
    { "Cargo.toml": "", "docs/design/operations.md": "## 12. Packaging and deployment\n" },
  );
  assert.deepEqual(found, []);
});

test("a link to a page that doesn't exist is broken", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\nSee [Install](instal.md).\n" }), [
    "docs/guide/README.md:3: instal.md: no such file in the site",
  ]);
});

test("a link to an anchor the page doesn't have is broken", () => {
  assert.deepEqual(
    check({
      "README.md": "# Guide\n\n[by hand](install.md#by-hands) and [here](#gide)\n",
      "install.md": "# Install\n\n## By hand\n",
    }),
    [
      "docs/guide/README.md:3: install.md#by-hands: install.md has no heading with that anchor",
      "docs/guide/README.md:3: #gide: README.md has no heading with that anchor",
    ],
  );
});

test("a relative link out of the site is broken, since the site doesn't publish its target", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\n[design](../design/README.md)\n" }, { "docs/design/README.md": "# Design\n" }), [
    "docs/guide/README.md:3: ../design/README.md: leaves the site; link to https://github.com/nledford/Ladderback/blob/main/docs/design/README.md",
  ]);
});

test("a link to a README.md is broken, since mdBook builds it as index.html but links it as README.html", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n", "install.md": "# Install\n\n[home](README.md) [home](./) [home](./#guide)\n" }), [
    "docs/guide/install.md:3: README.md: mdBook links it as README.html, which it doesn't build; link its folder (./) instead",
  ]);
});

test("a link to a folder with no README.md is broken, since the site has no page there", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\n[theme](theme/)\n" }, { "docs/guide/theme/site.css": "" }), [
    "docs/guide/README.md:3: theme/: a folder with no README.md, so no page",
  ]);
});

test("a link a percent sign can't decode is broken, not a crash", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\n[x](a%zz.md)\n" }), [
    "docs/guide/README.md:3: a%zz.md: malformed percent-encoding",
  ]);
});

test("a path from the site's root is broken under the site's /Ladderback/ prefix", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\n[x](/README.md)\n" }), [
    "docs/guide/README.md:3: /README.md: a path from the site's root breaks under its /Ladderback/ prefix; use a relative link",
  ]);
});

test("a link to the repository in any of GitHub's forms is checked, and one with a query or to a folder passes", () => {
  assert.deepEqual(
    check(
      {
        "README.md":
          "# Guide\n\n[a](http://www.github.com/NLEDFORD/ladderback/blob/HEAD/nope.md) [b](https://github.com/nledford/Ladderback/raw/main/gone.png) [c](https://github.com/nledford/Ladderback/tree/main/docs/design) [d](https://github.com/nledford/Ladderback/blob/main/Cargo.toml?plain=1)\n",
      },
      { "docs/design/README.md": "# Design\n", "Cargo.toml": "" },
    ),
    [
      "docs/guide/README.md:3: http://www.github.com/NLEDFORD/ladderback/blob/HEAD/nope.md: no such file in the repository",
      "docs/guide/README.md:3: https://github.com/nledford/Ladderback/raw/main/gone.png: no such file in the repository",
    ],
  );
});

test("a relative link out of the repository is broken", () => {
  assert.deepEqual(check({ "README.md": "# Guide\n\n[elsewhere](../../../outside.md)\n" }), [
    "docs/guide/README.md:3: ../../../outside.md: points outside the repository",
  ]);
});

test("a repository link to a file, or an anchor, that main doesn't have is broken", () => {
  assert.deepEqual(
    check(
      {
        "README.md":
          "# Guide\n\n[a](https://github.com/nledford/Ladderback/blob/main/docs/design/gone.md) [b](https://github.com/nledford/Ladderback/blob/main/docs/design/operations.md#13-nothing) [c](https://github.com/nledford/Ladderback/blob/main/../outside.md)\n",
      },
      { "docs/design/operations.md": "## 12. Packaging\n" },
    ),
    [
      "docs/guide/README.md:3: https://github.com/nledford/Ladderback/blob/main/docs/design/gone.md: no such file in the repository",
      "docs/guide/README.md:3: https://github.com/nledford/Ladderback/blob/main/docs/design/operations.md#13-nothing: docs/design/operations.md has no heading with that anchor",
      "docs/guide/README.md:3: https://github.com/nledford/Ladderback/blob/main/../outside.md: points outside the repository",
    ],
  );
});

test("a page the summary doesn't list isn't built, so a link to it is broken", () => {
  const files = {
    [`${SITE}/SUMMARY.md`]: "- [Guide](README.md)\n",
    [`${SITE}/README.md`]: "# Guide\n\n[Draft](draft.md)\n",
    [`${SITE}/draft.md`]: "# Draft\n",
  };
  assert.deepEqual(brokenLinks(SITE, repository(files)), [
    "docs/guide/README.md:3: draft.md: SUMMARY.md doesn't list it, so the site doesn't build it",
  ]);
});
