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
#
# The apt path needs the mirrors' names to resolve. On 2026-11-11 GitHub's
# runners couldn't resolve them, while Quad9 resolved them from outside GitHub's
# network. So it asks Quad9 first, with the runner's own DNS server after it
# (prefer_quad9), and tries apt twice.
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

# Puts Quad9 before the DNS servers of the network link the default route uses.
# systemd-resolved keeps asking the first server until it errs (a timeout or
# SERVFAIL, not "no such name"), then moves to the next, so the runner's own
# resolver answers when Quad9 can't. On the same link, not as a global server:
# whether a global one is asked depends on the link's routing domains
# (systemd-resolved.service(8), "Protocols and Routing"). Names only the
# runner's resolver knows, such as Azure's internal ones, stop resolving, and
# apt needs none. Lasts until the link's DNS settings change, which on a hosted
# runner is the end of the job. Any failure here leaves DNS as it was.
prefer_quad9() {
    local link='' servers=''
    if command -v ip > /dev/null && command -v resolvectl > /dev/null; then
        link=$(ip route show default 2> /dev/null | awk '{ print $5; exit }') || link=''
    fi
    if [ -z "$link" ]; then
        echo "install-chromium: no systemd-resolved link to give Quad9; using the runner's DNS" >&2
        return 0
    fi
    # "Link 2 (eth0): 168.63.129.16" -> "168.63.129.16"
    servers=$(resolvectl dns "$link" 2> /dev/null | sed 's/^[^:]*: *//') || servers=''
    # Word splitting is wanted: one argument per server.
    # shellcheck disable=SC2086
    if sudo resolvectl dns "$link" 9.9.9.9 149.112.112.112 $servers; then
        echo "install-chromium: DNS on $link: Quad9, then ${servers:-nothing else}"
    else
        echo "install-chromium: couldn't give Quad9 to $link; using the runner's DNS" >&2
    fi
}

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
prefer_quad9

apt_install() {
    if [ "$fallback" = true ]; then
        # dpkg left the packages that depend on one it couldn't install half
        # done, and apt refuses other work until they are finished.
        sudo apt-get update && sudo apt-get install --fix-broken --yes
    fi
    npx playwright install --with-deps chromium
}
if ! apt_install; then
    echo "install-chromium: apt failed; trying once more in 30 s" >&2
    sudo resolvectl flush-caches 2> /dev/null || true
    sleep 30
    apt_install
fi
sudo rm -f "$conf"

# apt downloads as root; the cache reads them as the runner's user.
sudo chown -R "$(id -u):$(id -g)" "$debs"
rm -rf "$debs/partial" "$debs/lock"
touch "$debs/complete"
downloaded=("$debs"/*.deb)
echo "install-chromium: keeping ${#downloaded[@]} system packages for the cache"
echo "debs=saved" >> "${GITHUB_OUTPUT:-/dev/null}"
