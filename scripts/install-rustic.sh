#!/usr/bin/env bash
# Installs the rustic CLI that the profile golden tests run (technical design
# §6.2: an exported profile must read the same in the pinned rustic). It takes
# the official release tarball, checks it against the SHA-256 pinned here, and
# puts `rustic` in the given folder (default ~/.cargo/bin). A rustic of this
# version already on the PATH is left alone.
#
# To move to another release, change `version` and the four hashes, taking each
# hash from the release's own `<tarball>.sha256` asset after checking it.
set -euo pipefail

version=0.11.4
dest=${1:-"$HOME/.cargo/bin"}

# The release build says `rustic v0.11.4`, one built by cargo `rustic 0.11.4`.
have=$(rustic --version 2>/dev/null || true)
have=${have#rustic }
if [[ ${have#v} == "$version" ]]; then
  echo "rustic $version is already installed"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu sha=c20bc3682c6275de3cfbc9317101dca5c0800a2fb5c610d0f9ef2addfd27cf81 ;;
  Linux-aarch64) target=aarch64-unknown-linux-gnu sha=8d8121f9d72f5320e8a3dd870bce6cf56a4fb8cb4dadc4d29b4eeb228bd88406 ;;
  Darwin-x86_64) target=x86_64-apple-darwin sha=ff0014a53f10965bc7852945ad4ac52532ad8af0efe5e4f4957dd330889946de ;;
  Darwin-arm64) target=aarch64-apple-darwin sha=6df166f68876f1bd1e0965547542374877f04ab498cbb5e19b1b155844bce307 ;;
  *)
    echo "error: no pinned rustic $version build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

tarball="rustic-v$version-$target.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --retry 3 \
  --output "$work/$tarball" \
  "https://github.com/rustic-rs/rustic/releases/download/v$version/$tarball"

if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$work/$tarball" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$tarball" | cut -d' ' -f1)
fi
if [[ $actual != "$sha" ]]; then
  echo "error: $tarball has SHA-256 $actual, expected $sha" >&2
  exit 1
fi

tar -xzf "$work/$tarball" -C "$work" rustic
mkdir -p "$dest"
install -m 0755 "$work/rustic" "$dest/rustic"
echo "installed rustic $version in $dest"
