# Install Ladderback

Ladderback runs on each computer it backs up. Each system has one recommended way to install it:

- **Debian 12 or newer, Ubuntu 22.04 or newer, 64-bit Raspberry Pi OS and Fedora:** the release's [`.deb` or `.rpm` package](#install-a-package-debian-ubuntu-fedora).
- **Any other 64-bit Linux:** the [install script](#the-install-script), which downloads the release and checks it.
- **macOS:** the [install script](#the-install-script), which builds the release on your Mac, [signed with your own certificate](#signing-your-build) so that macOS keeps its permissions from one update to the next.

Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported, and neither is Windows. A hub for several computers isn't in a release yet. Once Ladderback is installed and started, [Getting started](getting-started.md) takes you to a first backup in the web UI. [Other ways to install](other-ways-to-install.md), such as building from source, are in the Advanced part.

## Install a package (Debian, Ubuntu, Fedora)

From 0.5.0, each release has a `.deb` for Debian and Ubuntu and an `.rpm` for Fedora, for `x86_64` and `aarch64`, with their checksums in a `SHA256SUMS` file. Download the package for your computer, check it, and install it; `VERSION` is the release's, such as `0.5.0`:

```sh
VERSION=0.5.0
base="https://github.com/nledford/Ladderback/releases/download/v$VERSION"
# Debian, Ubuntu, Raspberry Pi OS (amd64 or arm64):
deb="ladderback_${VERSION}_$(dpkg --print-architecture).deb"
curl -fL -O "$base/$deb" -O "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS
sudo apt install "./$deb"
# Fedora (x86_64 or aarch64):
rpm="ladderback-${VERSION}-1.$(uname -m).rpm"
curl -fL -O "$base/$rpm" -O "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS
sudo dnf install "./$rpm"
```

`sha256sum` must print `…: OK`; anything else means the download is damaged or isn't the release's, so don't install it. The packages are not signed, so the checksum is the check, and `dnf` warns that it skipped its OpenPGP checks. The `./` matters to `apt`, which otherwise looks for a package of that name in its repositories. On 32-bit Raspberry Pi OS `dpkg --print-architecture` says `armhf`, the download fails with a 404, and there is no package for it. On Atomic Fedora (Bazzite) a package needs `rpm-ostree` layering: use the [install script](#the-install-script) there.

The package puts `ladderback` at `/usr/bin/ladderback` and starts nothing: [start it](#start-ladderback) next.

## The install script

On any other 64-bit Linux, one command downloads the newest release, checks it against the release's checksums, and installs `ladderback` into `~/.cargo/bin`, asking nothing:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | bash
```

On macOS there is no download yet, so the same script builds the newest release from source, which takes a while. It asks once before it installs the build tools it can install by itself. A C compiler, git and Node.js 26 or newer it never installs: it names the missing ones with the command that installs them, and stops. Make your certificate first, as [Signing your build](#signing-your-build) says, then name it in the command:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" bash
```

Without it the build is signed ad hoc, and macOS asks again for Full Disk Access and Photos after every update: until you grant Full Disk Access again, backups can't read the folders they protect. [The install script in detail](other-ways-to-install.md#the-install-script-in-detail) says how to read the script before it runs, what it checks, and its options.

When it finishes, the script says what to run next. If it says to open a new terminal, or that your PATH doesn't lead to `ladderback`, do that first, or the next command isn't found.

### Signing your build

On macOS, sign the build with a certificate from your own keychain, so that macOS sees every update as the same program and keeps its Full Disk Access and Photos permissions. No Apple Developer account is needed. Make the certificate once:

1. In Keychain Access, choose Keychain Access → Certificate Assistant → Create a Certificate. Give it a name, such as `Ladderback Code Signing`, set Identity Type to Self Signed Root and Certificate Type to Code Signing, and create it. It goes into your login keychain.
2. Open the new certificate, expand Trust, and set Code Signing to Always Trust.
3. Check that macOS lists it as valid: `security find-identity -v -p codesigning` names it.

Then name it in `LADDERBACK_SIGN_IDENTITY` each time you run the install script, as above. The first signing asks whether `codesign` may use the key: choose Always Allow, and later installs don't ask. [Running as a service](running-as-a-service.md#macos-signing-and-the-keychain) has what signing changes and what to do when it fails.

## Start Ladderback

Run this as yourself, not under `sudo`:

```sh
ladderback install-service
```

It runs Ladderback in the background from each login on, and starts it now. Open `http://127.0.0.1:9898` in your browser for the web UI, and go on with [Getting started](getting-started.md).

On Linux, a user's services stop when the user logs out: unless yours may already keep running, the command says to run `loginctl enable-linger`, which lets them. [Running as a service](running-as-a-service.md) has the details, and how to stop it.

## macOS: Full Disk Access

macOS lets a program read your Documents, Desktop, Photos and other protected folders only with your permission. Grant the installed `ladderback` Full Disk Access by hand, in System Settings → Privacy & Security → Full Disk Access, so that it can back them up: press **+**, then in the file dialog press ⌘⇧G and type `~/.cargo/bin/ladderback`, where the install script puts it. A [signed build](#signing-your-build) keeps the permission through updates; an unsigned one loses it with each update, so then grant it again after each one.

## Updating

Install the new release the way you installed the first, then run `ladderback install-service` again, which restarts Ladderback on the new release:

- **A package:** there is no apt or dnf repository yet, so download, check and install the next release's package as [above](#install-a-package-debian-ubuntu-fedora).
- **The install script:** run the same command again, on macOS with `LADDERBACK_SIGN_IDENTITY` again.

On macOS, a repository whose password is in the keychain needs one more answer after an update: run `ladderback check-keyring`, and when macOS asks whether `ladderback` may use the keychain item, choose Always Allow. Do it before the next scheduled backup, which otherwise fails while the question waits for an answer. [Running as a service](running-as-a-service.md#macos-signing-and-the-keychain) says why.

To remove a package, run `ladderback remove-service` first, which stops Ladderback and removes its service, then `sudo apt remove ladderback` or `sudo dnf remove ladderback`. Its configuration and data stay where they are, and so do your backups.

## By hand

Building Ladderback from source by hand, and the tools the build needs, is in the Advanced part: [Other ways to install, By hand](other-ways-to-install.md#by-hand).

## Docker image (not supported yet)

Each release after 0.5.0 publishes a Docker image to try, but it isn't yet a supported way to install and run Ladderback. [Other ways to install](other-ways-to-install.md#docker-image-not-supported-yet), in the Advanced part, says how to run it and what Ladderback in a container can't do.
