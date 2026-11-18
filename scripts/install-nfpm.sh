#!/usr/bin/env bash
# Installs nfpm, which builds the release's .deb and .rpm packages from the
# binary the archive holds (`just package`, technical design §12.1, §16
# decision 123). taiki-e/install-action, which installs the other tools,
# doesn't list it, so this takes the official release tarball, checks it
# against the SHA-256 pinned here, and puts `nfpm` in the given folder (default
# ~/.cargo/bin). An nfpm of this version already on the PATH is left alone.
#
# To move to another release, change `version` and the four hashes, taking
# each from the release's `checksums.txt` asset after checking it against the
# downloaded tarball.
set -euo pipefail

version=2.49.0
dest=${1:-"$HOME/.cargo/bin"}

# `nfpm --version` prints a banner, with the version on its `GitVersion:` line.
have=$(nfpm --version 2>/dev/null | awk '$1 == "GitVersion:" { print $2; exit }' || true)
if [[ $have == "$version" ]]; then
  echo "nfpm $version is already installed"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=Linux_x86_64 sha=2afaa5ffb5e3493402e31e7c67a5a1fb42133e3f66fc315ca0051934125561c7 ;;
  Linux-aarch64) target=Linux_arm64 sha=d4eee71839e92c68cbd3b101640bad32d2856fc551acae0635be9118e48c9d9c ;;
  Darwin-x86_64) target=Darwin_x86_64 sha=182ba6c5ded85c87bf276dd80c0ecfd8a2f8b25f7db6ac5fb999557c5b4258ce ;;
  Darwin-arm64) target=Darwin_arm64 sha=27728af45782962ab59a8ff13ae041c7d955fe51dd94ab7883d9c5966a233f5d ;;
  *)
    echo "error: no pinned nfpm $version build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

tarball="nfpm_${version}_$target.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --retry 3 \
  --output "$work/$tarball" \
  "https://github.com/goreleaser/nfpm/releases/download/v$version/$tarball"

if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$work/$tarball" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$tarball" | cut -d' ' -f1)
fi
if [[ $actual != "$sha" ]]; then
  echo "error: $tarball has SHA-256 $actual, expected $sha" >&2
  exit 1
fi

tar -xzf "$work/$tarball" -C "$work" nfpm
mkdir -p "$dest"
install -m 0755 "$work/nfpm" "$dest/nfpm"
echo "installed nfpm $version in $dest"
