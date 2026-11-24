# Other ways to install

[Install](install.md) names one recommended way for each system: the release's `.deb` or `.rpm` package where there is one, and the install script elsewhere, since a package manager that knows the program can update and remove it ([the design](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/126-primary-install-channels.md) has the reasons). This page has the rest, for contributors, for installing the release's archive by hand, and for people who already use cargo-binstall: what the install script does and its options, moving to the package, the archive by hand, cargo-binstall and a build by hand. The [Docker image](docker.md), which isn't a supported way to run Ladderback yet, has a page of its own. Ladderback isn't published to crates.io, and has no Homebrew tap until a macOS build can be signed and notarized ([#344](https://github.com/nledford/Ladderback/issues/344)).

[Decision 123](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/123-deb-and-rpm-packages.md) says why the packages aren't signed, and what checks them instead.

## The install script in detail

To read the script before it runs, download it, read it, then run it:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh -o install.sh
less install.sh
bash install.sh
```

On 64-bit Linux with glibc 2.35 or newer, the script downloads the newest release's archive and `SHA256SUMS`, as [Download a release](#download-a-release-linux) does, checks the archive against it, and installs the binary into `~/.cargo/bin`, where a build from source goes too, so an update either way replaces the same binary. It needs none of the build's tools and asks nothing. An archive that doesn't match its checksum stops it, naming the file, with nothing installed and nothing built instead; a download that stalls or fails stops it too, and it suggests `--from-source`. Where the release has no archive for the system (macOS, 32-bit or musl systems, a glibc older than 2.35, or a release whose archives aren't attached yet, in the hour after it is published), or with `--from-source`, it builds from source and says why.

To build, it checks the tools the build needs, listed under [By hand](#by-hand), and installs those that need no administrator rights (rustup, just and trunk) after asking you once. trunk comes from its prebuilt release, checked against the SHA-256 the script pins; only on a system trunk publishes no release for is it built from source, with one build job per 2 GB of free memory.

A download that stalls, finding no connection within 30 s or averaging under 1 KiB/s for 60 s, stops it, after three retries, with a message that says so; a slow one that still moves is left to finish.

Those that need them, a C compiler, git and Node.js 26 or newer, it never installs: it names the missing ones with the command that installs them and stops, and it never runs `sudo`.

It keeps Ladderback's source in `~/.local/share/ladderback/source`, checks out the newest release, and installs it with `just install` into `~/.cargo/bin`. The command runs the script on `main`, and the script installs the newest release, not `main` ([decision 79](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/79-a-from-source-install-script.md)).

Options go after `bash -s --`:

- `--from-source` builds from source on Linux too,
- `--yes` installs the missing tools without asking,
- `--dir <folder>` keeps the source elsewhere,
- and what follows `--` goes to `just install`, so `bash -s -- -- --root ~/.local` installs into `~/.local/bin`; from the archive only `--root` applies.

`--help` lists them.

## Moving an existing install to the package

If `ladderback` came from the archive, the install script or a build from source, it is in `~/.local/bin` or `~/.cargo/bin`, and the service names that path:

1. remove that binary: `rm ~/.cargo/bin/ladderback` (or `cargo uninstall ladderback`, or `rm ~/.local/bin/ladderback`), so the shell finds the package's;
2. install the package as [Install on Debian, Ubuntu, Raspberry Pi OS or Fedora](install-linux-package.md#download-check-and-install) says;
3. run `ladderback install-service` again, so the unit names `/usr/bin/ladderback`, not the binary you removed.

## Download a release (Linux)

Each release from 0.3.1 on has an archive for `x86_64` and one for `aarch64` Linux, each holding the `ladderback` binary with the web UI built in, and a `SHA256SUMS` file with their checksums. macOS has no archive until it can be signed and notarized ([decision 71](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/71-signing-macos-builds-without-a-developer-id.md)). Download the archive for your machine and the checksums, check the archive, and install the binary into `~/.local/bin`:

```sh
target="$(uname -m)-unknown-linux-gnu"
base=https://github.com/nledford/Ladderback/releases/latest/download
curl -fL -O "$base/ladderback-$target.tar.xz" -O "$base/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS
tar -xJf "ladderback-$target.tar.xz"
mkdir -p ~/.local/bin && install -m 755 "ladderback-$target/ladderback" ~/.local/bin/
```

`sha256sum` must print `ladderback-…tar.xz: OK`; anything else means the download is damaged or isn't the release's, so don't install it. On a Raspberry Pi, `uname -m` says `aarch64` under 32-bit Raspberry Pi OS too, whose 64-bit kernel runs 32-bit programs, and the binary then doesn't start: `getconf LONG_BIT` must print `64`. `~/.local/bin` must be on your `PATH`; if you built Ladderback from source before, `cargo uninstall ladderback` removes that one, so the shell doesn't find the old binary first. To update, run the same commands again, which replace the binary, then `ladderback install-service` again if it runs as a [service](running-as-a-service.md).

Releases after 0.3.1 list inside the binary the crates the daemon was built from, so [cargo-audit](https://crates.io/crates/cargo-audit) can check an installed one against the security advisories published since, without its source: `cargo audit bin "$(command -v ladderback)"`. The list leaves out the crates of the web UI built into it, and a binary you built from source has none; `cargo audit` in a checkout of the release's tag checks both. An advisory the project has found doesn't affect Ladderback is in the `ignore` list of [`deny.toml`](https://github.com/nledford/Ladderback/blob/main/deny.toml), with the reason.

## cargo-binstall

For people who already use [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), on 64-bit Linux:

```sh
cargo binstall --strategies crate-meta-data --git https://github.com/nledford/Ladderback ladderback
```

It reads the version in `main`'s `Cargo.toml`, downloads that release's archive for your machine and installs `ladderback` into `~/.cargo/bin`, as the install script does. It doesn't check the archive against `SHA256SUMS` ([#357](https://github.com/nledford/Ladderback/issues/357)); the install script and [Download a release](#download-a-release-linux) do. `--strategies crate-meta-data` keeps it to the release's archive: where there is none (macOS, or a release whose archives aren't attached yet), it would otherwise build with `cargo install`, which doesn't build the web UI, so the binary it installs has none. To update, run the same command again.

## By hand

You need:

- **Rust**, through [rustup](https://rustup.rs). In the checkout, rustup installs the exact Rust version `rust-toolchain.toml` pins, with the `wasm32-unknown-unknown` target, on the first build, so you build with the compiler CI tested ([decision 78](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/78-the-rust-toolchain-is-pinned.md)). Without rustup, use Rust 1.100.0 or newer, the [MSRV](https://github.com/nledford/Ladderback/blob/main/Cargo.toml), with that target, and take trunk and just from their prebuilt releases.
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

On macOS, `just install` and `just install-restart` use the self-signed certificate named `Ladderback Code Signing` by default. Set `LADDERBACK_SIGN_IDENTITY` to another certificate name or SHA-1 hash when you need a different identity; [Running as a service](running-as-a-service.md#macos-signing-and-the-keychain) has what `just install` checks.

To update by hand, `git fetch --tags`, the same `git checkout` line, and `just install`. After the install script installed the archive, run `just install --force`: cargo won't overwrite a binary it didn't install. Releases up to 0.2.0 track the web UI's stylesheet, which their build can rewrite: coming from one of them by hand, first put it back with `git checkout -- crates/web/tw-out.css`, or git refuses to check out the new release.
