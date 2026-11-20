# Install Ladderback

Ladderback runs on each machine it backs up, and each system has one recommended way to install it ([decision 126](../design/decisions/126-primary-install-channels.md)):

- **Debian 12 or newer, Ubuntu 22.04 or newer, 64-bit Raspberry Pi OS and Fedora:** the release's [`.deb` or `.rpm` package](#install-a-package-debian-ubuntu-fedora), from 0.5.0.
- **Any other 64-bit Linux:** the [install script](#the-install-script), which downloads the release's archive and checks it.
- **macOS:** the [install script](#the-install-script), which builds the release from source, [signed with your own certificate](#signing-your-build) so that Full Disk Access carries over to the next update. macOS has no download until a build can be signed and notarized ([#344](https://github.com/nledford/Ladderback/issues/344)).
- **A hub:** not in a release yet; it arrives with M8, in the [Docker image](#docker-image-not-supported-yet).

Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported. [Other ways](#other-ways), for contributors and for people who already use cargo-binstall, follow: the release's archive by hand, cargo-binstall, and a build by hand. Then the guide says how to update, how to keep Ladderback running in the background, what macOS asks for, and how to try the [Docker image](#docker-image-not-supported-yet), which isn't a supported way to run Ladderback yet.

## Install a package (Debian, Ubuntu, Fedora)

From 0.5.0, each release has a `.deb` for Debian and Ubuntu and an `.rpm` for Fedora, for `x86_64` and `aarch64`, beside the archives and listed in the same `SHA256SUMS`. They put `ladderback` at `/usr/bin/ladderback`, need glibc 2.35 or newer, and start nothing: Ladderback runs as your own [service](#running-as-a-service) once you run `ladderback install-service`. Download the package for your machine, check it, and install it; `VERSION` is the release's, such as `0.5.0`:

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

`sha256sum` must print `…: OK`. The packages are not signed: `apt` and `dnf` install a file from disk without a repository's key, and `dnf` warns that it skipped the OpenPGP checks. The checksum is the check ([decision 123](../design/decisions/123-deb-and-rpm-packages.md)). The `./` matters to `apt`, which otherwise looks for a package of that name in its repositories. The packages are for 64-bit systems: on 32-bit Raspberry Pi OS `dpkg --print-architecture` says `armhf`, the download fails with a 404, and there is no package for it.

Then run `ladderback --version` and `ladderback install-service` as yourself, not under `sudo`. There is no apt or dnf repository yet, so to update, install the next release's package the same way, then run `ladderback install-service` again. `sudo apt remove ladderback` or `sudo dnf remove ladderback` removes the program and leaves your configuration, data and service unit as they are, so run `ladderback remove-service` first if you want the service gone too. On Atomic Fedora (Bazzite) a package needs `rpm-ostree` layering: use the [install script](#the-install-script) there.

**Moving an existing install to the package.** If `ladderback` came from the archive, the install script or a build from source, it is in `~/.local/bin` or `~/.cargo/bin`, and the service names that path:

1. remove that binary: `rm ~/.cargo/bin/ladderback` (or `cargo uninstall ladderback`, or `rm ~/.local/bin/ladderback`), so the shell finds the package's;
2. install the package as above;
3. run `ladderback install-service` again, so the unit names `/usr/bin/ladderback`, not the binary you removed.

## The install script

On 64-bit Linux with glibc 2.35 or newer, the script downloads the newest release's archive and `SHA256SUMS`, as [Download a release](#download-a-release-linux) does, checks the archive against it, and installs the binary into `~/.cargo/bin`, where a build from source goes too, so an update either way replaces the same binary. It needs none of the build's tools and asks nothing. An archive that doesn't match its checksum stops it, naming the file, with nothing installed and nothing built instead; a download that stalls or fails stops it too, and it suggests `--from-source`. Where the release has no archive for the system (macOS, 32-bit or musl systems, a glibc older than 2.35, or a release whose archives aren't attached yet, in the hour after it is published), or with `--from-source`, it builds from source and says why.

To build, it checks the tools the build needs, listed under [By hand](#by-hand), and installs those that need no administrator rights (rustup, just and trunk) after asking you once. trunk comes from its prebuilt release, checked against the SHA-256 the script pins; only on a system trunk publishes no release for is it built from source, with one build job per 2 GB of free memory.

A download that stalls, finding no connection within 30 s or averaging under 1 KiB/s for 60 s, stops it, after three retries, with a message that says so; a slow one that still moves is left to finish.

Those that need them, a C compiler, git and Node.js 26 or newer, it never installs: it names the missing ones with the command that installs them and stops, and it never runs `sudo`.

It keeps Ladderback's source in `~/.local/share/ladderback/source`, checks out the newest release, and installs it with `just install` into `~/.cargo/bin`. The command runs the script on `main`, and the script installs the newest release, not `main` ([decision 79](../design/decisions/79-a-from-source-install-script.md)).

Options go after `bash -s --`:

- `--from-source` builds from source on Linux too,
- `--yes` installs the missing tools without asking,
- `--dir <folder>` keeps the source elsewhere,
- and what follows `--` goes to `just install`, so `bash -s -- -- --root ~/.local` installs into `~/.local/bin`; from the archive only `--root` applies.

`--help` lists them.

**On macOS, sign the build.** Make the certificate that [Signing your build](#signing-your-build) describes once, then name it each time you run the script, so that the build's Full Disk Access and Photos grants carry over to the next update:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" bash
```

Without it the build is signed ad hoc, and macOS asks again for both after every update: until you grant Full Disk Access again, backups can't read the folders it protects.

## Other ways

These are for contributors, for installing the release's archive by hand, and for people who already use cargo-binstall. Ladderback isn't published to crates.io, and has no Homebrew tap until a macOS build can be signed and notarized ([#344](https://github.com/nledford/Ladderback/issues/344)).

### Download a release (Linux)

Each release from 0.3.1 on has an archive for `x86_64` and one for `aarch64` Linux, each holding the `ladderback` binary with the web UI built in, and a `SHA256SUMS` file with their checksums. macOS has no archive until it can be signed and notarized ([decision 71](../design/decisions/71-signing-macos-builds-without-a-developer-id.md)). Download the archive for your machine and the checksums, check the archive, and install the binary into `~/.local/bin`:

```sh
target="$(uname -m)-unknown-linux-gnu"
base=https://github.com/nledford/Ladderback/releases/latest/download
curl -fL -O "$base/ladderback-$target.tar.xz" -O "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS
tar -xJf "ladderback-$target.tar.xz"
mkdir -p ~/.local/bin && install -m 755 "ladderback-$target/ladderback" ~/.local/bin/
```

`sha256sum` must print `ladderback-…tar.xz: OK`; anything else means the download is damaged or isn't the release's, so don't install it. On a Raspberry Pi, `uname -m` says `aarch64` under 32-bit Raspberry Pi OS too, whose 64-bit kernel runs 32-bit programs, and the binary then doesn't start: `getconf LONG_BIT` must print `64`. `~/.local/bin` must be on your `PATH`; if you built Ladderback from source before, `cargo uninstall ladderback` removes that one, so the shell doesn't find the old binary first. To update, run the same commands again, which replace the binary, then `ladderback install-service` again if it runs as a [service](#running-as-a-service).

Releases after 0.3.1 list inside the binary the crates the daemon was built from, so [cargo-audit](https://crates.io/crates/cargo-audit) can check an installed one against the security advisories published since, without its source: `cargo audit bin "$(command -v ladderback)"`. The list leaves out the crates of the web UI built into it, and a binary you built from source has none; `cargo audit` in a checkout of the release's tag checks both. An advisory the project has found doesn't affect Ladderback is in the `ignore` list of [`deny.toml`](../../deny.toml), with the reason.

### cargo-binstall

For people who already use [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), on 64-bit Linux:

```sh
cargo binstall --strategies crate-meta-data --git https://github.com/nledford/Ladderback ladderback
```

It reads the version in `main`'s `Cargo.toml`, downloads that release's archive for your machine and installs `ladderback` into `~/.cargo/bin`, as the install script does. It doesn't check the archive against `SHA256SUMS` ([#357](https://github.com/nledford/Ladderback/issues/357)); the install script and [Download a release](#download-a-release-linux) do. `--strategies crate-meta-data` keeps it to the release's archive: where there is none (macOS, or a release whose archives aren't attached yet), it would otherwise build with `cargo install`, which doesn't build the web UI, so the binary it installs has none. To update, run the same command again.

### By hand

You need:

- **Rust**, through [rustup](https://rustup.rs). In the checkout, rustup installs the exact Rust version `rust-toolchain.toml` pins, with the `wasm32-unknown-unknown` target, on the first build, so you build with the compiler CI tested ([decision 78](../design/decisions/78-the-rust-toolchain-is-pinned.md)). Without rustup, use Rust 1.100.0 or newer, the [MSRV](../../Cargo.toml), with that target, and take trunk and just from their prebuilt releases.
- **git and a C compiler**: Xcode's Command Line Tools on macOS, `git` and `build-essential` or your distribution's equivalents on Linux.
- **Node.js** 26 or newer, with npm, ideally the newest 26 release: the web UI's build runs Tailwind's CLI, and `just install-restart` is TypeScript, which Node runs as it is. Debian's and Ubuntu's own packages are older: take it from [nodejs.org](https://nodejs.org) or [NodeSource's signed apt repository](https://github.com/nodesource/distributions).
- **[trunk](https://trunkrs.dev)**, which builds the web UI, and **[just](https://just.systems)** 1.40 or newer: `cargo +stable install --locked trunk just`, or your package manager's. trunk's newest release can need a newer Rust than the one the checkout pins, hence `+stable`. Building trunk from source takes more memory than a small machine has (with cargo's default jobs it ran a 4 GB Raspberry Pi out of memory), so prefer trunk's [prebuilt release](https://github.com/trunk-rs/trunk/releases), on Linux the `musl` archive, which runs whatever the system's glibc; where there is none for your system, bound the build to about one job per 2 GB of free memory with `-j`. trunk downloads the `wasm-bindgen` and `wasm-opt` the build needs.

On 64-bit ARM Linux with a glibc older than 2.38 (Debian 12, Ubuntu 22.04), the `wasm-bindgen` trunk downloads doesn't run. There, between the `git checkout` and `just install` below, and again after each update, build the version the checkout pins, which trunk then finds on the `PATH`: `cargo install --locked wasm-bindgen-cli --version "$(just _wasm-bindgen-version)"`.

Clone the repository, check out the newest release, and install it:

```sh
git clone https://github.com/nledford/Ladderback.git
cd Ladderback
# The newest release tag, leaving out release candidates (vX.Y.Z-rc.N).
git checkout "$(git tag --list 'v*' --sort=-v:refname | grep -v -- - | head -n 1)" && just install
```

`just install` builds the web UI, then `ladderback` with the UI built in, as the release archives are built (the `dist` profile, with thin LTO), and installs it with `cargo install` into `~/.cargo/bin`. Arguments go to `cargo install`: `just install --root ~/.local` installs into `~/.local/bin`. If `just` answers that there is no recipe `install`, that release predates it; run this instead, which builds with the `release` profile (older releases have no `dist` profile):

```sh
(cd crates/web && trunk build --release --no-default-features) && cargo install --locked --path . --no-default-features
```

## After installing

Start it with `ladderback serve` and open the address it prints, `listening on http://127.0.0.1:9898`; `--listen` picks another address or port (beyond loopback only with `--allowed-host`; [Remote access](remote-access.md) has the details). Its configuration and data live in your platform's config and data folders ([technical design §4.2](../design/components/daemon.md#42-daemon-agent), "Config").

## Updating

To update a package, install the next release's, then run `ladderback install-service` again (see [Install a package](#install-a-package-debian-ubuntu-fedora)). To update, run the install script again (on macOS with `LADDERBACK_SIGN_IDENTITY` again), or by hand `git fetch --tags`, the same `git checkout` line, and `just install`; either replaces the installed binary. By hand after the script installed the archive, run `just install --force`: cargo won't overwrite a binary it didn't install. Releases up to 0.2.0 track the web UI's stylesheet, which their build can rewrite: coming from one of them by hand, first put it back with `git checkout -- crates/web/tw-out.css`, or git refuses to check out the new release.

## Running as a service

To run it in the background instead, from each login on, run `ladderback install-service` as yourself, not under `sudo`. On Linux it writes a systemd user unit and has systemd start it; on macOS it writes a launchd agent, which logs to `~/Library/Logs/ladderback.log`. The service runs `ladderback serve --service-managed`, so it listens on `http://127.0.0.1:9898`, and Settings can restart it. On Linux a user's services stop when the user logs out, and unless you already linger the command says to run `loginctl enable-linger` to keep it running. A service that lingers starts before you log in, when it can't read a password in the keyring yet, so the command says so too when a repository keeps its password there, and suggests a [password file](secrets.md) instead.

**Restart Ladderback** in Settings restarts the service, now or once the running operations finish, and the page reloads once it is back (systemd waits 10 seconds before it starts the service again). A service installed by an earlier version can't be restarted from there: run `ladderback install-service` again first.

If the service stops without its service manager asking it to, after a crash or a `kill` from another program, the manager starts it again: systemd after 10 seconds, launchd within 10 seconds. So a `kill` doesn't stop it, nor free its data folder for another `ladderback serve`. To stop it until your next login, run `systemctl --user stop ladderback` on Linux or `launchctl bootout gui/$(id -u)/io.github.nledford.ladderback` on macOS. A service installed by an earlier version stays stopped after another program's `kill`: run `ladderback install-service` again.

After an update, run `ladderback install-service` again: it replaces the service and starts it on the new binary. From a checkout, `just install-restart` does both without interrupting a backup: it builds with `just install` (its other arguments go there) into a staging folder beside the installed `ladderback`, waits until the service's operation log shows no operation running or queued (up to 2 hours; `--wait <minutes>` changes that, `--now` skips the wait), then moves the new binary into place, restarts the service at once, and checks that it runs a new process of that build. Until the restart the service's binary stays in place, so a backup that starts during the wait can still read a keyring password. It doesn't restart a service when it can't tell whether it is busy, as when the service isn't running, unless `--now`.

`INSTALL_RESTART_ADDRESS=<host:port>` names the address of a service whose file you edited to listen elsewhere than `127.0.0.1:9898`. A hidden `.ladderback-install-` folder beside `ladderback` is one a run that was killed left behind: delete it. `ladderback remove-service` stops the service and removes its file. [Decision 64](../design/decisions/64-installing-the-agent-as-a-user-service.md) says what the service file holds.

## macOS: Full Disk Access

On macOS, grant the installed `ladderback` Full Disk Access by hand, in System Settings → Privacy & Security. A build from source signed ad hoc loses the grant when it is rebuilt, so then grant it again after each update ([technical design §12](../design/operations.md#12-packaging-and-deployment), [decision 71](../design/decisions/71-signing-macos-builds-without-a-developer-id.md)); [signing your build](#signing-your-build) keeps it. A reinstall of an unsigned build can also make macOS ask again for access to the keychain, to Photos and to Full Disk Access; [technical design §12](../design/operations.md#12-packaging-and-deployment) says why, and what a prompt you decline or leave unanswered costs.

### Signing your build

A build from source carries the linker's ad-hoc signature, which changes with every build, so macOS sees each update as a new program: it drops the Full Disk Access grant and asks again for Photos and the keychain. Signed with a certificate from your own keychain under one fixed identifier, every build has the same signature to macOS, so its Full Disk Access and Photos grants carry over to the next update (tested on macOS 27.3, [#281](https://github.com/nledford/Ladderback/issues/281)). No Apple Developer account is needed.

The keychain still asks once for each new build, signed or not, before it lets the build read a keyring entry: a self-signed certificate has no Apple Team ID, so "Always Allow" lets in only the build that was answered ([#892](https://github.com/nledford/Ladderback/issues/892), [technical design §12](../design/operations.md#12-packaging-and-deployment)). Left unanswered, the prompt fails the first scheduled backup, or notification, after an update. Any of these avoids that:

- `just install-restart` reads each keyring entry once with the new program, a repository's password and a notification channel's address, token or password, in your terminal, before it restarts the service, so the keychain asks while you are there. Answer "Always Allow": a single "Allow" is for that one read, and the service is asked again. It stops before the restart when an entry can't be read; `--ignore-keyring-failure` restarts all the same. After any other update, run `ladderback check-keyring` yourself.
- Read the entry with Apple's `security` program, which a new build of Ladderback doesn't change: `password-command = "security find-generic-password -s ladderback -a nas -w"` for the entry `ladderback/nas`. The keychain may ask once for `security` itself, so run that command once in a terminal and answer "Always Allow".
- Keep the password in a `password-file` only you can read (`chmod 600`).

Make the certificate once:

1. In Keychain Access, choose Keychain Access → Certificate Assistant → Create a Certificate. Give it a name, such as `Ladderback Code Signing`, set Identity Type to Self Signed Root and Certificate Type to Code Signing, and create it. It goes into your login keychain.
2. Open the new certificate, expand Trust, and set Code Signing to Always Trust.
3. Check that macOS lists it as valid: `security find-identity -v -p codesigning` names it.

Then name it in `LADDERBACK_SIGN_IDENTITY` for every install: when you run the install script, as [The install script](#the-install-script) shows, or with `just install` or `just install-restart` in a checkout:

```sh
LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" just install-restart
```

The install script passes the variable on to the `just install` it runs. It builds the newest release, and every release from 0.4.0 on signs its build; releases up to 0.3.1 sign nothing.

`just install` checks the name before it builds, and installs nothing when `security` lists no valid certificate of exactly that name, or lists more than one: then name one by the SHA-1 hash it shows, or, when it is one certificate kept in two keychains, delete a copy. After installing, it signs the binary with the certificate under the identifier `io.github.nledford.ladderback` and checks the signature. Only the name passes through; the key stays in your keychain.

The first signing asks whether `codesign` may use the key: choose Always Allow, and later installs don't ask. Over SSH, or with the keychain locked, nobody can answer, so the signing fails. After `just install`, the new binary is installed by then, still with its ad-hoc signature (or, when the signature fails its check, with that signature), and the command says which. To sign the installed binary without building again, run `node scripts/install-signing.ts sign` in the checkout with the variable and the arguments you gave `just install`. `just install-restart` signs in its staging folder, so there a failed signing installs nothing and the service keeps running as it was: run it again once the keychain can answer. The first signed build is still new to macOS, so grant it Full Disk Access once more.

## Docker image (not supported yet)

Each release after 0.5.0 publishes a Docker image for x86_64 and aarch64 Linux, as `ghcr.io/nledford/ladderback` and `docker.io/nledford/ladderback`: the same image, tagged with the release's version and `latest`. For 0.5.0, or a build of your own, `just image <version> <archive>... --tag ghcr.io/nledford/ladderback --load`, in a checkout, builds it from a release archive, as a release does. Docker Hub limits how often you may pull without an account; GitHub's registry doesn't. The image is there to try: it isn't yet a supported way to install and run Ladderback, and the package or the install script above is. It runs a machine's agent, which backs up the folders mounted into it. From M8's release it runs the hub too, and becomes the recommended way to run one ([decision 126](../design/decisions/126-primary-install-channels.md)); until then it has no hub, and refuses `serve --hub`.

The image runs `ladderback` as user and group 10001 and listens on port 9898. It keeps its configuration in the volume at `/var/lib/ladderback/config`, its data (the operation history and the admin password among them) in the volume at `/var/lib/ladderback/data`, and its cache in `/var/cache/ladderback`. Turn sign-in on first: `set-password` asks twice for an admin password of at least 8 characters, without showing it, and keeps it in the data volume. With `-i` alone in place of `-it`, it reads one line from a pipe instead.

```sh
docker run --rm -it -v ladderback-data:/var/lib/ladderback/data ghcr.io/nledford/ladderback set-password
```

Then start the agent, here on a machine that browsers reach as `nas.example.com`, backing up `/srv/photos`:

```sh
docker run --detach --name ladderback \
  --hostname nas --init --restart unless-stopped --stop-timeout 75 \
  --env TZ=Europe/Berlin --publish 9898:9898 \
  -v ladderback-config:/var/lib/ladderback/config \
  -v ladderback-data:/var/lib/ladderback/data \
  -v ladderback-cache:/var/cache/ladderback \
  -v /srv/photos:/srv/photos:ro \
  ghcr.io/nledford/ladderback \
  serve --listen 0.0.0.0:9898 --allowed-host nas.example.com --service-managed
```

The same agent as a Compose file, with its port on loopback for a reverse proxy on the same machine, is [`examples/compose.yaml`](examples/compose.yaml): turn sign-in on with `docker compose run --rm ladderback set-password`, then start it with `docker compose up --detach`.

What each part is for:

- **`--allowed-host`** names the host name browsers reach Ladderback by. Listening beyond the container's loopback, Ladderback refuses to start without one, so the image's own command, `serve --listen 0.0.0.0:9898`, doesn't start until you give it ([Remote access](remote-access.md#listening-beyond-loopback)). Over plain HTTP the password crosses the network unencrypted, so put a reverse proxy that adds TLS in front. With the proxy on the same machine, publish the port on loopback only, `--publish 127.0.0.1:9898:9898`: Docker's own firewall rules pass by ufw and firewalld.
- **`--hostname`** stays the same from one container to the next. Ladderback names the machine, and its snapshots' host, by the container's hostname, which Docker makes up anew for each container unless you give one.
- **Each folder to back up is mounted read-only, at the path it has on the host.** The snapshots record the paths inside the container, so the same path keeps them meaningful to a restore and to `rustic`. A restore into a mounted folder needs it mounted without `:ro`, and writes files as the container's user, or with their original owners when it runs as root.
- **`--init`** runs a small init as process 1, which hands on `docker stop`'s signal and reaps the processes hooks leave behind. **`--stop-timeout 75`** gives Ladderback the 65 seconds it may take to stop cleanly, where Docker kills it after 10; in Compose, `stop_grace_period: 75s`.
- **`--restart unless-stopped`** stands in for `install-service`, which a container can't use, and `--service-managed` lets **Restart Ladderback** in Settings restart it: Ladderback exits, and Docker starts it again.
- **`TZ`** sets the time zone the schedules run in; without it, a container's is UTC.
- **The cache volume** is optional: it keeps the repositories' cache from one container to the next.

The container's user reads only what user 10001 or everyone may read. To back up a whole machine, run as root with `--user 0`: root reads every file mounted into the container, and the volumes need no change. To run as one of the host's users instead, with `--user <uid>:<gid>`, keep the configuration, data and cache in folders on the host that this user owns, made as that user:

```sh
install -d -m 0700 ~/ladderback/config ~/ladderback/data ~/ladderback/cache
```

Mount them in place of the volumes, as in `-v ~/ladderback/data:/var/lib/ladderback/data`, and give the same `--user`, in every `docker run`, `set-password`'s included. A named volume won't do: while it is empty, each mount gives it back to user 10001, so a `chown` of one doesn't last. Keep to one user, since the files root writes stay root's.

A container has no keyring, so keep a repository's password in a file, such as a Docker secret under `/run/secrets`, in an environment variable or behind a command ([Secrets](secrets.md)). A repository on a local path needs a volume or a folder on the host mounted at that path, as `/srv/photos` is above: one written anywhere else lives in the container and goes when the container is removed. To update, pull the image, remove the container and run the same `docker run` again, with the same mounts: the volumes keep the configuration, the history and the password, and the repositories mounted into it stay where they are.

On macOS, Docker runs Linux in a virtual machine, so an agent there sees only the folders shared with it and gets no Full Disk Access, Photos library or keychain: use the install script. From M8's release, the same image runs the hub with `serve --hub --listen 0.0.0.0:9898 --allowed-host <name>` in place of the agent's command; this guide will say how to pair machines with it then.
