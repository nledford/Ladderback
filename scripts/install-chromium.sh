#!/usr/bin/env bash
# Installs Playwright's Chromium for the UI journeys on a Linux runner, with the
# system packages it needs (.github/actions/chromium runs it).
#
# The packages come from the .deb files in $HOME/.cache/playwright-debs when the
# action's cache restored some, so the install needs no Ubuntu mirror. Without
# them it installs as `playwright install --with-deps` does, through apt, with
# apt told to download into that folder, and writes `debs=saved` to
# $GITHUB_OUTPUT so the action saves what it downloaded. If the cached files
# don't install, it falls back to apt.
set -euo pipefail
shopt -s nullglob

debs="$HOME/.cache/playwright-debs"
cd "$(dirname "$0")/../ui-tests"

cached=("$debs"/*.deb)
if [ ${#cached[@]} -gt 0 ]; then
    echo "install-chromium: ${#cached[@]} system packages from the cache"
    if sudo dpkg --install --skip-same-version "${cached[@]}"; then
        npx playwright install chromium
        exit 0
    fi
    echo "install-chromium: the cached packages didn't install; using apt" >&2
fi

# apt's own archive folder won't do: some images (Debian's Docker image) delete
# its files after every install. This setting lasts for this install only.
conf=/etc/apt/apt.conf.d/99-ladderback-chromium-debs
trap 'sudo rm -f "$conf"' EXIT
sudo rm -rf "$debs"
mkdir -p "$debs/partial"
echo "Dir::Cache::Archives \"$debs/\";" | sudo tee "$conf" > /dev/null
npx playwright install --with-deps chromium
sudo rm -f "$conf"

# apt downloads as root; the cache reads them as the runner's user.
sudo chown -R "$(id -u):$(id -g)" "$debs"
rm -rf "$debs/partial" "$debs/lock"
downloaded=("$debs"/*.deb)
echo "install-chromium: keeping ${#downloaded[@]} system packages for the cache"
if [ ${#downloaded[@]} -gt 0 ]; then
    echo "debs=saved" >> "${GITHUB_OUTPUT:-/dev/null}"
fi
