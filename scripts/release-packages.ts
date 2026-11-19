// What the release smoke test (scripts/smoke-release.ts) needs to know about
// the release's .deb and .rpm packages (technical design §12.1, §16 decision
// 123): which format a container's system installs, the commands that install
// and remove one, and what the package may hold. Pure, so
// scripts/release-packages.test.ts checks it without a container.

export type PackageFormat = "deb" | "rpm";

/** The format a smoke-test container installs: Fedora's is the .rpm, every Debian and Ubuntu one the .deb. */
export function containerFormat(container: string): PackageFormat {
  return container === "fedora" ? "rpm" : "deb";
}

/** The format of a package file, by its extension; `undefined` for anything else. */
export function packageFormat(file: string): PackageFormat | undefined {
  if (file.endsWith(".deb")) return "deb";
  if (file.endsWith(".rpm")) return "rpm";
  return undefined;
}

/**
 * The version nfpm writes into the package for the release `version`
 * (`packaging/nfpm.yaml`): a pre-release's `-rc.1` becomes `~rc.1`, which sorts
 * before the release, and `+<commit>` of a build between releases stays. An
 * .rpm's release, `1`, follows a dash.
 */
export function packageVersion(version: string, format: PackageFormat): string {
  const written = version.replace("-", "~");
  return format === "rpm" ? `${written}-1` : written;
}

/** The command, in the container, that installs the package file there, offline: a local file needs no repository. */
export function installCommand(format: PackageFormat, file: string): string[] {
  return format === "deb"
    ? ["apt-get", "install", "--yes", "--no-install-recommends", file]
    : ["dnf", "install", "--assumeyes", "--disablerepo=*", file];
}

/** The command that removes the package. */
export function removeCommand(format: PackageFormat): string[] {
  return format === "deb"
    ? ["apt-get", "remove", "--yes", "ladderback"]
    : ["dnf", "remove", "--assumeyes", "--disablerepo=*", "ladderback"];
}

/** A shell command that prints the installed package's version, and fails when it isn't installed. */
export function installedVersionScript(format: PackageFormat): string {
  return format === "deb"
    ? // biome-ignore lint/suspicious/noTemplateCurlyInString: `${Version}` is dpkg-query's own format syntax, for the shell to pass on.
      "dpkg-query --show --showformat='${Version}' ladderback"
    : "rpm --query --queryformat='%{VERSION}-%{RELEASE}' ladderback";
}

/** A shell command that prints the installed package's files, one a line, no folders. */
export function installedFilesScript(format: PackageFormat): string {
  return format === "deb"
    ? 'dpkg --listfiles ladderback | while read -r path; do [ -d "$path" ] || echo "$path"; done'
    : "rpm --query --list ladderback";
}

/** A shell command that prints the package file's own description. */
export function describeScript(format: PackageFormat, file: string): string {
  return format === "deb" ? `dpkg-deb --info ${file}` : `rpm --query --info --package ${file}`;
}

/** A shell command that prints the package's maintainer scripts, nothing when it has none. */
export function scriptsScript(format: PackageFormat): string {
  return format === "deb"
    ? "ls /var/lib/dpkg/info | grep -E '^ladderback\\.(preinst|postinst|prerm|postrm|config)$' || true"
    : "rpm --query --scripts ladderback";
}

/**
 * A shell command that prints the state of the person's configuration and data
 * folders, under `$HOME` (decision 64's platform folders): each entry's type,
 * mode and size, and each file's SHA-256, sorted. Modification times are left
 * out: a package that touched nothing changes none, but they say nothing a
 * hash doesn't.
 */
export const FOLDER_STATE_SCRIPT =
  'cd "$HOME" && for folder in .config/ladderback .local/share/ladderback .cache/ladderback; do [ -e "$folder" ] || continue; find "$folder" -printf \'%y %m %s %p\\n\'; find "$folder" -type f -exec sha256sum {} +; done | sort';

/** A shell command that seeds those folders with what a person who used Ladderback has there. */
export const SEED_FOLDERS_SCRIPT =
  "cd \"$HOME\" && mkdir -p .config/ladderback .local/share/ladderback/repository .cache/ladderback && printf 'schema = 1\\n' > .config/ladderback/config.toml && printf 'a backup\\n' > .local/share/ladderback/repository/data && printf 'a cache\\n' > .cache/ladderback/cache";

// Every path a package may hold: the program and its licence texts, and the .rpm's own licence folder.
const ALLOWED = [
  /^\/usr\/bin\/ladderback$/,
  /^\/usr\/share\/licenses\/ladderback$/,
  /^\/usr\/share\/doc\/ladderback\/LICENSE-(?:MIT|APACHE)$/,
  /^\/usr\/share\/licenses\/ladderback\/LICENSE-(?:MIT|APACHE)$/,
];

/** The paths in `files` a package must not hold: anything but the program and its licences (so no service unit, no `/etc`). */
export function unexpectedFiles(files: string[]): string[] {
  return files.filter((file) => !ALLOWED.some((pattern) => pattern.test(file)));
}
