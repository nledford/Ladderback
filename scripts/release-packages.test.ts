// Tests for scripts/release-packages.ts, what the release smoke test knows
// about the .deb and .rpm packages. `just lint` runs them with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  containerFormat,
  installCommand,
  installedFilesScript,
  packageFormat,
  packageVersion,
  removeCommand,
  unexpectedFiles,
} from "./release-packages.ts";

test("Fedora installs the .rpm, every Debian and Ubuntu container the .deb", () => {
  assert.equal(containerFormat("fedora"), "rpm");
  for (const name of ["debian-12", "debian-13", "ubuntu-22.04", "ubuntu-24.04"]) assert.equal(containerFormat(name), "deb");
});

test("a package's format is its extension's", () => {
  assert.equal(packageFormat("target/distrib/ladderback_0.5.0_arm64.deb"), "deb");
  assert.equal(packageFormat("ladderback-0.5.0-1.x86_64.rpm"), "rpm");
  assert.equal(packageFormat("ladderback-x86_64-unknown-linux-gnu.tar.xz"), undefined);
  assert.equal(packageFormat("ladderback.deb.sha256"), undefined);
});

test("nfpm's version: a pre-release sorts before the release, a commit stays, an .rpm has its release", () => {
  assert.equal(packageVersion("0.5.0", "deb"), "0.5.0");
  assert.equal(packageVersion("0.5.0", "rpm"), "0.5.0-1");
  assert.equal(packageVersion("0.5.0-rc.1", "deb"), "0.5.0~rc.1");
  assert.equal(packageVersion("0.5.0-rc.1", "rpm"), "0.5.0~rc.1-1");
  assert.equal(packageVersion("0.4.0+3f937cf", "deb"), "0.4.0+3f937cf");
  assert.equal(packageVersion("0.4.0+3f937cf.dirty", "rpm"), "0.4.0+3f937cf.dirty-1");
});

test("a package is installed and removed offline, by the system's own tool", () => {
  assert.deepEqual(installCommand("deb", "/tmp/a.deb"), ["apt-get", "install", "--yes", "--no-install-recommends", "/tmp/a.deb"]);
  assert.deepEqual(installCommand("rpm", "/tmp/a.rpm"), ["dnf", "install", "--assumeyes", "--disablerepo=*", "/tmp/a.rpm"]);
  assert.deepEqual(removeCommand("deb"), ["apt-get", "remove", "--yes", "ladderback"]);
  assert.deepEqual(removeCommand("rpm"), ["dnf", "remove", "--assumeyes", "--disablerepo=*", "ladderback"]);
});

test("a package may hold the program and its licences only", () => {
  const files = [
    "/usr/bin/ladderback",
    "/usr/share/licenses/ladderback",
    "/usr/share/doc/ladderback/LICENSE-MIT",
    "/usr/share/doc/ladderback/LICENSE-APACHE",
    "/usr/share/licenses/ladderback/LICENSE-MIT",
    "/usr/share/licenses/ladderback/LICENSE-APACHE",
  ];
  assert.deepEqual(unexpectedFiles(files), []);
  const extra = [
    "/usr/lib/systemd/user/ladderback.service",
    "/etc/ladderback/config.toml",
    "/usr/bin/ladderback-helper",
    "/usr/share/doc/ladderback/LICENSE-MIT.bak",
  ];
  assert.deepEqual(unexpectedFiles([...files, ...extra]), extra);
});

test("the .deb's file list leaves out folders, the .rpm's has none", () => {
  assert.match(installedFilesScript("deb"), /\[ -d "\$path" \] \|\| echo/);
  assert.match(installedFilesScript("rpm"), /^rpm --query --list ladderback$/);
});
