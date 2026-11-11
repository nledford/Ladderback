#!/usr/bin/env bash
# Installs Playwright's Chromium for the UI journeys on a Linux runner, with the
# system packages it needs. .github/actions/chromium runs it in ui-tests, the
# folder whose `npm ci` installed Playwright.
#
# $HOME/.cache/playwright-debs holds the .deb files a past install downloaded
# and a `complete` marker; the action's cache restores it. With the marker, the
# files install with dpkg (there may be none: the image had every package) and
# the install needs no Ubuntu mirror. Without it, or if they don't install, it
# installs as `playwright install --with-deps` does, through apt, with apt told
# to download into that folder, and writes `debs=saved` to $GITHUB_OUTPUT so
# the action saves the folder.
set -euo pipefail
shopt -s nullglob

debs="$HOME/.cache/playwright-debs"
fallback=false

if [ -e "$debs/complete" ]; then
    cached=("$debs"/*.deb)
    echo "install-chromium: ${#cached[@]} system packages from the cache"
    if [ ${#cached[@]} -eq 0 ] || sudo dpkg --install --skip-same-version "${cached[@]}"; then
        npx playwright install chromium
        exit 0
    fi
    echo "install-chromium: the cached packages didn't install; using apt" >&2
    fallback=true
fi

# apt's own archive folder won't do: some images (Debian's Docker image) delete
# its files after every install. This setting lasts for this install only.
conf=/etc/apt/apt.conf.d/99-ladderback-chromium-debs
trap 'sudo rm -f "$conf"' EXIT
# After a failed install the cached files stay: apt checks each against its
# hash and downloads only the ones that don't match, so the set it leaves is
# whole even though dpkg already installed most of the packages.
if [ "$fallback" = false ]; then
    sudo rm -rf "$debs"
fi
rm -f "$debs/complete"
mkdir -p "$debs/partial"
echo "Dir::Cache::Archives \"$debs/\";" | sudo tee "$conf" > /dev/null

apt_install() {
    if [ "$fallback" = true ]; then
        # dpkg left the packages that depend on one it couldn't install half
        # done, and apt refuses other work until they are finished.
        sudo apt-get update && sudo apt-get install --fix-broken --yes
    fi
    npx playwright install --with-deps chromium
}
apt_install
sudo rm -f "$conf"

# apt downloads as root; the cache reads them as the runner's user.
sudo chown -R "$(id -u):$(id -g)" "$debs"
rm -rf "$debs/partial" "$debs/lock"
touch "$debs/complete"
downloaded=("$debs"/*.deb)
echo "install-chromium: keeping ${#downloaded[@]} system packages for the cache"
echo "debs=saved" >> "${GITHUB_OUTPUT:-/dev/null}"
