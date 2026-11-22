# Install on other Linux

On any other 64-bit Linux with glibc 2.35 or newer, such as Arch Linux, openSUSE or Atomic Fedora (Bazzite), the install script installs Ladderback. One command downloads the newest release, checks it against the release's checksums, and installs `ladderback` into `~/.cargo/bin`, asking nothing:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | bash
```

When it finishes, the script names the version it installed and what to run next. If it says to open a new terminal, or that your PATH doesn't lead to `ladderback`, do that first, putting `~/.cargo/bin` on your PATH, or the next command isn't found. If it says that nothing answers for a Secret Service, Ladderback can't read passwords from the system keyring until you start one, such as GNOME Keyring or KWallet; [Getting started](getting-started.md) keeps a repository's password in the configuration file on Linux, which needs none.

Then [start Ladderback](install.md#start-ladderback).

[The install script in detail](other-ways-to-install.md#the-install-script-in-detail), in the Advanced part, says how to read the script before it runs, what it checks, and its options, such as `--from-source`, which builds the release on your computer instead of downloading it. Where the release has no download for your system, the script builds from source by itself, which needs build tools the same page lists. To download and check the release's archive yourself, see [Download a release](other-ways-to-install.md#download-a-release-linux).

## Updating

Run the same command again: it installs the newest release over the one you have. Then run `ladderback install-service` again, which restarts Ladderback on the new release. The web UI's header says when a new version is available.

## Removing

Stop Ladderback and remove its service, then the program:

```sh
ladderback remove-service
rm ~/.cargo/bin/ladderback
```

If the script built Ladderback from source, it also left the source in `~/.local/share/ladderback/source`, inside Ladderback's data folder, and the build tools it installed: Rust, through rustup, and just and trunk in `~/.cargo/bin`. Remove those only if nothing else uses them: `rustup self uninstall` removes Rust and the whole `~/.cargo` folder, with any other program installed there.

Your settings, history and backups stay where they are: [What removing leaves behind](install.md#what-removing-leaves-behind) lists them, and how to delete them too.
