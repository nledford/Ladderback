# Install on Debian, Ubuntu, Raspberry Pi OS or Fedora

On Debian 12 or newer, Ubuntu 22.04 or newer, 64-bit Raspberry Pi OS and Fedora, install the release's `.deb` or `.rpm` package: your package manager then knows the program, and removes it with its usual command.

## Download, check and install

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

`sha256sum` must print `…: OK`; anything else, such as `FAILED`, means the download is damaged or isn't the release's, so don't install it. The packages are not signed, so the checksum is the check, and `dnf` warns that it skipped its OpenPGP checks. The `./` matters to `apt`, which otherwise looks for a package of that name in its repositories. On 32-bit Raspberry Pi OS `dpkg --print-architecture` says `armhf`, the download fails with a 404, and there is no package for it. On Atomic Fedora (Bazzite) a package needs `rpm-ostree` layering: use the [install script](install-linux.md) there.

The package puts `ladderback` at `/usr/bin/ladderback` and starts nothing: [start Ladderback](install.md#start-ladderback) next. If Ladderback is already installed another way, [move that install to the package](other-ways-to-install.md#moving-an-existing-install-to-the-package) instead.

## Updating

There is no apt or dnf repository yet, so `apt upgrade` and `dnf upgrade` don't see a new release. When the web UI's header says a new version is available, or the [releases page](https://github.com/nledford/Ladderback/releases) lists one, run the commands above again with the new `VERSION`: the package manager replaces the version you have. Then run `ladderback install-service` again, which restarts Ladderback on the new release.

## Removing

Stop Ladderback and remove its service first, as yourself, since removing the package doesn't stop a service you installed for your own user. Then remove the package:

```sh
ladderback remove-service
# Debian, Ubuntu, Raspberry Pi OS:
sudo apt remove ladderback
# Fedora:
sudo dnf remove ladderback
```

Your settings, history and backups stay where they are, and so does anything in your home folder, which no package touches: [What removing leaves behind](install.md#what-removing-leaves-behind) lists it, and how to delete it too.
