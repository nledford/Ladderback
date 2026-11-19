#!/usr/bin/env bash
# Installs shellcheck, which `just lint` runs on the scripts in scripts/ and
# actionlint runs on the shell in workflows when it finds it on the PATH. This
# takes the official release archive, checks it against the SHA-256 pinned
# here, and puts `shellcheck` in the given folder (default ~/.cargo/bin). If
# the PATH already holds this version, nothing is installed. (No comment line
# here may start with the tool's name: shellcheck reads that as a directive.)
#
# To move to another release, change `version` and the four hashes. The
# release publishes no checksums, so take each hash from its downloaded
# archive (`shasum -a 256`), and check that the archive's `shellcheck`
# reports the new version.
set -euo pipefail

version=0.11.0
dest=${1:-"$HOME/.cargo/bin"}

have=$(shellcheck --version 2>/dev/null | sed -n 's/^version: //p' || true)
if [[ $have == "$version" ]]; then
  echo "shellcheck $version is already installed"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=linux.x86_64 sha=8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198 ;;
  Linux-aarch64) target=linux.aarch64 sha=12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588 ;;
  Darwin-x86_64) target=darwin.x86_64 sha=3c89db4edcab7cf1c27bff178882e0f6f27f7afdf54e859fa041fca10febe4c6 ;;
  Darwin-arm64) target=darwin.aarch64 sha=56affdd8de5527894dca6dc3d7e0a99a873b0f004d7aabc30ae407d3f48b0a79 ;;
  *)
    echo "error: no pinned shellcheck $version build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

archive="shellcheck-v$version.$target.tar.xz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --retry 3 \
  --output "$work/$archive" \
  "https://github.com/koalaman/shellcheck/releases/download/v$version/$archive"

if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$work/$archive" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$archive" | cut -d' ' -f1)
fi
if [[ $actual != "$sha" ]]; then
  echo "error: $archive has SHA-256 $actual, expected $sha" >&2
  exit 1
fi

tar -xJf "$work/$archive" -C "$work" "shellcheck-v$version/shellcheck"
mkdir -p "$dest"
install -m 0755 "$work/shellcheck-v$version/shellcheck" "$dest/shellcheck"
echo "installed shellcheck $version in $dest"
