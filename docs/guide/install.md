# Install Ladderback

On Linux, from 0.3.1 on, you can [download a release](#download-a-release-linux); on macOS, and on Linux too if you prefer, you build one from source. Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported. The [README](../../README.md#install) has the one command that installs it: the release's archive on Linux, a build from source on macOS. This guide says how to download a release, what that command does, how to build by hand, how to update, and how to keep Ladderback running in the background.

## Download a release (Linux)

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

## By hand

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

Start it with `ladderback serve` and open the address it prints, `listening on http://127.0.0.1:9898`; `--listen` picks another address or port (beyond loopback only with `--allowed-host`; [Remote access](remote-access.md) has the details). Its configuration and data live in your platform's config and data folders ([technical design §4.2](../design/components.md#42-daemon-agent), "Config").

## Updating

To update, run the install command again, or by hand `git fetch --tags`, the same `git checkout` line, and `just install`; either replaces the installed binary. By hand after the script installed the archive, run `just install --force`: cargo won't overwrite a binary it didn't install. Releases up to 0.2.0 track the web UI's stylesheet, which their build can rewrite: coming from one of them by hand, first put it back with `git checkout -- crates/web/tw-out.css`, or git refuses to check out the new release.

## Running as a service

To run it in the background instead, from each login on, run `ladderback install-service` as yourself, not under `sudo`. On Linux it writes a systemd user unit and has systemd start it; on macOS it writes a launchd agent, which logs to `~/Library/Logs/ladderback.log`. The service runs `ladderback serve --service-managed`, so it listens on `http://127.0.0.1:9898`, and Settings can restart it. On Linux a user's services stop when the user logs out, and unless you already linger the command says to run `loginctl enable-linger` to keep it running. A service that lingers starts before you log in, when it can't read a password in the keyring yet, so the command says so too when a repository keeps its password there, and suggests a [password file](secrets.md) instead.

**Restart Ladderback** in Settings restarts the service, now or once the running operations finish, and the page reloads once it is back (systemd waits 10 seconds before it starts the service again). A service installed by an earlier version can't be restarted from there: run `ladderback install-service` again first.

After an update, run `ladderback install-service` again: it replaces the service and starts it on the new binary. From a checkout, `just install-restart` does both without interrupting a backup: it runs `just install` (its other arguments go there), waits until the service's operation log shows no operation running or queued (up to 2 hours; `--wait <minutes>` changes that, `--now` skips the wait), restarts it, and checks that the service runs a new process of the build it installed. It doesn't restart a service when it can't tell whether it is busy, as when the service isn't running, unless `--now`. `INSTALL_RESTART_ADDRESS=<host:port>` names the address of a service whose file you edited to listen elsewhere than `127.0.0.1:9898`. `ladderback remove-service` stops the service and removes its file. [Decision 64](../design/decisions/64-installing-the-agent-as-a-user-service.md) says what the service file holds.

## macOS: Full Disk Access

On macOS, grant the installed `ladderback` Full Disk Access by hand, in System Settings → Privacy & Security, and again after each update: a build from source loses the grant when it is rebuilt ([technical design §12](../design/operations.md#12-packaging-and-deployment), [decision 71](../design/decisions/71-signing-macos-builds-without-a-developer-id.md)). A reinstall from source can also make macOS ask again for access to the keychain, to Photos and to Full Disk Access; [technical design §12](../design/operations.md#12-packaging-and-deployment) says why, and what a prompt you decline or leave unanswered costs.
