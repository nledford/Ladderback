#!/usr/bin/env bash
# Installs dist (cargo-dist), which builds the release archives (`just
# release-build`, technical design §12.1). taiki-e/install-action, which
# installs the other tools, doesn't list it, so this takes the official release
# tarball, checks it against the SHA-256 pinned here, and puts `dist` in the
# given folder (default ~/.cargo/bin). A dist of this version already on the
# PATH is left alone. The version is dist-workspace.toml's `cargo-dist-version`.
#
# To move to another release, change `version` here and in
# dist-workspace.toml, and the four hashes, taking each from the release's
# `cargo-dist-<target>.tar.xz.sha256` asset after checking it against the
# downloaded tarball.
set -euo pipefail

version=0.34.0
dest=${1:-"$HOME/.cargo/bin"}

have=$(dist --version 2>/dev/null | head -n 1 || true)
if [[ $have == "cargo-dist $version" ]]; then
  echo "dist $version is already installed"
  exit 0
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu sha=7fee44ff3ff71ed0064b32d4ec62fee6b58bd37c5887b12a86aedddc5af0c7b5 ;;
  Linux-aarch64) target=aarch64-unknown-linux-gnu sha=58efb33edf01342875bfd0c3ce2befa9d73075579ca1b3e5dd75b98c88f92e83 ;;
  Darwin-x86_64) target=x86_64-apple-darwin sha=6aa89bb1e7c18f51ae8b148ce2fc6ab00ac46ed8173d4f2ed9afa12101218773 ;;
  Darwin-arm64) target=aarch64-apple-darwin sha=d1eec30e63eed23e5a5bd48fe96eae314799811bffca254ec1d2264638ba1890 ;;
  *)
    echo "error: no pinned dist $version build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

tarball="cargo-dist-$target.tar.xz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --retry 3 \
  --output "$work/$tarball" \
  "https://github.com/axodotdev/cargo-dist/releases/download/v$version/$tarball"

if command -v sha256sum >/dev/null; then
  actual=$(sha256sum "$work/$tarball" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$tarball" | cut -d' ' -f1)
fi
if [[ $actual != "$sha" ]]; then
  echo "error: $tarball has SHA-256 $actual, expected $sha" >&2
  exit 1
fi

tar -xJf "$work/$tarball" -C "$work" "cargo-dist-$target/dist"
mkdir -p "$dest"
install -m 0755 "$work/cargo-dist-$target/dist" "$dest/dist"
echo "installed dist $version in $dest"
