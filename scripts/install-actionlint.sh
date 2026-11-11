#!/usr/bin/env bash
# Installs actionlint, the GitHub Actions workflow linter `just lint-workflows`
# runs. taiki-e/install-action, which installs the other tools, doesn't list
# it, so this takes the official release tarball, checks it against the
# SHA-256 pinned here, and puts `actionlint` in the given folder (default
# ~/.cargo/bin). An actionlint of this version already on the PATH is left
# alone.
#
# To move to another release, change `version` and the four hashes, taking
# each from the release's `actionlint_<version>_checksums.txt` asset after
# checking it against the downloaded tarball.
set -euo pipefail

version=1.7.12
dest=${1:-"$HOME/.cargo/bin"}

have=$(actionlint --version 2>/dev/null | head -n 1 || true)
if [[ $have == "$version" ]]; then
  echo "actionlint $version is already installed"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=linux_amd64 sha=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 ;;
  Linux-aarch64) target=linux_arm64 sha=325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6 ;;
  Darwin-x86_64) target=darwin_amd64 sha=5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644 ;;
  Darwin-arm64) target=darwin_arm64 sha=aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f ;;
  *)
    echo "error: no pinned actionlint $version build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

tarball="actionlint_${version}_$target.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --retry 3 \
  --output "$work/$tarball" \
  "https://github.com/rhysd/actionlint/releases/download/v$version/$tarball"

if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$work/$tarball" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$tarball" | cut -d' ' -f1)
fi
if [[ $actual != "$sha" ]]; then
  echo "error: $tarball has SHA-256 $actual, expected $sha" >&2
  exit 1
fi

tar -xzf "$work/$tarball" -C "$work" actionlint
mkdir -p "$dest"
install -m 0755 "$work/actionlint" "$dest/actionlint"
echo "installed actionlint $version in $dest"
